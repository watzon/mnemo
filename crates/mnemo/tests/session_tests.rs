//! Integration tests for session-scoped memory isolation
//!
//! Tests that memories are properly isolated by session ID while global memories
//! remain visible to all sessions.
//!
//! IMPORTANT: Run with --test-threads=1 due to ML model loading contention.
//! `cargo test -p mnemo --test session_tests -- --test-threads=1`

use mnemo_server::memory::types::{Memory, MemorySource, MemoryType};
use mnemo_server::proxy::{SessionId, SessionIdError};
use mnemo_server::storage::filter::MemoryFilter;
use mnemo_server::storage::LanceStore;
use mnemo_server::testing::MockEmbeddingModel;
use tempfile::tempdir;

// =============================================================================
// Test Fixtures and Helpers
// =============================================================================

/// Test fixture: Create a test store in a temporary directory
async fn create_test_store() -> (LanceStore, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let mut store = LanceStore::connect(dir.path()).await.unwrap();
    store.create_memories_table().await.unwrap();
    (store, dir)
}

/// Create a test memory with specified content and optional session ID
fn create_memory_with_session(content: &str, session_id: Option<&str>) -> Memory {
    let mock_model = MockEmbeddingModel::new();
    let mut memory = Memory::new(
        content.to_string(),
        mock_model.embed(content),
        MemoryType::Semantic,
        MemorySource::Manual,
    );
    memory.conversation_id = session_id.map(|s| s.to_string());
    memory
}

/// Create a test memory with specific embedding and optional session ID
fn create_memory_with_embedding_session(
    content: &str,
    embedding: Vec<f32>,
    session_id: Option<&str>,
) -> Memory {
    let mut memory = Memory::new(
        content.to_string(),
        embedding,
        MemoryType::Semantic,
        MemorySource::Manual,
    );
    memory.conversation_id = session_id.map(|s| s.to_string());
    memory
}

/// Generate a similar embedding with slight variation
fn similar_embedding(base: &[f32], variation: f32) -> Vec<f32> {
    base.iter().map(|v| v + variation).collect()
}

// =============================================================================
// Session Isolation Tests
// =============================================================================

mod session_isolation_tests {
    use super::*;

    #[tokio::test]
    async fn test_session_memory_not_visible_to_other_sessions() {
        let (store, _dir) = create_test_store().await;

        // Create a base embedding for similarity search
        let base_embedding: Vec<f32> = vec![0.5; 384];

        // Create memory with session-a
        let memory_a = create_memory_with_embedding_session(
            "Session A specific memory about Rust programming",
            similar_embedding(&base_embedding, 0.01),
            Some("session-a"),
        );
        store.insert(&memory_a).await.unwrap();

        // Query with session-b filter
        let filter = MemoryFilter::new().with_session_filter(Some("session-b".to_string()));
        let results = store
            .search_filtered(&base_embedding, &filter, 10)
            .await
            .unwrap();

        // Memory from session-a should NOT be visible when querying with session-b
        let found = results.iter().any(|m| m.id == memory_a.id);
        assert!(
            !found,
            "Session A memory should NOT be visible to Session B"
        );
    }

    #[tokio::test]
    async fn test_global_memory_visible_to_all_sessions() {
        let (store, _dir) = create_test_store().await;

        let base_embedding: Vec<f32> = vec![0.5; 384];

        // Create global memory (no session ID)
        let global_memory = create_memory_with_embedding_session(
            "Global memory about programming best practices",
            similar_embedding(&base_embedding, 0.01),
            None, // Global - no session
        );
        store.insert(&global_memory).await.unwrap();

        // Query with session-a filter - should see global memory
        let filter_a = MemoryFilter::new().with_session_filter(Some("session-a".to_string()));
        let results_a = store
            .search_filtered(&base_embedding, &filter_a, 10)
            .await
            .unwrap();

        let found_a = results_a.iter().any(|m| m.id == global_memory.id);
        assert!(
            found_a,
            "Global memory should be visible to Session A"
        );

        // Query with session-b filter - should also see global memory
        let filter_b = MemoryFilter::new().with_session_filter(Some("session-b".to_string()));
        let results_b = store
            .search_filtered(&base_embedding, &filter_b, 10)
            .await
            .unwrap();

        let found_b = results_b.iter().any(|m| m.id == global_memory.id);
        assert!(
            found_b,
            "Global memory should be visible to Session B"
        );
    }

