//! Fail-Open Error Handling for Proxy Operations
//!
//! This module implements a fail-open strategy for the memory proxy:
//! - Router errors: Skip memory injection, pass through
//! - Retrieval errors: Skip memory injection, pass through
//! - Ingestion errors: Log and ignore (fire-and-forget)
//! - Upstream errors: Return upstream error to client
//!
//! The goal is to ensure the proxy never blocks LLM requests due to
//! memory system failures.

use axum::{
    body::Body,
    http::{Response, StatusCode},
    response::IntoResponse,
};

use thiserror::Error;
use tracing::{error, warn};

use crate::error::NovaError;

/// Errors that can occur during proxy operations
#[derive(Error, Debug, Clone)]
pub enum ProxyError {
    /// Router classification failed
    #[error("Router error: {0}")]
    Router(String),

    /// Memory retrieval failed
    #[error("Retrieval error: {0}")]
    Retrieval(String),

    /// Memory ingestion failed (fire-and-forget)
    #[error("Ingestion error: {0}")]
    Ingestion(String),

    /// Upstream LLM API returned an error
    #[error("Upstream error: {status}")]
    Upstream { status: StatusCode, body: String },

    /// Request parsing or validation failed
    #[error("Request error: {0}")]
    Request(String),

    /// Network-level error (connection, timeout, etc.)
    #[error("Network error: {0}")]
    Network(String),
}

impl ProxyError {
    /// Convert to an HTTP response for client errors
    pub fn into_response(self) -> Response<Body> {
        match self {
            ProxyError::Upstream { status, body } => {
                // Pass through upstream errors as-is
                Response::builder()
                    .status(status)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap_or_else(|_| {
                        Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(Body::empty())
                            .unwrap()
                    })
            }
            _ => {
                // For other errors, return 502 Bad Gateway
                // This indicates the proxy failed, not the upstream
                let error_body = serde_json::json!({
                    "error": {
                        "message": self.to_string(),
                        "type": "proxy_error",
                    }
                });

                Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .header("content-type", "application/json")
                    .body(Body::from(error_body.to_string()))
                    .unwrap_or_else(|_| {
                        Response::builder()
                            .status(StatusCode::BAD_GATEWAY)
                            .body(Body::empty())
                            .unwrap()
                    })
            }
        }
    }

    /// Get the error category for metrics/logging
    pub fn category(&self) -> &'static str {
        match self {
            ProxyError::Router(_) => "router",
            ProxyError::Retrieval(_) => "retrieval",
            ProxyError::Ingestion(_) => "ingestion",
            ProxyError::Upstream { .. } => "upstream",
            ProxyError::Request(_) => "request",
            ProxyError::Network(_) => "network",
        }
    }
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response<Body> {
        self.into_response()
    }
}

/// Decision for how to handle a request after an error
#[derive(Debug)]
pub enum PassthroughDecision {
    /// Continue with memory features enabled
    WithMemory,
    /// Skip memory features, just proxy the request
    SkipMemory,
    /// Return an error response to the client
    ReturnError(Response<Body>),
}

impl PartialEq for PassthroughDecision {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::WithMemory, Self::WithMemory)
                | (Self::SkipMemory, Self::SkipMemory)
                | (Self::ReturnError(_), Self::ReturnError(_))
        )
    }
}

/// Handle router classification errors
///
/// Strategy: Log the error and skip memory injection, allowing the
/// request to pass through without memory features.
pub fn handle_router_error(error: &NovaError) -> PassthroughDecision {
    error!(
        error_type = "router",
        error_message = %error,
        "Router classification failed, skipping memory injection"
    );

    PassthroughDecision::SkipMemory
}

/// Handle memory retrieval errors
///
/// Strategy: Log the error and skip memory injection, allowing the
/// request to pass through without memory features.
pub fn handle_retrieval_error(error: &NovaError) -> PassthroughDecision {
    error!(
        error_type = "retrieval",
        error_message = %error,
        "Memory retrieval failed, skipping memory injection"
    );

    PassthroughDecision::SkipMemory
}

/// Handle memory ingestion errors
///
/// Strategy: Log the error and continue. Ingestion is fire-and-forget,
/// so we don't want to fail the response that's already been sent.
pub fn handle_ingestion_error(error: &NovaError) {
    // Use warn level since ingestion is best-effort
    warn!(
        error_type = "ingestion",
        error_message = %error,
        "Memory ingestion failed (fire-and-forget), continuing"
    );

    // No return value - ingestion errors are silently ignored
}

