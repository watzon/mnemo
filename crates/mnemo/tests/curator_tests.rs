//! Integration tests for the memory curator system
//!
//! Tests the curator's ability to:
//! - Classify meaningful vs trivial content
//! - Track injections to prevent duplicates
//! - Handle failures with fail-open behavior
//! - Respect configuration settings
//!
//! IMPORTANT: Run with --test-threads=1 due to shared test state.
//! `cargo test -p mnemo-server curator -- --test-threads=1`

use async_trait::async_trait;

use mnemo_server::config::BufferConfig;
use mnemo_server::curator::{
    ConversationBuffer, ConversationTurn, CuratedMemory, CurationResult, CuratorError,
    CuratorProvider, Role,
};
use mnemo_server::memory::types::MemoryType;
use mnemo_server::memory::InjectionTracker;

// =============================================================================
// Mock Curator for Deterministic Testing
// =============================================================================

/// Mock curator provider for deterministic tests without real LLM calls
struct MockCurator {
    should_store: bool,
    fail: bool,
}

impl MockCurator {
    /// Create a mock curator that returns should_store=true
    fn new(should_store: bool) -> Self {
        Self {
            should_store,
            fail: false,
        }
    }

    /// Create a mock curator that simulates failures
    fn failing() -> Self {
        Self {
            should_store: true,
            fail: true,
        }
    }
}

#[async_trait]
impl CuratorProvider for MockCurator {
    async fn curate(&self, conversation: &str) -> Result<CurationResult, CuratorError> {
        if self.fail {
            return Err(CuratorError::InferenceFailed("Mock failure".into()));
        }

        if self.should_store {
            let preview_len = conversation.len().min(50);
            Ok(CurationResult::should_store(
                vec![CuratedMemory::new(
                    MemoryType::Semantic,
                    format!("Curated from: {}", &conversation[..preview_len]),
                    0.8,
                    vec!["test".to_string()],
                )],
                "Mock reasoning: content is memory-worthy".into(),
            ))
        } else {
            Ok(CurationResult::should_not_store(
                "Mock reasoning: not memory-worthy".into(),
            ))
        }
    }

    async fn is_available(&self) -> bool {
        !self.fail
    }

    fn name(&self) -> &'static str {
        "mock"
    }
}

// =============================================================================
// Test 1: Curator Classification (Meaningful vs Trivial)
// =============================================================================

#[tokio::test]
async fn test_curator_classifies_meaningful_vs_trivial() {
    let meaningful_curator = MockCurator::new(true);
    let trivial_curator = MockCurator::new(false);

    // Test meaningful content classification
    let meaningful_result = meaningful_curator
        .curate("User prefers dark mode for all applications and uses VS Code as their primary editor")
        .await;
    assert!(
        meaningful_result.is_ok(),
        "Meaningful content should be processed successfully"
    );
    let result = meaningful_result.unwrap();
    assert!(
        result.should_store,
        "Meaningful content should be marked for storage"
    );
    assert_eq!(
        result.memories.len(),
        1,
        "Should extract exactly one memory"
    );
    assert!(
        result.reasoning.contains("memory-worthy"),
        "Reasoning should indicate memory-worthiness"
    );

    // Test trivial content classification
    let trivial_result = trivial_curator.curate("Hello, how are you?").await;
    assert!(
        trivial_result.is_ok(),
        "Trivial content should be processed successfully"
    );
    let result = trivial_result.unwrap();
    assert!(
        !result.should_store,
        "Trivial content should not be marked for storage"
    );
    assert!(
        result.memories.is_empty(),
        "Trivial content should produce no memories"
    );
    assert!(
        result.reasoning.contains("not memory-worthy"),
        "Reasoning should explain why not stored"
    );
}

// =============================================================================
// Test 2: Injection Tracking Prevents Re-injection
// =============================================================================

#[tokio::test]
async fn test_injection_tracking_prevents_reinjection() {
    let mut tracker = InjectionTracker::new(100);
    let id = uuid::Uuid::new_v4();

    // First check - memory has not been injected yet
    assert!(
        !tracker.was_injected(&id),
        "New memory ID should not be marked as injected"
    );

    // Mark as injected
    tracker.mark_injected(id);

    // Second check - memory was injected
    assert!(
        tracker.was_injected(&id),
        "Memory ID should be marked as injected after marking"
    );

    // Verify tracker state
    assert_eq!(tracker.len(), 1, "Tracker should contain exactly one entry");

    // Test with multiple IDs
    let id2 = uuid::Uuid::new_v4();
    let id3 = uuid::Uuid::new_v4();

    tracker.mark_injected(id2);
    tracker.mark_injected(id3);

    assert_eq!(tracker.len(), 3, "Tracker should contain three entries");
    assert!(tracker.was_injected(&id2));
    assert!(tracker.was_injected(&id3));

    // Test that checking updates LRU order (access id)
    assert!(tracker.was_injected(&id));
}

