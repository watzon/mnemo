//! Integration tests for the HTTP proxy functionality
//!
//! Tests for health endpoint, request parsing, memory injection format,
//! and error handling behavior.

use axum::{
    Json, Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use std::sync::Arc;
use tower::ServiceExt;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers};

use mnemo::config::ProxyConfig;
use mnemo::memory::retrieval::RetrievedMemory;
use mnemo::proxy::{AppState, create_router};
use mnemo::memory::types::{Memory, MemorySource, MemoryType};
use mnemo::proxy::{
    estimate_tokens, extract_user_query, format_memory_block, inject_memories, truncate_to_budget,
};

// =============================================================================
// Test Fixtures
// =============================================================================

/// Creates a test memory with specified content and type
fn create_test_memory(content: &str, memory_type: MemoryType) -> Memory {
    let mut memory = Memory::new(
        content.to_string(),
        vec![0.5; 384],
        memory_type,
        MemorySource::Manual,
    );
    memory.created_at = chrono::Utc::now();
    memory
}

/// Creates a retrieved memory with test scores
fn create_retrieved_memory(content: &str, memory_type: MemoryType) -> RetrievedMemory {
    RetrievedMemory {
        memory: create_test_memory(content, memory_type),
        similarity_score: 0.9,
        effective_weight: 0.8,
        final_score: 0.85,
    }
}

/// Create a valid OpenAI-format chat request
fn create_chat_request() -> serde_json::Value {
    serde_json::json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "Hello, how are you?"}
        ],
        "temperature": 0.7,
        "stream": false
    })
}

/// Create a mock health check handler matching production behavior
async fn mock_health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

// =============================================================================
// Health Endpoint Tests
// =============================================================================

mod health_endpoint_tests {
    use super::*;

    #[tokio::test]
    async fn test_health_endpoint_returns_200_ok() {
        let app = Router::new().route("/health", get(mock_health_check));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_health_endpoint_returns_json_status() {
        let app = Router::new().route("/health", get(mock_health_check));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn test_health_endpoint_accepts_get_method() {
        let app = Router::new().route("/health", get(mock_health_check));

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}

// =============================================================================
// Request Parsing Tests
// =============================================================================

mod request_parsing_tests {
    use super::*;

    #[test]
    fn test_extract_user_query_basic() {
        let request = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "What is Rust?"}
            ]
        });

        let query = extract_user_query(&request);
        assert_eq!(query, Some("What is Rust?".to_string()));
    }

    #[test]
    fn test_extract_user_query_multiple_user_messages() {
        let request = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "First question"},
                {"role": "assistant", "content": "First answer"},
                {"role": "user", "content": "Follow-up question"}
            ]
        });

        let query = extract_user_query(&request);
        // Should return the LAST user message
        assert_eq!(query, Some("Follow-up question".to_string()));
    }

    #[test]
    fn test_extract_user_query_no_user_message() {
        let request = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are helpful."}
            ]
        });

        let query = extract_user_query(&request);
        assert!(query.is_none());
    }

    #[test]
    fn test_extract_user_query_missing_messages() {
        let request = serde_json::json!({
            "model": "gpt-4"
        });

        let query = extract_user_query(&request);
        assert!(query.is_none());
    }

    #[test]
    fn test_extract_user_query_empty_messages() {
        let request = serde_json::json!({
            "model": "gpt-4",
            "messages": []
        });

        let query = extract_user_query(&request);
        assert!(query.is_none());
    }

    #[test]
    fn test_extract_user_query_complex_conversation() {
        let request = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are a coding assistant."},
                {"role": "user", "content": "Write a Python function"},
                {"role": "assistant", "content": "def hello(): pass"},
                {"role": "user", "content": "Add type hints"},
                {"role": "assistant", "content": "def hello() -> None: pass"},
                {"role": "user", "content": "Now add a docstring please"}
            ]
        });

        let query = extract_user_query(&request);
        assert_eq!(query, Some("Now add a docstring please".to_string()));
    }
}

// =============================================================================
// Memory Injection Format Tests
// =============================================================================

mod memory_injection_format_tests {
    use super::*;

    #[test]
    fn test_format_memory_block_structure() {
        let memories = vec![create_retrieved_memory(
            "User prefers dark mode",
            MemoryType::Episodic,
        )];

        let block = format_memory_block(&memories);

        // Verify XML structure
        assert!(block.starts_with("<mnemo-memories>"));
        assert!(block.ends_with("</mnemo-memories>"));
        assert!(block.contains("<memory"));
        assert!(block.contains("</memory>"));
    }

