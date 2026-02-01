//! Full end-to-end integration tests for Mnemo proxy flow
//!
//! Tests the complete proxy pipeline:
//! - Request -> Memory Injection -> Mock Upstream -> Response -> Capture -> Ingestion
//!
//! These tests verify that all components work together correctly without
//! requiring external services. Mock servers simulate the upstream LLM API.
//!
//! IMPORTANT: Run with --test-threads=1 due to ML model loading contention.
//! `cargo test -p mnemo --test integration_test -- --test-threads=1`

use axum::{
    Json, Router,
    body::Body,
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use chrono::{Duration, Utc};
use serde_json::json;
use tempfile::TempDir;
use tower::ServiceExt;

use mnemo::memory::ingestion::IngestionPipeline;
use mnemo::memory::retrieval::{RetrievalPipeline, RetrievedMemory};
use mnemo::memory::types::{CompressionLevel, Memory, MemorySource, MemoryType, StorageTier};
use mnemo::proxy::{
    ResponseCapture, SseEvent, StreamingProxy, estimate_tokens, extract_user_query,
    format_memory_block, inject_memories, truncate_to_budget,
};
use mnemo::storage::LanceStore;
use mnemo::storage::compaction::Compactor;
use mnemo::storage::eviction::{CapacityStatus, EvictionConfig, Evictor};
use mnemo::storage::tiers::TierManager;
use mnemo::testing::SHARED_EMBEDDING_MODEL;

// =============================================================================
// Test Fixtures and Helpers
// =============================================================================

async fn create_test_store() -> (LanceStore, TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
    store.create_memories_table().await.unwrap();
    store.create_tombstones_table().await.unwrap();
    (store, temp_dir)
}

async fn create_store_at_path(path: &std::path::Path) -> LanceStore {
    let mut store = LanceStore::connect(path).await.unwrap();
    store.open_memories_table().await.unwrap();
    store
}

/// Create a test memory with specified content and type
fn create_test_memory(content: &str, memory_type: MemoryType) -> Memory {
    let mut memory = Memory::new(
        content.to_string(),
        vec![0.5; 384],
        memory_type,
        MemorySource::Manual,
    );
    memory.created_at = Utc::now();
    memory
}

/// Create a memory with specific age for testing tier/compaction
fn create_memory_with_age(content: &str, age_days: i64, tier: StorageTier) -> Memory {
    let mut memory = Memory::new(
        content.to_string(),
        vec![0.1; 384],
        MemoryType::Semantic,
        MemorySource::Manual,
    );
    memory.created_at = Utc::now() - Duration::days(age_days);
    memory.tier = tier;
    memory.weight = 0.5;
    memory
}

/// Create a retrieved memory for injection testing
fn create_retrieved_memory(content: &str, memory_type: MemoryType) -> RetrievedMemory {
    RetrievedMemory {
        memory: create_test_memory(content, memory_type),
        similarity_score: 0.9,
        effective_weight: 0.8,
        final_score: 0.85,
    }
}

/// Create a valid OpenAI-format chat request
fn create_chat_request(user_message: &str) -> serde_json::Value {
    json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": user_message}
        ],
        "temperature": 0.7,
        "stream": true
    })
}

/// Generate mock SSE streaming response (OpenAI format)
fn generate_mock_sse_response(content: &str) -> String {
    let mut response = String::new();

    // First event: role delta
    response.push_str(&format!(
        "data: {{\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":{},\"model\":\"gpt-4\",\"choices\":[{{\"index\":0,\"delta\":{{\"role\":\"assistant\"}},\"finish_reason\":null}}]}}\n\n",
        Utc::now().timestamp()
    ));

    // Content deltas (split by words for realism)
    for word in content.split_whitespace() {
        response.push_str(&format!(
            "data: {{\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":{},\"model\":\"gpt-4\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{} \"}},\"finish_reason\":null}}]}}\n\n",
            Utc::now().timestamp(),
            word
        ));
    }

    // Final delta with finish_reason
    response.push_str(&format!(
        "data: {{\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":{},\"model\":\"gpt-4\",\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\n",
        Utc::now().timestamp()
    ));

    // Done marker
    response.push_str("data: [DONE]\n\n");

    response
}