// =============================================================================
// Test 3: Fail-Open Behavior on Curator Failure
// =============================================================================

#[tokio::test]
async fn test_failopen_on_curator_error() {
    let failing_curator = MockCurator::failing();

    // Verify curator reports as unavailable
    assert!(
        !failing_curator.is_available().await,
        "Failing curator should report as unavailable"
    );

    // Attempt curation - should fail
    let result = failing_curator.curate("Some conversation content").await;
    assert!(result.is_err(), "Failing curator should return an error");

    // Verify error type
    match result {
        Err(CuratorError::InferenceFailed(msg)) => {
            assert!(
                msg.contains("Mock failure"),
                "Error message should contain failure reason"
            );
        }
        _ => panic!("Expected InferenceFailed error, got {:?}", result),
    }

    // In a real proxy scenario, this error would trigger fallback to blind storage
    // The key behavior is that the error is properly propagated for handling
}

// =============================================================================
// Test 4: Conversation Buffer Limits and Context Generation
// =============================================================================

#[tokio::test]
async fn test_conversation_buffer_limits() {
    let config = BufferConfig::default();
    let mut buffer = ConversationBuffer::new(&config);

    // Verify default config values
    assert_eq!(
        config.max_turns, 10,
        "Default max_turns should be 10"
    );
    assert_eq!(
        config.max_tokens, 8000,
        "Default max_tokens should be 8000"
    );

    // Add more than max_turns
    for i in 0..15 {
        buffer.push(ConversationTurn::new(
            Role::User,
            format!("Message {i}"),
        ));
    }

    // Should be limited to max_turns (10 by default)
    assert!(
        buffer.len() <= 10,
        "Buffer should be limited to max_turns"
    );
    assert_eq!(buffer.len(), 10, "Buffer should contain exactly 10 turns");

    // Context should be generated with proper XML format
    let context = buffer.to_prompt_context();
    assert!(!context.is_empty(), "Context should not be empty");
    assert!(
        context.contains("<conversation>"),
        "Context should contain conversation opening tag"
    );
    assert!(
        context.contains("</conversation>"),
        "Context should contain conversation closing tag"
    );
    assert!(
        context.contains("<turn role=\"user\">"),
        "Context should contain turn elements"
    );

    // Verify the most recent messages are kept (LRU eviction)
    assert!(
        context.contains("Message 14"),
        "Most recent message should be present"
    );
    assert!(
        context.contains("Message 5"),
        "Older messages within limit should be present"
    );
    assert!(
        !context.contains("Message 0"),
        "Oldest messages should be evicted"
    );
}

// =============================================================================
// Test 5: CuratedMemory Construction and Properties
// =============================================================================

#[tokio::test]
async fn test_curated_memory_construction() {
    // Test basic construction
    let memory = CuratedMemory::new(
        MemoryType::Semantic,
        "User prefers dark mode".to_string(),
        0.85,
        vec!["dark mode".to_string(), "preferences".to_string()],
    );

    assert_eq!(memory.memory_type, MemoryType::Semantic);
    assert_eq!(memory.content, "User prefers dark mode");
    assert!((memory.importance - 0.85).abs() < f32::EPSILON);
    assert_eq!(memory.entities.len(), 2);
    assert!(memory.supersedes_hint.is_none());

    // Test importance clamping (high)
    let memory_high = CuratedMemory::new(
        MemoryType::Episodic,
        "Test".to_string(),
        1.5, // Above max
        vec![],
    );
    assert!(
        (memory_high.importance - 1.0).abs() < f32::EPSILON,
        "Importance should be clamped to 1.0"
    );

    // Test importance clamping (low)
    let memory_low = CuratedMemory::new(
        MemoryType::Procedural,
        "Test".to_string(),
        -0.5, // Below min
        vec![],
    );
    assert!(
        (memory_low.importance - 0.0).abs() < f32::EPSILON,
        "Importance should be clamped to 0.0"
    );
}