    #[tokio::test]
    async fn test_combined_retrieval_session_and_global() {
        let (store, _dir) = create_test_store().await;

        let base_embedding: Vec<f32> = vec![0.5; 384];

        // Create session-specific memory
        let session_memory = create_memory_with_embedding_session(
            "Session A specific memory about Rust",
            similar_embedding(&base_embedding, 0.01),
            Some("session-a"),
        );
        store.insert(&session_memory).await.unwrap();

        // Create global memory
        let global_memory = create_memory_with_embedding_session(
            "Global memory about programming",
            similar_embedding(&base_embedding, 0.02),
            None,
        );
        store.insert(&global_memory).await.unwrap();

        // Create memory for different session (should not appear)
        let other_session_memory = create_memory_with_embedding_session(
            "Session B specific memory about Python",
            similar_embedding(&base_embedding, 0.03),
            Some("session-b"),
        );
        store.insert(&other_session_memory).await.unwrap();

        // Query with session-a filter
        let filter = MemoryFilter::new().with_session_filter(Some("session-a".to_string()));
        let results = store
            .search_filtered(&base_embedding, &filter, 10)
            .await
            .unwrap();

        // Should find both session-a memory AND global memory
        let found_session = results.iter().any(|m| m.id == session_memory.id);
        let found_global = results.iter().any(|m| m.id == global_memory.id);
        let found_other = results.iter().any(|m| m.id == other_session_memory.id);

        assert!(
            found_session,
            "Should find session-specific memory for session-a"
        );
        assert!(found_global, "Should find global memory");
        assert!(
            !found_other,
            "Should NOT find memory from other session (session-b)"
        );
    }

    #[tokio::test]
    async fn test_no_header_returns_global_only() {
        let (store, _dir) = create_test_store().await;

        let base_embedding: Vec<f32> = vec![0.5; 384];

        // Create global memory
        let global_memory = create_memory_with_embedding_session(
            "Global memory about algorithms",
            similar_embedding(&base_embedding, 0.01),
            None,
        );
        store.insert(&global_memory).await.unwrap();

        // Create session-specific memories
        let session_a_memory = create_memory_with_embedding_session(
            "Session A memory about data structures",
            similar_embedding(&base_embedding, 0.02),
            Some("session-a"),
        );
        store.insert(&session_a_memory).await.unwrap();

        let session_b_memory = create_memory_with_embedding_session(
            "Session B memory about networking",
            similar_embedding(&base_embedding, 0.03),
            Some("session-b"),
        );
        store.insert(&session_b_memory).await.unwrap();

        // Query with session_id = None (global only filter)
        let filter = MemoryFilter::new().with_session_filter(None);
        let results = store
            .search_filtered(&base_embedding, &filter, 10)
            .await
            .unwrap();

        // Should only find global memory
        let found_global = results.iter().any(|m| m.id == global_memory.id);
        let found_session_a = results.iter().any(|m| m.id == session_a_memory.id);
        let found_session_b = results.iter().any(|m| m.id == session_b_memory.id);

        assert!(found_global, "Should find global memory with None filter");
        assert!(
            !found_session_a,
            "Should NOT find session-a memory with None filter"
        );
        assert!(
            !found_session_b,
            "Should NOT find session-b memory with None filter"
        );
    }

    #[tokio::test]
    async fn test_empty_filter_returns_all_memories() {
        let (store, _dir) = create_test_store().await;

        let base_embedding: Vec<f32> = vec![0.5; 384];

        // Create memories across different sessions and global
        let global_memory = create_memory_with_embedding_session(
            "Global memory",
            similar_embedding(&base_embedding, 0.01),
            None,
        );
        store.insert(&global_memory).await.unwrap();

        let session_a_memory = create_memory_with_embedding_session(
            "Session A memory",
            similar_embedding(&base_embedding, 0.02),
            Some("session-a"),
        );
        store.insert(&session_a_memory).await.unwrap();

        let session_b_memory = create_memory_with_embedding_session(
            "Session B memory",
            similar_embedding(&base_embedding, 0.03),
            Some("session-b"),
        );
        store.insert(&session_b_memory).await.unwrap();

        // Query with empty filter (no session filter set)
        let filter = MemoryFilter::new();
        let results = store
            .search_filtered(&base_embedding, &filter, 10)
            .await
            .unwrap();

        // Should find all memories
        assert_eq!(results.len(), 3, "Empty filter should return all memories");

        let found_global = results.iter().any(|m| m.id == global_memory.id);
        let found_session_a = results.iter().any(|m| m.id == session_a_memory.id);
        let found_session_b = results.iter().any(|m| m.id == session_b_memory.id);

        assert!(found_global, "Should find global memory");
        assert!(found_session_a, "Should find session-a memory");
        assert!(found_session_b, "Should find session-b memory");
    }
}

// =============================================================================
// Session Filter SQL Generation Tests
// =============================================================================

mod session_filter_sql_tests {
    use super::*;

    #[test]
    fn test_session_filter_with_session_generates_correct_sql() {
        let filter = MemoryFilter::new().with_session_filter(Some("session-abc".to_string()));

        let sql = filter.to_sql_clause().unwrap();
        assert_eq!(
            sql,
            "(conversation_id = 'session-abc' OR conversation_id IS NULL)"
        );
    }