/// Handle upstream LLM API errors
///
/// Strategy: Return the upstream error to the client. The proxy's job
/// is to transparently pass through upstream responses, including errors.
pub fn handle_upstream_error(status: StatusCode, body: String) -> PassthroughDecision {
    error!(
        error_type = "upstream",
        status = %status,
        body_length = body.len(),
        "Upstream returned error, passing through to client"
    );

    let response = Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap()
        });

    PassthroughDecision::ReturnError(response)
}

/// Handle request parsing/validation errors
///
/// Strategy: Return a 400 Bad Request to the client since the request
/// is malformed and cannot be processed.
pub fn handle_request_error(error: &NovaError) -> PassthroughDecision {
    error!(
        error_type = "request",
        error_message = %error,
        "Request validation failed"
    );

    let error_body = serde_json::json!({
        "error": {
            "message": format!("Invalid request: {}", error),
            "type": "invalid_request_error",
        }
    });

    let response = Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .header("content-type", "application/json")
        .body(Body::from(error_body.to_string()))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::empty())
                .unwrap()
        });

    PassthroughDecision::ReturnError(response)
}

/// Handle network-level errors
///
/// Strategy: Return a 502 Bad Gateway to indicate the proxy couldn't
/// reach the upstream service.
pub fn handle_network_error(error: &NovaError) -> PassthroughDecision {
    error!(
        error_type = "network",
        error_message = %error,
        "Network error connecting to upstream"
    );

    let error_body = serde_json::json!({
        "error": {
            "message": "Failed to connect to upstream service",
            "type": "proxy_error",
        }
    });

    let response = Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .header("content-type", "application/json")
        .body(Body::from(error_body.to_string()))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::empty())
                .unwrap()
        });

    PassthroughDecision::ReturnError(response)
}

/// Main error handler that routes to specific handlers based on error type
///
/// This is the primary entry point for handling proxy errors with the
/// fail-open strategy.
pub fn handle_proxy_error(error: ProxyError) -> PassthroughDecision {
    match error {
        ProxyError::Router(_) => {
            // Convert to NovaError for handler
            let nova_err = NovaError::Router(error.to_string());
            handle_router_error(&nova_err)
        }
        ProxyError::Retrieval(_) => {
            let nova_err = NovaError::Memory(error.to_string());
            handle_retrieval_error(&nova_err)
        }
        ProxyError::Ingestion(_) => {
            let nova_err = NovaError::Memory(error.to_string());
            handle_ingestion_error(&nova_err);
            // Ingestion errors don't affect the response
            PassthroughDecision::WithMemory
        }
        ProxyError::Upstream { status, body } => handle_upstream_error(status, body),
        ProxyError::Request(_) => {
            let nova_err = NovaError::Proxy(error.to_string());
            handle_request_error(&nova_err)
        }
        ProxyError::Network(_) => {
            let nova_err = NovaError::Proxy(error.to_string());
            handle_network_error(&nova_err)
        }
    }
}