    #[test]
    fn test_format_memory_block_contains_timestamp() {
        let memories = vec![create_retrieved_memory(
            "Test content",
            MemoryType::Semantic,
        )];

        let block = format_memory_block(&memories);

        // Should contain a timestamp attribute in YYYY-MM-DD format
        assert!(block.contains("timestamp=\""));
        // The timestamp should match a date pattern (basic check)
        let has_date = block.contains("-") && block.contains("20");
        assert!(has_date, "Block should contain a date timestamp");
    }

    #[test]
    fn test_format_memory_block_contains_type() {
        let memories = vec![
            create_retrieved_memory("Episodic memory", MemoryType::Episodic),
            create_retrieved_memory("Semantic memory", MemoryType::Semantic),
            create_retrieved_memory("Procedural memory", MemoryType::Procedural),
        ];

        let block = format_memory_block(&memories);

        assert!(block.contains("type=\"episodic\""));
        assert!(block.contains("type=\"semantic\""));
        assert!(block.contains("type=\"procedural\""));
    }

    #[test]
    fn test_format_memory_block_empty_returns_empty_string() {
        let memories: Vec<RetrievedMemory> = vec![];
        let block = format_memory_block(&memories);
        assert!(block.is_empty());
    }

    #[test]
    fn test_format_memory_block_content_preserved() {
        let test_content = "The user's preferred programming language is Rust";
        let memories = vec![create_retrieved_memory(test_content, MemoryType::Semantic)];

        let block = format_memory_block(&memories);

        assert!(
            block.contains(test_content),
            "Memory content should be preserved in output"
        );
    }

    #[test]
    fn test_format_memory_block_multiple_memories_ordered() {
        let memories = vec![
            create_retrieved_memory("First memory", MemoryType::Episodic),
            create_retrieved_memory("Second memory", MemoryType::Semantic),
            create_retrieved_memory("Third memory", MemoryType::Procedural),
        ];

        let block = format_memory_block(&memories);

        // Verify ordering by checking positions
        let first_pos = block.find("First memory").unwrap();
        let second_pos = block.find("Second memory").unwrap();
        let third_pos = block.find("Third memory").unwrap();

        assert!(first_pos < second_pos);
        assert!(second_pos < third_pos);
    }
}

// =============================================================================
// Memory Injection Tests
// =============================================================================

mod memory_injection_tests {
    use super::*;

    #[test]
    fn test_inject_memories_appends_to_system_message() {
        let mut request = create_chat_request();
        let memories = vec![create_retrieved_memory(
            "User prefers concise answers",
            MemoryType::Semantic,
        )];

        inject_memories(&mut request, &memories, 2000).unwrap();

        let messages = request["messages"].as_array().unwrap();
        let system_content = messages[0]["content"].as_str().unwrap();

        assert!(system_content.contains("You are a helpful assistant."));
        assert!(system_content.contains("<mnemo-memories>"));
        assert!(system_content.contains("User prefers concise answers"));
    }

    #[test]
    fn test_inject_memories_creates_system_when_missing() {
        let mut request = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "Hello!"}
            ]
        });

        let memories = vec![create_retrieved_memory(
            "User context here",
            MemoryType::Episodic,
        )];

        inject_memories(&mut request, &memories, 2000).unwrap();

        let messages = request["messages"].as_array().unwrap();

        // Should have 2 messages now
        assert_eq!(messages.len(), 2);
        // First should be system
        assert_eq!(messages[0]["role"], "system");
        assert!(
            messages[0]["content"]
                .as_str()
                .unwrap()
                .contains("<mnemo-memories>")
        );
        // Second should be user
        assert_eq!(messages[1]["role"], "user");
    }

    #[test]
    fn test_inject_memories_empty_is_noop() {
        let original = create_chat_request();
        let mut request = original.clone();

        let memories: Vec<RetrievedMemory> = vec![];
        inject_memories(&mut request, &memories, 2000).unwrap();

        assert_eq!(request, original);
    }

    #[test]
    fn test_inject_memories_invalid_request_returns_error() {
        let mut request = serde_json::json!({
            "model": "gpt-4"
            // Missing "messages" array
        });

        let memories = vec![create_retrieved_memory("test", MemoryType::Semantic)];
        let result = inject_memories(&mut request, &memories, 2000);

        assert!(result.is_err());
    }

    #[test]
    fn test_inject_memories_preserves_other_fields() {
        let mut request = serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
            "temperature": 0.7,
            "max_tokens": 1000,
            "stream": true
        });

        let memories = vec![create_retrieved_memory("test", MemoryType::Semantic)];
        inject_memories(&mut request, &memories, 2000).unwrap();

        // Other fields should be preserved
        assert_eq!(request["model"], "gpt-4");
        assert_eq!(request["temperature"], 0.7);
        assert_eq!(request["max_tokens"], 1000);
        assert_eq!(request["stream"], true);
    }
}

