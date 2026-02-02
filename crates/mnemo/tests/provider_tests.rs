//! Integration tests for LLM provider detection
//!
//! Tests for Provider enum detection logic based on URL, headers, and body structure.

use axum::http::HeaderMap;
use serde_json::json;
use url::Url;

use mnemo_server::proxy::Provider;

// =============================================================================
// URL-based Detection Tests
// =============================================================================

#[test]
fn test_detect_openai_from_url() {
    let url = Url::parse("https://api.openai.com/v1/chat/completions").unwrap();
    let headers = HeaderMap::new();
    let body = json!({"model": "gpt-4", "messages": []});

    let provider = Provider::detect(&url, &headers, &body);
    assert_eq!(provider, Provider::OpenAI);
}

#[test]
fn test_detect_anthropic_from_url() {
    let url = Url::parse("https://api.anthropic.com/v1/messages").unwrap();
    let headers = HeaderMap::new();
    let body = json!({"model": "claude-3", "messages": []});

    let provider = Provider::detect(&url, &headers, &body);
    assert_eq!(provider, Provider::Anthropic);
}

#[test]
fn test_detect_openai_from_subdomain() {
    let url = Url::parse("https://beta.api.openai.com/v1/chat/completions").unwrap();
    let headers = HeaderMap::new();
    let body = json!({});

    let provider = Provider::detect(&url, &headers, &body);
    assert_eq!(provider, Provider::OpenAI);
}

#[test]
fn test_detect_anthropic_from_subdomain() {
    let url = Url::parse("https://beta.api.anthropic.com/v1/messages").unwrap();
    let headers = HeaderMap::new();
    let body = json!({});

    let provider = Provider::detect(&url, &headers, &body);
    assert_eq!(provider, Provider::Anthropic);
}

// =============================================================================
// Body Structure Detection Tests
// =============================================================================

#[test]
fn test_detect_anthropic_from_body_structure() {
    // Unknown URL but body has top-level "system" field (Anthropic pattern)
    let url = Url::parse("https://unknown-api.example.com/v1/chat").unwrap();
    let headers = HeaderMap::new();
    let body = json!({
        "model": "some-model",
        "system": "You are a helpful assistant",
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let provider = Provider::detect(&url, &headers, &body);
    assert_eq!(provider, Provider::Anthropic);
}

#[test]
fn test_detect_openai_from_body_structure() {
    // Unknown URL but body has messages[0].role == "system" (OpenAI pattern)
    let url = Url::parse("https://unknown-api.example.com/v1/chat").unwrap();
    let headers = HeaderMap::new();
    let body = json!({
        "model": "some-model",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant"},
            {"role": "user", "content": "Hello"}
        ]
    });

    let provider = Provider::detect(&url, &headers, &body);
    assert_eq!(provider, Provider::OpenAI);
}

#[test]
fn test_detect_anthropic_from_max_tokens_required() {
    // Anthropic requires max_tokens, OpenAI doesn't
    let url = Url::parse("https://unknown-api.example.com/v1/chat").unwrap();
    let headers = HeaderMap::new();
    let body = json!({
        "model": "some-model",
        "max_tokens": 100,
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let provider = Provider::detect(&url, &headers, &body);
    assert_eq!(provider, Provider::Anthropic);
}

// =============================================================================
// Header-based Detection Tests
// =============================================================================

#[test]
fn test_detect_anthropic_from_header_hint() {
    // Unknown URL but has x-api-key header (Anthropic pattern)
    let url = Url::parse("https://unknown-api.example.com/v1/chat").unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("x-api-key", "sk-ant-xxx".parse().unwrap());
    let body = json!({"messages": []});

    let provider = Provider::detect(&url, &headers, &body);
    assert_eq!(provider, Provider::Anthropic);
}

#[test]
fn test_openai_from_authorization_header() {
    // Authorization header with Bearer token (common for OpenAI)
    let url = Url::parse("https://unknown-api.example.com/v1/chat").unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer sk-xxx".parse().unwrap());
    let body = json!({"messages": []});

    let provider = Provider::detect(&url, &headers, &body);
    assert_eq!(provider, Provider::OpenAI);
}

// =============================================================================
// Unknown Provider Fallback Tests
// =============================================================================

#[test]
fn test_unknown_provider_fallback() {
    // Unknown URL + can't determine from body or headers
    let url = Url::parse("https://unknown-api.example.com/v1/chat").unwrap();
    let headers = HeaderMap::new();
    let body = json!({
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let provider = Provider::detect(&url, &headers, &body);
    assert_eq!(provider, Provider::Unknown);
}

#[test]
fn test_unknown_provider_empty_body() {
    let url = Url::parse("https://unknown-api.example.com/v1/chat").unwrap();
    let headers = HeaderMap::new();
    let body = json!({});

    let provider = Provider::detect(&url, &headers, &body);
    assert_eq!(provider, Provider::Unknown);
}

// =============================================================================
// Priority Tests (URL takes precedence)
// =============================================================================

#[test]
fn test_url_takes_precedence_over_body() {
    // URL says OpenAI, but body structure looks like Anthropic
    let url = Url::parse("https://api.openai.com/v1/chat/completions").unwrap();
    let headers = HeaderMap::new();
    let body = json!({
        "system": "You are helpful",  // Anthropic-style
        "messages": []
    });

    let provider = Provider::detect(&url, &headers, &body);
    assert_eq!(provider, Provider::OpenAI);
}

#[test]
fn test_url_takes_precedence_over_headers() {
    // URL says Anthropic, but has OpenAI-style authorization header
    let url = Url::parse("https://api.anthropic.com/v1/messages").unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer sk-xxx".parse().unwrap());
    let body = json!({"messages": []});

    let provider = Provider::detect(&url, &headers, &body);
    assert_eq!(provider, Provider::Anthropic);
}

// =============================================================================
// Edge Cases
// =============================================================================

#[test]
fn test_case_insensitive_url_matching() {
    let url = Url::parse("https://API.OPENAI.COM/v1/chat/completions").unwrap();
    let headers = HeaderMap::new();
    let body = json!({"messages": []});

    let provider = Provider::detect(&url, &headers, &body);
    assert_eq!(provider, Provider::OpenAI);
}

#[test]
fn test_localhost_url_unknown() {
    let url = Url::parse("http://localhost:8080/v1/chat").unwrap();
    let headers = HeaderMap::new();
    let body = json!({"messages": []});

    let provider = Provider::detect(&url, &headers, &body);
    assert_eq!(provider, Provider::Unknown);
}

#[test]
fn test_anthropic_messages_format() {
    // Anthropic uses "message" role in content blocks
    let url = Url::parse("https://unknown-api.example.com/v1/chat").unwrap();
    let headers = HeaderMap::new();
    let body = json!({
        "messages": [
            {
                "role": "user",
                "content": [{"type": "text", "text": "Hello"}]
            }
        ]
    });

    let provider = Provider::detect(&url, &headers, &body);
    assert_eq!(provider, Provider::Anthropic);
}