// =============================================================================
// Mock Upstream Server for Testing
// =============================================================================

/// Mock handler that returns a streaming SSE response
async fn mock_chat_completions() -> impl IntoResponse {
    let content =
        "The capital of France is Paris. It is a beautiful city with many historic landmarks.";
    let sse_response = generate_mock_sse_response(content);

    (
        StatusCode::OK,
        [
            ("content-type", "text/event-stream"),
            ("cache-control", "no-cache"),
        ],
        sse_response,
    )
}

/// Create a mock upstream server router
fn create_mock_upstream() -> Router {
    Router::new()
        .route("/v1/chat/completions", post(mock_chat_completions))
        .route("/health", get(|| async { Json(json!({"status": "ok"})) }))
}

// =============================================================================
// Full Proxy Flow Tests
// =============================================================================

mod full_proxy_flow_tests {
    use super::*;

    #[tokio::test]
    async fn test_end_to_end_memory_flow() {
        let (store, temp_dir) = create_test_store().await;
        let store_path = temp_dir.path().to_path_buf();

        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let _ = pipeline
            .ingest(
                "The user's favorite programming language is Rust.",
                MemorySource::Conversation,
                Some("conv-001".to_string()),
            )
            .await
            .unwrap();

        let _ = pipeline
            .ingest(
                "User mentioned they prefer dark mode in all applications.",
                MemorySource::Conversation,
                Some("conv-001".to_string()),
            )
            .await
            .unwrap();

        let _ = pipeline
            .ingest(
                "The meeting with John about the database migration is tomorrow at 3pm.",
                MemorySource::Conversation,
                Some("conv-002".to_string()),
            )
            .await
            .unwrap();

        let store = create_store_at_path(&store_path).await;
        let count = store.count_by_tier(StorageTier::Hot).await.unwrap();
        assert!(count >= 3, "Should have at least 3 memories ingested");

        let mut retrieval = RetrievalPipeline::with_defaults(&store, &*SHARED_EMBEDDING_MODEL);

        let results = retrieval
            .retrieve("What programming language does the user prefer?", 5)
            .await
            .unwrap();

        assert!(
            !results.is_empty(),
            "Should retrieve at least one relevant memory"
        );

        // Verify retrieval found the Rust-related memory
        let rust_memory = results.iter().find(|r| r.memory.content.contains("Rust"));
        assert!(rust_memory.is_some(), "Should find the Rust memory");

        // Step 3: Format memories for injection
        let memory_block = format_memory_block(&results);
        assert!(
            memory_block.contains("<mnemo-memories>"),
            "Should have XML wrapper"
        );
        assert!(
            memory_block.contains("</mnemo-memories>"),
            "Should have closing XML tag"
        );

        // Step 4: Inject memories into a request
        let mut request = create_chat_request("What programming language should I use?");
        inject_memories(&mut request, &results, 2000).unwrap();

        // Verify injection
        let messages = request["messages"].as_array().unwrap();
        let system_content = messages[0]["content"].as_str().unwrap();
        assert!(
            system_content.contains("<mnemo-memories>"),
            "System message should contain injected memories"
        );
    }

    /// Test that streaming responses can be captured and parsed
    #[tokio::test]
    async fn test_streaming_response_capture() {
        let content = "This is a test response about Rust programming.";
        let sse_data = generate_mock_sse_response(content);

        // Parse the SSE events
        let events = StreamingProxy::parse_sse_events(&sse_data);
        assert!(!events.is_empty(), "Should parse SSE events");

        // Verify [DONE] marker is present
        let has_done = events.iter().any(|e| matches!(e, SseEvent::Done));
        assert!(has_done, "Should have [DONE] marker");

        // Extract the response content
        let extracted = StreamingProxy::extract_response_content(&sse_data);
        assert!(extracted.is_complete, "Response should be marked complete");
        assert!(
            extracted.content.contains("test response"),
            "Should extract content"
        );
    }