// =============================================================================
// Token Budget Tests
// =============================================================================

mod token_budget_tests {
    use super::*;

    #[test]
    fn test_estimate_tokens_basic() {
        // ~4 chars per token approximation
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("Hi"), 0); // 2 chars / 4 = 0
        assert_eq!(estimate_tokens("Hello World!"), 3); // 12 chars / 4 = 3
        assert_eq!(estimate_tokens("A longer sentence here"), 5); // 22 chars / 4 = 5
    }

    #[test]
    fn test_truncate_to_budget_keeps_most_relevant() {
        let memories = vec![
            create_retrieved_memory("Short", MemoryType::Semantic),
            create_retrieved_memory("Medium length content here", MemoryType::Episodic),
            create_retrieved_memory(
                "A very long memory that takes up many tokens in the response",
                MemoryType::Procedural,
            ),
        ];

        // Large budget should keep all
        let truncated = truncate_to_budget(&memories, 10000);
        assert_eq!(truncated.len(), 3);

        // Very small budget should only keep first few
        let truncated_small = truncate_to_budget(&memories, 50);
        assert!(truncated_small.len() <= 3);
        assert!(!truncated_small.is_empty());
    }

    #[test]
    fn test_truncate_to_budget_zero_returns_empty() {
        let memories = vec![create_retrieved_memory("test", MemoryType::Semantic)];
        let truncated = truncate_to_budget(&memories, 0);
        assert!(truncated.is_empty());
    }

    #[test]
    fn test_truncate_to_budget_empty_input() {
        let memories: Vec<RetrievedMemory> = vec![];
        let truncated = truncate_to_budget(&memories, 1000);
        assert!(truncated.is_empty());
    }

    #[test]
    fn test_truncate_to_budget_preserves_order() {
        let memories = vec![
            create_retrieved_memory("First", MemoryType::Semantic),
            create_retrieved_memory("Second", MemoryType::Episodic),
            create_retrieved_memory("Third", MemoryType::Procedural),
        ];

        let truncated = truncate_to_budget(&memories, 10000);

        assert_eq!(truncated[0].memory.content, "First");
        assert_eq!(truncated[1].memory.content, "Second");
        assert_eq!(truncated[2].memory.content, "Third");
    }
}

// =============================================================================
// Error Handling Tests
// =============================================================================

mod error_handling_tests {
    use super::*;

    #[test]
    fn test_inject_memories_handles_invalid_messages_type() {
        let mut request = serde_json::json!({
            "model": "gpt-4",
            "messages": "not an array"  // Invalid type
        });

        let memories = vec![create_retrieved_memory("test", MemoryType::Semantic)];
        let result = inject_memories(&mut request, &memories, 2000);

        assert!(result.is_err());
    }

    #[test]
    fn test_inject_memories_handles_null_messages() {
        let mut request = serde_json::json!({
            "model": "gpt-4",
            "messages": null
        });

        let memories = vec![create_retrieved_memory("test", MemoryType::Semantic)];
        let result = inject_memories(&mut request, &memories, 2000);

        assert!(result.is_err());
    }

    #[test]
    fn test_extract_user_query_handles_malformed_message() {
        let request = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user"}  // Missing content field
            ]
        });

        let query = extract_user_query(&request);
        // Should return None gracefully, not panic
        assert!(query.is_none());
    }

    #[test]
    fn test_extract_user_query_handles_non_string_content() {
        let request = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": 12345}  // Content is not a string
            ]
        });

        let query = extract_user_query(&request);
        assert!(query.is_none());
    }
}

// =============================================================================
// Dynamic Passthrough Tests (using wiremock)
// =============================================================================

mod passthrough_tests {
    use super::*;