    #[test]
    fn test_session_filter_global_only_generates_correct_sql() {
        let filter = MemoryFilter::new().with_session_filter(None);

        let sql = filter.to_sql_clause().unwrap();
        assert_eq!(sql, "conversation_id IS NULL");
    }

    #[test]
    fn test_session_filter_combined_with_other_filters() {
        let filter = MemoryFilter::new()
            .with_session_filter(Some("session-xyz".to_string()))
            .with_memory_types(vec![MemoryType::Semantic])
            .with_min_weight(0.7);

        let sql = filter.to_sql_clause().unwrap();

        // Should contain all conditions
        assert!(sql.contains("(conversation_id = 'session-xyz' OR conversation_id IS NULL)"));
        assert!(sql.contains("memory_type = 'Semantic'"));
        assert!(sql.contains("weight >= 0.7"));
        assert!(sql.contains(" AND "));
    }

    #[test]
    fn test_session_filter_not_set_returns_none() {
        let filter = MemoryFilter::new();
        assert!(filter.to_sql_clause().is_none());
    }
}

// =============================================================================
// SessionId Validation Tests
// =============================================================================

mod session_id_validation_tests {
    use super::*;

    #[test]
    fn test_valid_session_ids() {
        // Standard valid cases
        assert!(SessionId::try_from("project-abc").is_ok());
        assert!(SessionId::try_from("PROJECT_123").is_ok());
        assert!(SessionId::try_from("a-b_c").is_ok());
        assert!(SessionId::try_from("a").is_ok()); // Single char
        assert!(SessionId::try_from("123").is_ok()); // Numbers only
        assert!(SessionId::try_from("abc_def-ghi").is_ok()); // Mixed
        assert!(SessionId::try_from("ABC-DEF_GHI").is_ok()); // Uppercase
    }

    #[test]
    fn test_empty_session_id_rejected() {
        let result = SessionId::try_from("");
        assert!(matches!(result, Err(SessionIdError::Empty)));
    }

    #[test]
    fn test_whitespace_only_session_id_rejected() {
        // Whitespace is not allowed (invalid chars)
        let result = SessionId::try_from("   ");
        assert!(matches!(result, Err(SessionIdError::InvalidChars)));
    }

    #[test]
    fn test_session_id_with_spaces_rejected() {
        let result = SessionId::try_from("has spaces");
        assert!(matches!(result, Err(SessionIdError::InvalidChars)));
    }

    #[test]
    fn test_session_id_with_special_chars_rejected() {
        let result = SessionId::try_from("has!special");
        assert!(matches!(result, Err(SessionIdError::InvalidChars)));

        let result = SessionId::try_from("test@email");
        assert!(matches!(result, Err(SessionIdError::InvalidChars)));

        let result = SessionId::try_from("test#hash");
        assert!(matches!(result, Err(SessionIdError::InvalidChars)));

        let result = SessionId::try_from("test$money");
        assert!(matches!(result, Err(SessionIdError::InvalidChars)));

        let result = SessionId::try_from("test%percent");
        assert!(matches!(result, Err(SessionIdError::InvalidChars)));
    }

    #[test]
    fn test_session_id_too_long_rejected() {
        // Create a 129 character string
        let long_id = "a".repeat(129);
        let result = SessionId::try_from(long_id.as_str());
        assert!(matches!(result, Err(SessionIdError::TooLong)));
    }

    #[test]
    fn test_session_id_at_max_length_accepted() {
        // 128 characters should be valid
        let max_id = "a".repeat(128);
        assert!(SessionId::try_from(max_id.as_str()).is_ok());
    }

    #[test]
    fn test_session_id_case_sensitive() {
        // Session IDs are case-sensitive
        let lower = SessionId::try_from("abc").unwrap();
        let upper = SessionId::try_from("ABC").unwrap();
        assert_ne!(lower, upper);
        assert_eq!(lower.as_str(), "abc");
        assert_eq!(upper.as_str(), "ABC");
    }

    #[test]
    fn test_try_from_string() {
        // Test TryFrom<String>
        let s = String::from("valid-id");
        assert!(SessionId::try_from(s).is_ok());

        let s = String::from("has spaces");
        assert!(matches!(
            SessionId::try_from(s),
            Err(SessionIdError::InvalidChars)
        ));
    }

    #[test]
    fn test_session_id_into_string() {
        let session_id = SessionId::try_from("test-id").unwrap();
        let s: String = session_id.into();
        assert_eq!(s, "test-id");
    }

    #[test]
    fn test_session_id_as_str() {
        let session_id = SessionId::try_from("test-id").unwrap();
        assert_eq!(session_id.as_str(), "test-id");
    }