    #[tokio::test]
    async fn test_response_capture_and_ingestion() {
        let (store, _temp_dir) = create_test_store().await;

        let content =
            "Python is excellent for data science because of libraries like NumPy and Pandas.";
        let sse_data = generate_mock_sse_response(content);

        let extracted = StreamingProxy::extract_response_content(&sse_data);
        assert!(extracted.is_complete);

        assert!(
            ResponseCapture::should_ingest(&extracted.content),
            "Content should pass ingestion filters"
        );

        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");
        let result = pipeline
            .ingest(
                &extracted.content,
                MemorySource::Conversation,
                Some("conv-capture-test".to_string()),
            )
            .await
            .unwrap();

        assert!(
            result.is_some(),
            "Should successfully ingest captured content"
        );
        let memory = result.unwrap();
        assert_eq!(memory.memory_type, MemoryType::Episodic);
        assert_eq!(memory.tier, StorageTier::Hot);
    }

    /// Test mock upstream server responds correctly
    #[tokio::test]
    async fn test_mock_upstream_server() {
        let app = create_mock_upstream();

        // Test health endpoint
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Test chat completions endpoint
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "model": "gpt-4",
                            "messages": [{"role": "user", "content": "Hello"}],
                            "stream": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Verify streaming response format
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8_lossy(&body);

        assert!(body_str.contains("data:"), "Should be SSE format");
        assert!(body_str.contains("[DONE]"), "Should have DONE marker");
    }
}

// =============================================================================
// Memory Injection Tests
// =============================================================================

mod memory_injection_tests {
    use super::*;

    /// Test that injection respects token budget
    #[tokio::test]
    async fn test_injection_token_budget() {
        let memories: Vec<RetrievedMemory> = (0..10)
            .map(|i| {
                create_retrieved_memory(
                    &format!("This is memory number {i} with some content."),
                    MemoryType::Semantic,
                )
            })
            .collect();

        // Small budget should truncate
        let truncated = truncate_to_budget(&memories, 100);
        assert!(
            truncated.len() < memories.len(),
            "Should truncate with small budget"
        );

        // Large budget should keep all
        let all = truncate_to_budget(&memories, 10000);
        assert_eq!(
            all.len(),
            memories.len(),
            "Should keep all with large budget"
        );
    }

    /// Test memory block XML formatting
    #[tokio::test]
    async fn test_memory_block_xml_format() {
        let memories = vec![
            create_retrieved_memory("User likes Python.", MemoryType::Episodic),
            create_retrieved_memory("Database migration scheduled.", MemoryType::Semantic),
            create_retrieved_memory("How to deploy to AWS.", MemoryType::Procedural),
        ];

        let block = format_memory_block(&memories);

        // Verify XML structure
        assert!(block.starts_with("<mnemo-memories>"));
        assert!(block.ends_with("</mnemo-memories>"));
        assert!(block.contains("<memory"));
        assert!(block.contains("</memory>"));

        // Verify all types are present
        assert!(block.contains("type=\"episodic\""));
        assert!(block.contains("type=\"semantic\""));
        assert!(block.contains("type=\"procedural\""));

        // Verify content is preserved
        assert!(block.contains("User likes Python."));
        assert!(block.contains("Database migration scheduled."));
        assert!(block.contains("How to deploy to AWS."));
    }

    /// Test user query extraction from complex conversations
    #[tokio::test]
    async fn test_user_query_extraction() {
        // Simple case
        let simple = json!({
            "messages": [
                {"role": "user", "content": "What is Rust?"}
            ]
        });
        assert_eq!(
            extract_user_query(&simple),
            Some("What is Rust?".to_string())
        );

        // Multi-turn conversation
        let multi_turn = json!({
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "First question"},
                {"role": "assistant", "content": "First answer"},
                {"role": "user", "content": "Follow-up question"}
            ]
        });
        assert_eq!(
            extract_user_query(&multi_turn),
            Some("Follow-up question".to_string())
        );

        // No user message
        let no_user = json!({
            "messages": [
                {"role": "system", "content": "System only"}
            ]
        });
        assert_eq!(extract_user_query(&no_user), None);
    }
}