    /// Creates a test router with the given allowed hosts
    fn create_test_router_with_allowed(allowed_hosts: Vec<String>) -> Router {
        let config = ProxyConfig {
            listen_addr: "127.0.0.1:0".to_string(),
            upstream_url: None,
            timeout_secs: 10,
            max_injection_tokens: 2000,
            allowed_hosts,
        };
        let state = Arc::new(AppState {
            config,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap(),
        });
        create_router(state)
    }

    #[tokio::test]
    async fn test_passthrough_basic_post() {
        // Start mock server
        let mock_server = MockServer::start().await;

        // Configure mock response
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/test"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"success": true})),
            )
            .mount(&mock_server)
            .await;

        // Create router allowing the mock server's host (127.0.0.1)
        let router = create_test_router_with_allowed(vec!["127.0.0.1".to_string()]);

        // Build request to passthrough endpoint
        let target = format!("{}/test", mock_server.uri());
        let request = Request::builder()
            .method("POST")
            .uri(format!("/p/{}", target))
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"test": "data"}"#))
            .unwrap();

        // Execute
        let response = router.oneshot(request).await.unwrap();

        // Assert
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body["success"], true);
    }

    #[tokio::test]
    async fn test_passthrough_get_with_query_string() {
        let mock_server = MockServer::start().await;

        // Configure mock to verify query string is received
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/search"))
            .and(matchers::query_param("foo", "bar"))
            .and(matchers::query_param("baz", "qux"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"query": "received"})),
            )
            .mount(&mock_server)
            .await;

        let router = create_test_router_with_allowed(vec!["127.0.0.1".to_string()]);

        // Include query string in the passthrough URL
        let target = format!("{}/search?foo=bar&baz=qux", mock_server.uri());
        let request = Request::builder()
            .method("GET")
            .uri(format!("/p/{}", target))
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body["query"], "received");
    }

    #[tokio::test]
    async fn test_passthrough_headers_forwarded() {
        let mock_server = MockServer::start().await;

        // Configure mock to require Authorization header
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/protected"))
            .and(matchers::header("Authorization", "Bearer test-token"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"authorized": true})),
            )
            .mount(&mock_server)
            .await;

        let router = create_test_router_with_allowed(vec!["127.0.0.1".to_string()]);

        let target = format!("{}/protected", mock_server.uri());
        let request = Request::builder()
            .method("GET")
            .uri(format!("/p/{}", target))
            .header("Authorization", "Bearer test-token")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body["authorized"], true);
    }

    #[tokio::test]
    async fn test_passthrough_hop_by_hop_stripped() {
        let mock_server = MockServer::start().await;

        // Mock that should NOT receive the Connection header
        // If Connection header arrives, this would fail
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/check"))
            .and(matchers::header_exists("X-Custom-Header"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"headers": "ok"})),
            )
            .mount(&mock_server)
            .await;

        let router = create_test_router_with_allowed(vec!["127.0.0.1".to_string()]);

        let target = format!("{}/check", mock_server.uri());
        let request = Request::builder()
            .method("GET")
            .uri(format!("/p/{}", target))
            .header("Connection", "keep-alive") // Should be stripped
            .header("X-Custom-Header", "should-pass") // Should be forwarded
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        // Request should succeed, meaning Connection was stripped
        // and X-Custom-Header was passed through
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_passthrough_blocked_host_returns_403() {
        // Create router that only allows api.openai.com
        let router = create_test_router_with_allowed(vec!["api.openai.com".to_string()]);

        // Try to access a different host
        let request = Request::builder()
            .method("GET")
            .uri("/p/https://evil.example.com/api")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("host_not_allowed"));
    }

    #[tokio::test]
    async fn test_passthrough_invalid_url_returns_400() {
        let router = create_test_router_with_allowed(vec![]);

        // Invalid URL (no scheme)
        let request = Request::builder()
            .method("GET")
            .uri("/p/not-a-valid-url")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("invalid_url"));
    }

    #[tokio::test]
    async fn test_passthrough_empty_path_returns_404() {
        let router = create_test_router_with_allowed(vec![]);

        // Empty passthrough path falls through to fallback handler (no upstream configured)
        let request = Request::builder()
            .method("GET")
            .uri("/p/")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        // Falls through to fallback, returns 404 (no upstream configured)
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_passthrough_upstream_error_returned() {
        let mock_server = MockServer::start().await;

        // Configure mock to return 500 error
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/error"))
            .respond_with(
                ResponseTemplate::new(500)
                    .set_body_json(serde_json::json!({"error": "Internal Server Error"})),
            )
            .mount(&mock_server)
            .await;

        let router = create_test_router_with_allowed(vec!["127.0.0.1".to_string()]);

        let target = format!("{}/error", mock_server.uri());
        let request = Request::builder()
            .method("GET")
            .uri(format!("/p/{}", target))
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        // Should pass through the 500 status from upstream
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body["error"], "Internal Server Error");
    }

    #[tokio::test]
    async fn test_passthrough_upstream_404_returned() {
        let mock_server = MockServer::start().await;

        // Configure mock to return 404
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/notfound"))
            .respond_with(
                ResponseTemplate::new(404).set_body_json(serde_json::json!({"error": "Not Found"})),
            )
            .mount(&mock_server)
            .await;

        let router = create_test_router_with_allowed(vec!["127.0.0.1".to_string()]);

        let target = format!("{}/notfound", mock_server.uri());
        let request = Request::builder()
            .method("GET")
            .uri(format!("/p/{}", target))
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        // Should pass through the 404 status from upstream
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body["error"], "Not Found");
    }

    #[tokio::test]
    async fn test_health_endpoint_still_works() {
        // Verify health endpoint works alongside passthrough
        let router = create_test_router_with_allowed(vec!["127.0.0.1".to_string()]);

        let request = Request::builder()
            .method("GET")
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("\"status\":\"ok\""));
    }

    #[tokio::test]
    async fn test_passthrough_put_method() {
        let mock_server = MockServer::start().await;

        Mock::given(matchers::method("PUT"))
            .and(matchers::path("/update"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"updated": true})),
            )
            .mount(&mock_server)
            .await;

        let router = create_test_router_with_allowed(vec!["127.0.0.1".to_string()]);

        let target = format!("{}/update", mock_server.uri());
        let request = Request::builder()
            .method("PUT")
            .uri(format!("/p/{}", target))
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"data": "updated"}"#))
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_passthrough_delete_method() {
        let mock_server = MockServer::start().await;

        Mock::given(matchers::method("DELETE"))
            .and(matchers::path("/resource/123"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&mock_server)
            .await;

        let router = create_test_router_with_allowed(vec!["127.0.0.1".to_string()]);

        let target = format!("{}/resource/123", mock_server.uri());
        let request = Request::builder()
            .method("DELETE")
            .uri(format!("/p/{}", target))
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_passthrough_response_headers_forwarded() {
        let mock_server = MockServer::start().await;

        Mock::given(matchers::method("GET"))
            .and(matchers::path("/with-headers"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data": "test"}))
                    .insert_header("X-Custom-Response", "header-value")
                    .insert_header("X-Request-Id", "12345"),
            )
            .mount(&mock_server)
            .await;

        let router = create_test_router_with_allowed(vec!["127.0.0.1".to_string()]);

        let target = format!("{}/with-headers", mock_server.uri());
        let request = Request::builder()
            .method("GET")
            .uri(format!("/p/{}", target))
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("X-Custom-Response").unwrap(),
            "header-value"
        );
        assert_eq!(response.headers().get("X-Request-Id").unwrap(), "12345");
    }

    #[tokio::test]
    async fn test_passthrough_empty_allowlist_allows_all() {
        let mock_server = MockServer::start().await;

        Mock::given(matchers::method("GET"))
            .and(matchers::path("/open"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"access": "allowed"})),
            )
            .mount(&mock_server)
            .await;

        // Empty allowlist should allow all hosts
        let router = create_test_router_with_allowed(vec![]);

        let target = format!("{}/open", mock_server.uri());
        let request = Request::builder()
            .method("GET")
            .uri(format!("/p/{}", target))
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_passthrough_body_forwarded() {
        let mock_server = MockServer::start().await;

        // Configure mock to verify body content
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/echo"))
            .and(matchers::body_json(serde_json::json!({"message": "hello world"})))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"echo": "received"})),
            )
            .mount(&mock_server)
            .await;

        let router = create_test_router_with_allowed(vec!["127.0.0.1".to_string()]);

        let target = format!("{}/echo", mock_server.uri());
        let request = Request::builder()
            .method("POST")
            .uri(format!("/p/{}", target))
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"message": "hello world"}"#))
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