    #[test]
    fn test_session_id_display() {
        let session_id = SessionId::try_from("test-id").unwrap();
        assert_eq!(format!("{}", session_id), "test-id");
    }
}

// =============================================================================
// Edge Case Tests
// =============================================================================

mod edge_case_tests {
    use super::*;

    #[tokio::test]
    async fn test_multiple_memories_same_session() {
        let (store, _dir) = create_test_store().await;

        let base_embedding: Vec<f32> = vec![0.5; 384];

        // Create multiple memories for the same session
        let memory1 = create_memory_with_embedding_session(
            "First session memory about Rust",
            similar_embedding(&base_embedding, 0.01),
            Some("session-a"),
        );
        store.insert(&memory1).await.unwrap();

        let memory2 = create_memory_with_embedding_session(
            "Second session memory about Cargo",
            similar_embedding(&base_embedding, 0.02),
            Some("session-a"),
        );
        store.insert(&memory2).await.unwrap();

        let memory3 = create_memory_with_embedding_session(
            "Third session memory about crates",
            similar_embedding(&base_embedding, 0.03),
            Some("session-a"),
        );
        store.insert(&memory3).await.unwrap();

        // Query with session-a filter
        let filter = MemoryFilter::new().with_session_filter(Some("session-a".to_string()));
        let results = store
            .search_filtered(&base_embedding, &filter, 10)
            .await
            .unwrap();

        // Should find all 3 session memories
        assert_eq!(results.len(), 3, "Should find all memories for session-a");

        let found1 = results.iter().any(|m| m.id == memory1.id);
        let found2 = results.iter().any(|m| m.id == memory2.id);
        let found3 = results.iter().any(|m| m.id == memory3.id);

        assert!(found1, "Should find first session memory");
        assert!(found2, "Should find second session memory");
        assert!(found3, "Should find third session memory");
    }

    #[tokio::test]
    async fn test_session_id_with_hyphens_and_underscores() {
        let (store, _dir) = create_test_store().await;

        let base_embedding: Vec<f32> = vec![0.5; 384];

        // Create memory with complex session ID
        let complex_session_id = "user_123-project-abc-2024";
        let memory = create_memory_with_embedding_session(
            "Memory with complex session ID",
            similar_embedding(&base_embedding, 0.01),
            Some(complex_session_id),
        );
        store.insert(&memory).await.unwrap();

        // Query with the same complex session ID
        let filter =
            MemoryFilter::new().with_session_filter(Some(complex_session_id.to_string()));
        let results = store
            .search_filtered(&base_embedding, &filter, 10)
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, memory.id);
    }

    #[tokio::test]
    async fn test_session_isolation_with_similar_content() {
        let (store, _dir) = create_test_store().await;

        let base_embedding: Vec<f32> = vec![0.5; 384];

        // Create memories with very similar content but different sessions
        let memory_a = create_memory_with_embedding_session(
            "User prefers Rust for systems programming",
            similar_embedding(&base_embedding, 0.01),
            Some("session-a"),
        );
        store.insert(&memory_a).await.unwrap();

        let memory_b = create_memory_with_embedding_session(
            "User prefers Rust for systems programming", // Same content
            similar_embedding(&base_embedding, 0.02),
            Some("session-b"),
        );
        store.insert(&memory_b).await.unwrap();

        // Query with session-a filter
        let filter_a = MemoryFilter::new().with_session_filter(Some("session-a".to_string()));
        let results_a = store
            .search_filtered(&base_embedding, &filter_a, 10)
            .await
            .unwrap();

        // Should find session-a memory but not session-b
        let found_a = results_a.iter().any(|m| m.id == memory_a.id);
        let found_b = results_a.iter().any(|m| m.id == memory_b.id);

        assert!(found_a, "Should find session-a memory");
        assert!(!found_b, "Should NOT find session-b memory when filtering for session-a");
    }

    #[tokio::test]
    async fn test_conversation_id_field_preserved() {
        let (store, _dir) = create_test_store().await;

        // Create memory with session ID
        let session_memory = create_memory_with_session(
            "Test memory with session",
            Some("test-session-123"),
        );
        store.insert(&session_memory).await.unwrap();

        // Retrieve and verify conversation_id is preserved
        let retrieved = store.get(session_memory.id).await.unwrap().unwrap();
        assert_eq!(
            retrieved.conversation_id,
            Some("test-session-123".to_string())
        );

        // Create global memory
        let global_memory = create_memory_with_session("Test global memory", None);
        store.insert(&global_memory).await.unwrap();

        // Retrieve and verify conversation_id is None
        let retrieved_global = store.get(global_memory.id).await.unwrap().unwrap();
        assert_eq!(retrieved_global.conversation_id, None);
    }
}