// =============================================================================
// Ingestion Pipeline Tests
// =============================================================================

mod ingestion_tests {
    use super::*;

    #[tokio::test]
    async fn test_ingestion_pipeline_creates_complete_memory() {
        let (store, _temp_dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline
            .ingest(
                "John Smith from Microsoft discussed the new Azure features yesterday.",
                MemorySource::Conversation,
                Some("meeting-001".to_string()),
            )
            .await
            .unwrap();

        assert!(result.is_some());
        let memory = result.unwrap();

        // Verify memory fields
        assert_eq!(memory.memory_type, MemoryType::Episodic);
        assert_eq!(memory.source, MemorySource::Conversation);
        assert_eq!(memory.tier, StorageTier::Hot);
        assert_eq!(memory.embedding.len(), 384);
        assert!(memory.weight >= 0.5);
        assert!(memory.weight <= 1.0);
        assert_eq!(memory.conversation_id, Some("meeting-001".to_string()));

        // Verify embedding is non-zero
        let embedding_sum: f32 = memory.embedding.iter().sum();
        assert!(
            embedding_sum.abs() > 0.0,
            "Embedding should not be all zeros"
        );
    }

    #[tokio::test]
    async fn test_ingestion_filters() {
        let (store, _temp_dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        // Empty content
        let empty = pipeline
            .ingest("", MemorySource::Manual, None)
            .await
            .unwrap();
        assert!(empty.is_none(), "Should filter empty content");

        // Whitespace only
        let whitespace = pipeline
            .ingest("   \n\t  ", MemorySource::Manual, None)
            .await
            .unwrap();
        assert!(
            whitespace.is_none(),
            "Should filter whitespace-only content"
        );

        // Too short
        let short = pipeline
            .ingest("Hi", MemorySource::Manual, None)
            .await
            .unwrap();
        assert!(short.is_none(), "Should filter content < 10 chars");

        // Exactly 10 chars should pass
        let boundary = pipeline
            .ingest("1234567890", MemorySource::Manual, None)
            .await
            .unwrap();
        assert!(boundary.is_some(), "Should accept content >= 10 chars");
    }

    #[tokio::test]
    async fn test_compression_levels() {
        let (store, _temp_dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        // Short content -> Full compression
        let short_mem = pipeline
            .ingest("Short memory content here.", MemorySource::Manual, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(short_mem.compression, CompressionLevel::Full);

        // Medium content -> Summary compression
        let medium = "x".repeat(150);
        let medium_mem = pipeline
            .ingest(&medium, MemorySource::Manual, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(medium_mem.compression, CompressionLevel::Summary);

        // Long content -> Keywords compression
        let long = "x".repeat(600);
        let long_mem = pipeline
            .ingest(&long, MemorySource::Manual, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(long_mem.compression, CompressionLevel::Keywords);
    }
}

// =============================================================================
// Capacity Management Tests
// =============================================================================

mod capacity_management_tests {
    use super::*;

    /// Test tier migration (Hot -> Warm -> Cold)
    #[tokio::test]
    async fn test_tier_migration() {
        let (store, _temp_dir) = create_test_store().await;

        let memory = create_test_memory("Memory for tier migration", MemoryType::Semantic);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);

        // Verify initial tier
        assert_eq!(manager.get_tier(id).await.unwrap(), StorageTier::Hot);

        // Demote: Hot -> Warm
        manager.demote(id).await.unwrap();
        assert_eq!(manager.get_tier(id).await.unwrap(), StorageTier::Warm);

        // Demote: Warm -> Cold
        manager.demote(id).await.unwrap();
        assert_eq!(manager.get_tier(id).await.unwrap(), StorageTier::Cold);

        // Promote: Cold -> Warm
        manager.promote(id).await.unwrap();
        assert_eq!(manager.get_tier(id).await.unwrap(), StorageTier::Warm);

        // Promote: Warm -> Hot
        manager.promote(id).await.unwrap();
        assert_eq!(manager.get_tier(id).await.unwrap(), StorageTier::Hot);

        // Verify content is preserved through migrations
        let retrieved = store.get(id).await.unwrap().unwrap();
        assert_eq!(retrieved.content, "Memory for tier migration");
    }

    /// Test compaction based on age
    #[tokio::test]
    async fn test_compaction_by_age() {
        let (store, _temp_dir) = create_test_store().await;

        let old_memory = create_memory_with_age(
            "First sentence here. Second sentence with more content. Third sentence adds detail. Fourth adds extra. Fifth is final.",
            45,
            StorageTier::Warm,
        );
        let id = old_memory.id;
        store.insert(&old_memory).await.unwrap();

        let compactor = Compactor::new(&store);
        let result = compactor.compact(StorageTier::Warm).await.unwrap();

        assert_eq!(result.compacted_count, 1);
        assert!(result.compacted_ids.contains(&id));

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(
            updated.compression,
            CompressionLevel::Summary,
            "Compression level should change to Summary"
        );
    }

    /// Test that high-weight memories are not compacted
    #[tokio::test]
    async fn test_compaction_preserves_high_weight() {
        let (store, _temp_dir) = create_test_store().await;

        // Create high-weight old memory
        let mut memory =
            create_memory_with_age("Important high-weight memory.", 45, StorageTier::Warm);
        memory.weight = 0.9; // Above default 0.7 threshold
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let compactor = Compactor::new(&store);
        let result = compactor.compact(StorageTier::Warm).await.unwrap();

        assert_eq!(result.skipped_high_weight, 1);
        assert!(!result.compacted_ids.contains(&id));

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.compression, CompressionLevel::Full);
    }

    /// Test eviction when capacity threshold is exceeded
    #[tokio::test]
    async fn test_eviction_at_capacity() {
        let (store, _temp_dir) = create_test_store().await;

        // Configure small capacity for testing
        let config = EvictionConfig {
            max_memories_per_tier: 10,
            eviction_threshold: 0.80,
            warning_threshold: 0.70,
            aggressive_threshold: 0.95,
            recent_access_hours: 1,    // Short window
            min_weight_protected: 0.9, // High threshold
        };
        let evictor = Evictor::with_config(&store, config);

        // Insert 9 memories (90% capacity -> eviction needed)
        for i in 0..9 {
            let mut memory = create_test_memory(
                &format!("Evictable memory number {i}"),
                MemoryType::Semantic,
            );
            memory.weight = 0.1 + (i as f32) * 0.05; // Low weights
            memory.last_accessed = Utc::now() - Duration::hours(48); // Old access
            memory.tier = StorageTier::Hot;
            store.insert(&memory).await.unwrap();
        }

        // Check capacity status
        let status = evictor.check_capacity(StorageTier::Hot).await.unwrap();
        assert_eq!(status, CapacityStatus::EvictionNeeded);

        // Trigger eviction
        let evicted = evictor.evict_if_needed(StorageTier::Hot).await.unwrap();
        assert!(!evicted.is_empty(), "Should have evicted some memories");

        // Verify count decreased
        let new_count = store.count_by_tier(StorageTier::Hot).await.unwrap();
        assert!(new_count < 9, "Count should decrease after eviction");
    }

    /// Test that protected memories are not evicted
    #[tokio::test]
    async fn test_eviction_protects_high_weight() {
        let (store, _temp_dir) = create_test_store().await;

        let config = EvictionConfig {
            max_memories_per_tier: 10,
            eviction_threshold: 0.80,
            warning_threshold: 0.70,
            aggressive_threshold: 0.95,
            recent_access_hours: 1,
            min_weight_protected: 0.7,
        };
        let evictor = Evictor::with_config(&store, config);

        // Insert mix of protected and evictable
        let mut protected_ids = Vec::new();

        // High-weight protected memories
        for i in 0..3 {
            let mut memory =
                create_test_memory(&format!("Protected high-weight {i}"), MemoryType::Semantic);
            memory.weight = 0.8;
            memory.last_accessed = Utc::now() - Duration::hours(48);
            memory.tier = StorageTier::Hot;
            protected_ids.push(memory.id);
            store.insert(&memory).await.unwrap();
        }

        // Low-weight evictable memories
        for i in 0..6 {
            let mut memory =
                create_test_memory(&format!("Evictable low-weight {i}"), MemoryType::Semantic);
            memory.weight = 0.2;
            memory.last_accessed = Utc::now() - Duration::hours(48);
            memory.tier = StorageTier::Hot;
            store.insert(&memory).await.unwrap();
        }

        let evicted = evictor.evict_if_needed(StorageTier::Hot).await.unwrap();

        // Verify protected memories were not evicted
        for protected_id in &protected_ids {
            assert!(
                !evicted.contains(protected_id),
                "Protected memory should not be evicted"
            );
        }

        // Verify protected memories still exist
        for protected_id in &protected_ids {
            let retrieved = store.get(*protected_id).await.unwrap();
            assert!(
                retrieved.is_some(),
                "Protected memory should still exist in store"
            );
        }
    }

    /// Test tombstone creation on eviction
    #[tokio::test]
    async fn test_eviction_creates_tombstones() {
        let (store, _temp_dir) = create_test_store().await;

        let config = EvictionConfig {
            max_memories_per_tier: 10,
            eviction_threshold: 0.80,
            warning_threshold: 0.70,
            aggressive_threshold: 0.95,
            recent_access_hours: 1,
            min_weight_protected: 0.9,
        };
        let evictor = Evictor::with_config(&store, config);

        // Insert memories with entities
        for i in 0..9 {
            let mut memory =
                create_test_memory(&format!("Memory about topic-{i}"), MemoryType::Semantic);
            memory.weight = 0.1;
            memory.last_accessed = Utc::now() - Duration::hours(48);
            memory.tier = StorageTier::Hot;
            memory.entities = vec![format!("topic-{}", i), "shared-topic".to_string()];
            store.insert(&memory).await.unwrap();
        }

        // Verify no tombstones before eviction
        let tombstones_before = store.list_all_tombstones().await.unwrap();
        assert!(tombstones_before.is_empty());

        // Trigger eviction
        let evicted = evictor.evict_if_needed(StorageTier::Hot).await.unwrap();
        assert!(!evicted.is_empty());

        // Verify tombstones were created
        let tombstones_after = store.list_all_tombstones().await.unwrap();
        assert_eq!(
            tombstones_after.len(),
            evicted.len(),
            "Should create tombstone for each evicted memory"
        );

        // Verify tombstone content
        for tombstone in &tombstones_after {
            assert!(
                tombstone.topics.contains(&"shared-topic".to_string()),
                "Tombstone should preserve entity topics"
            );
        }
    }
}

// =============================================================================
// Response Capture Filter Tests
// =============================================================================

mod response_capture_tests {
    use super::*;

    /// Test that should_ingest filters error responses
    #[test]
    fn test_should_ingest_filters_errors() {
        assert!(!ResponseCapture::should_ingest(
            "Error: Something went wrong"
        ));
        assert!(!ResponseCapture::should_ingest("error: invalid input"));
        assert!(!ResponseCapture::should_ingest("ERROR: Connection failed"));
    }

    /// Test that should_ingest filters apology patterns
    #[test]
    fn test_should_ingest_filters_apologies() {
        assert!(!ResponseCapture::should_ingest(
            "I'm sorry, I can't help with that."
        ));
        assert!(!ResponseCapture::should_ingest(
            "I apologize, but I cannot provide that information."
        ));
        assert!(!ResponseCapture::should_ingest(
            "I cannot assist with that request."
        ));
        assert!(!ResponseCapture::should_ingest(
            "I can't help you with that."
        ));
    }

    /// Test that should_ingest filters short content
    #[test]
    fn test_should_ingest_filters_short() {
        assert!(!ResponseCapture::should_ingest(""));
        assert!(!ResponseCapture::should_ingest("   "));
        assert!(!ResponseCapture::should_ingest("Hi"));
    }

    /// Test that valid content passes filters
    #[test]
    fn test_should_ingest_accepts_valid() {
        assert!(ResponseCapture::should_ingest(
            "Python is a versatile programming language used for web development, data science, and automation."
        ));
        assert!(ResponseCapture::should_ingest(
            "The capital of France is Paris."
        ));
        assert!(ResponseCapture::should_ingest(
            "Here's how to implement a binary search in Rust."
        ));
    }
}

// =============================================================================
// Token Estimation Tests
// =============================================================================

mod token_estimation_tests {
    use super::*;

    /// Test token estimation approximation
    #[test]
    fn test_token_estimation() {
        // Empty string
        assert_eq!(estimate_tokens(""), 0);

        // ~4 chars per token approximation
        assert_eq!(estimate_tokens("1234"), 1);
        assert_eq!(estimate_tokens("12345678"), 2);
        assert_eq!(estimate_tokens("Hello World!"), 3);
    }

    /// Test truncation with token budget
    #[test]
    fn test_truncate_to_budget() {
        let memories: Vec<RetrievedMemory> = (0..5)
            .map(|i| {
                create_retrieved_memory(
                    &format!("Memory {i} with some content here."),
                    MemoryType::Semantic,
                )
            })
            .collect();

        // Zero budget returns empty
        let zero = truncate_to_budget(&memories, 0);
        assert!(zero.is_empty());

        // Small budget limits results
        let small = truncate_to_budget(&memories, 50);
        assert!(small.len() < memories.len());

        // Large budget keeps all
        let large = truncate_to_budget(&memories, 10000);
        assert_eq!(large.len(), memories.len());
    }
}

// =============================================================================
// Integration: Full Pipeline with Retrieval and Injection
// =============================================================================

mod full_pipeline_tests {
    use super::*;

    #[tokio::test]
    async fn test_complete_memory_pipeline() {
        let (store, temp_dir) = create_test_store().await;
        let store_path = temp_dir.path().to_path_buf();

        let mut ingestion = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let memories_to_ingest = vec![
            "The user prefers Python for data science projects.",
            "User mentioned they use VS Code as their primary editor.",
            "The team meeting about the ML project is scheduled for Friday.",
            "User asked about deploying applications to AWS Lambda.",
            "Previous conversation discussed database optimization strategies.",
        ];

        for content in &memories_to_ingest {
            let result = ingestion
                .ingest(
                    content,
                    MemorySource::Conversation,
                    Some("test-conv".to_string()),
                )
                .await;
            assert!(result.is_ok() && result.unwrap().is_some());
        }

        let store = create_store_at_path(&store_path).await;
        let count = store.count_by_tier(StorageTier::Hot).await.unwrap();
        assert_eq!(count, 5);

        let mut retrieval = RetrievalPipeline::with_defaults(&store, &*SHARED_EMBEDDING_MODEL);

        let results = retrieval
            .retrieve("What editor does the user prefer?", 3)
            .await
            .unwrap();

        assert!(!results.is_empty());

        // Verify VS Code memory is in results
        let vs_code_found = results.iter().any(|r| r.memory.content.contains("VS Code"));
        assert!(vs_code_found, "Should find VS Code memory for editor query");

        // Phase 3: Inject into request
        let mut request = create_chat_request("What tools should I use for development?");
        inject_memories(&mut request, &results, 2000).unwrap();

        // Verify injection
        let system_msg = request["messages"][0]["content"].as_str().unwrap();
        assert!(system_msg.contains("<mnemo-memories>"));
        assert!(system_msg.contains("VS Code"));

        // Phase 4: Simulate response and capture
        let response_content =
            "Based on your preferences, I recommend using VS Code for development.";
        let sse_response = generate_mock_sse_response(response_content);

        // Parse and verify capture would work
        let extracted = StreamingProxy::extract_response_content(&sse_response);
        assert!(extracted.is_complete);
        assert!(ResponseCapture::should_ingest(&extracted.content));
    }
}