/// Wrap a memory operation with error handling
///
/// This helper function wraps any fallible operation and converts
/// errors into appropriate passthrough decisions.
pub fn with_error_handling<T, F>(
    operation: F,
    error_converter: impl FnOnce(NovaError) -> ProxyError,
) -> Result<T, ProxyError>
where
    F: FnOnce() -> Result<T, NovaError>,
{
    operation().map_err(error_converter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[test]
    fn test_proxy_error_display() {
        let err = ProxyError::Router("classification failed".to_string());
        assert_eq!(err.to_string(), "Router error: classification failed");

        let err = ProxyError::Retrieval("database timeout".to_string());
        assert_eq!(err.to_string(), "Retrieval error: database timeout");

        let err = ProxyError::Upstream {
            status: StatusCode::SERVICE_UNAVAILABLE,
            body: "upstream down".to_string(),
        };
        assert_eq!(err.to_string(), "Upstream error: 503 Service Unavailable");
    }

    #[test]
    fn test_proxy_error_categories() {
        assert_eq!(ProxyError::Router("test".to_string()).category(), "router");
        assert_eq!(
            ProxyError::Retrieval("test".to_string()).category(),
            "retrieval"
        );
        assert_eq!(
            ProxyError::Ingestion("test".to_string()).category(),
            "ingestion"
        );
        assert_eq!(
            ProxyError::Upstream {
                status: StatusCode::OK,
                body: "".to_string()
            }
            .category(),
            "upstream"
        );
        assert_eq!(
            ProxyError::Request("test".to_string()).category(),
            "request"
        );
        assert_eq!(
            ProxyError::Network("timeout".to_string()).category(),
            "network"
        );
    }

    #[tokio::test]
    async fn test_handle_router_error_returns_skip_memory() {
        let error = NovaError::Router("classification failed".to_string());
        let decision = handle_router_error(&error);

        assert_eq!(decision, PassthroughDecision::SkipMemory);
    }

    #[tokio::test]
    async fn test_handle_retrieval_error_returns_skip_memory() {
        let error = NovaError::Memory("database timeout".to_string());
        let decision = handle_retrieval_error(&error);

        assert_eq!(decision, PassthroughDecision::SkipMemory);
    }

    #[test]
    fn test_handle_ingestion_error_does_not_panic() {
        let error = NovaError::Memory("insert failed".to_string());
        // Should not panic and should not return anything
        handle_ingestion_error(&error);
    }

    #[tokio::test]
    async fn test_handle_upstream_error_returns_error_response() {
        let status = StatusCode::TOO_MANY_REQUESTS;
        let body = r#"{"error": "rate limited"}"#.to_string();

        let decision = handle_upstream_error(status, body.clone());

        match decision {
            PassthroughDecision::ReturnError(response) => {
                assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

                let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
                let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
                assert!(body_str.contains("rate limited"));
            }
            _ => panic!("Expected ReturnError decision"),
        }
    }

    #[tokio::test]
    async fn test_handle_request_error_returns_bad_request() {
        let error = NovaError::Proxy("missing field".to_string());
        let decision = handle_request_error(&error);

        match decision {
            PassthroughDecision::ReturnError(response) => {
                assert_eq!(response.status(), StatusCode::BAD_REQUEST);

                let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
                let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
                assert!(body_str.contains("Invalid request"));
            }
            _ => panic!("Expected ReturnError decision"),
        }
    }

    #[tokio::test]
    async fn test_handle_network_error_returns_bad_gateway() {
        let error = NovaError::Proxy("connection timeout".to_string());
        let decision = handle_network_error(&error);

        match decision {
            PassthroughDecision::ReturnError(response) => {
                assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

                let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
                let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
                assert!(body_str.contains("upstream"));
            }
            _ => panic!("Expected ReturnError decision"),
        }
    }

    #[test]
    fn test_handle_proxy_error_routes_correctly() {
        // Router error -> SkipMemory
        let decision = handle_proxy_error(ProxyError::Router("fail".to_string()));
        assert!(matches!(decision, PassthroughDecision::SkipMemory));

        // Retrieval error -> SkipMemory
        let decision = handle_proxy_error(ProxyError::Retrieval("fail".to_string()));
        assert!(matches!(decision, PassthroughDecision::SkipMemory));

        // Ingestion error -> WithMemory (fire and forget)
        let decision = handle_proxy_error(ProxyError::Ingestion("fail".to_string()));
        assert!(matches!(decision, PassthroughDecision::WithMemory));

        // Request error -> ReturnError
        let decision = handle_proxy_error(ProxyError::Request("fail".to_string()));
        assert!(matches!(decision, PassthroughDecision::ReturnError(_)));

        // Network error -> ReturnError
        let decision = handle_proxy_error(ProxyError::Network("fail".to_string()));
        assert!(matches!(decision, PassthroughDecision::ReturnError(_)));
    }

    #[tokio::test]
    async fn test_proxy_error_into_response_upstream() {
        let error = ProxyError::Upstream {
            status: StatusCode::UNAUTHORIZED,
            body: r#"{"error": "invalid key"}"#.to_string(),
        };

        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("invalid key"));
    }

    #[tokio::test]
    async fn test_proxy_error_into_response_other() {
        let error = ProxyError::Router("classification failed".to_string());

        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

        let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("proxy_error"));
    }

    #[test]
    fn test_passthrough_decision_equality() {
        assert_eq!(
            PassthroughDecision::WithMemory,
            PassthroughDecision::WithMemory
        );
        assert_eq!(
            PassthroughDecision::SkipMemory,
            PassthroughDecision::SkipMemory
        );
        assert_ne!(
            PassthroughDecision::WithMemory,
            PassthroughDecision::SkipMemory
        );
    }

    #[test]
    fn test_with_error_handling_success() {
        let result = with_error_handling(
            || Ok::<_, NovaError>(42),
            |e| ProxyError::Router(e.to_string()),
        );

        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_with_error_handling_failure() {
        let result = with_error_handling(
            || Err::<i32, _>(NovaError::Router("fail".to_string())),
            |e| ProxyError::Router(e.to_string()),
        );

        assert!(matches!(result, Err(ProxyError::Router(_))));
    }
}