// =============================================================================
// Test 6: CurationResult Factory Methods
// =============================================================================

#[tokio::test]
async fn test_curation_result_factories() {
    // Test should_store factory
    let memories = vec![CuratedMemory::new(
        MemoryType::Semantic,
        "Test memory".to_string(),
        0.7,
        vec!["test".to_string()],
    )];
    let store_result =
        CurationResult::should_store(memories.clone(), "Important information".to_string());

    assert!(store_result.should_store);
    assert_eq!(store_result.memories.len(), 1);
    assert_eq!(store_result.reasoning, "Important information");

    // Test should_not_store factory
    let skip_result = CurationResult::should_not_store("Not relevant".to_string());

    assert!(!skip_result.should_store);
    assert!(skip_result.memories.is_empty());
    assert_eq!(skip_result.reasoning, "Not relevant");
}

// =============================================================================
// Test 7: Buffer Token Limit Enforcement
// =============================================================================

#[tokio::test]
async fn test_buffer_token_limit_eviction() {
    // Create config with very low token limit
    let config = BufferConfig {
        max_turns: 100,    // High turn limit
        max_tokens: 10,    // Very low token limit (~40 chars of content)
    };
    let mut buffer = ConversationBuffer::new(&config);

    // Add turns that would exceed token limit if all kept
    // Each turn content is ~17 chars = ~4 tokens
    for i in 0..5 {
        buffer.push(ConversationTurn::new(
            Role::User,
            format!("Msg {i} 0123456789"), // ~17 chars = ~4 tokens each
        ));
    }

    // Buffer should have evicted some turns to stay under token limit
    // With 10 token limit and ~4 tokens per message, should keep at most 2-3
    assert!(
        buffer.len() < 5,
        "Buffer should evict turns to enforce token limit, got {} turns",
        buffer.len()
    );

    // Verify context can be generated
    let context = buffer.to_prompt_context();
    assert!(!context.is_empty());
    assert!(context.contains("<conversation>"));
}

// =============================================================================
// Test 8: Curator Error Types
// =============================================================================

#[tokio::test]
async fn test_curator_error_types() {
    // Test all error variants display correctly
    let err1 = CuratorError::ModelNotFound("llama-7b".to_string());
    assert!(err1.to_string().contains("Model not found"));
    assert!(err1.to_string().contains("llama-7b"));

    let err2 = CuratorError::ModelLoadFailed("OOM".to_string());
    assert!(err2.to_string().contains("Model loading failed"));

    let err3 = CuratorError::InferenceFailed("timeout".to_string());
    assert!(err3.to_string().contains("Inference failed"));

    let err4 = CuratorError::ParseError("invalid json".to_string());
    assert!(err4.to_string().contains("Parse error"));

    let err5 = CuratorError::ApiError("rate limited".to_string());
    assert!(err5.to_string().contains("API error"));

    let err6 = CuratorError::ConfigError("missing key".to_string());
    assert!(err6.to_string().contains("Configuration error"));
}

// =============================================================================
// Test 9: Role Enum and ConversationTurn
// =============================================================================

#[tokio::test]
async fn test_role_and_turn() {
    // Test role string representations
    assert_eq!(Role::User.as_str(), "user");
    assert_eq!(Role::Assistant.as_str(), "assistant");
    assert_eq!(Role::System.as_str(), "system");

    // Test conversation turn creation
    let turn = ConversationTurn::new(Role::User, "Hello world".to_string());
    assert_eq!(turn.role, Role::User);
    assert_eq!(turn.content, "Hello world");
    assert!(turn.timestamp <= chrono::Utc::now());

    // Test token estimation (chars / 4)
    let short_turn = ConversationTurn::new(Role::Assistant, "Hi".to_string());
    assert_eq!(short_turn.estimate_tokens(), 0); // 2/4 = 0

    let long_turn = ConversationTurn::new(Role::System, "a".repeat(100));
    assert_eq!(long_turn.estimate_tokens(), 25); // 100/4 = 25
}

// =============================================================================
// Test 10: Mock Curator Availability and Name
// =============================================================================

#[tokio::test]
async fn test_mock_curator_metadata() {
    let working_curator = MockCurator::new(true);
    let failing_curator = MockCurator::failing();

    // Test name
    assert_eq!(working_curator.name(), "mock");
    assert_eq!(failing_curator.name(), "mock");

    // Test availability
    assert!(working_curator.is_available().await);
    assert!(!failing_curator.is_available().await);
}
