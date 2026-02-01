//! HTTP Proxy Server with Dynamic URL Passthrough
//!
//! Implements a transparent proxy that supports:
//! - Dynamic passthrough via `/p/{url}` routes
//! - Configured upstream fallback for standard requests
//! - Fail-open error handling strategy

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, RawQuery, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    response::Response,
    routing::{any, get},
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::Mutex as TokioMutex;
use url::Url;

use crate::config::{ProxyConfig, RouterConfig};
use crate::embedding::EmbeddingModel;
use crate::error::{MnemoError, Result};
use crate::memory::retrieval::RetrievalPipeline;
use crate::router::MemoryRouter;
use crate::storage::LanceStore;
use serde_json::Value;

use super::passthrough::UpstreamTarget;
use super::provider::Provider;
use super::providers::{AnthropicProvider, LLMProvider, OpenAiProvider};

/// Hop-by-hop headers that should not be forwarded to upstream
const HOP_BY_HOP_HEADERS: &[&str] = &[
    "host",
    "connection",
    "keep-alive",
    "transfer-encoding",
    "proxy-connection",
    "te",
    "upgrade",
];

/// Shared application state for all handlers
#[derive(Clone)]
pub struct AppState {
    /// Proxy configuration
    pub config: ProxyConfig,
    /// HTTP client for upstream requests
    pub client: reqwest::Client,
    /// Storage backend for memories
    pub store: Arc<TokioMutex<LanceStore>>,
    /// Embedding model for vector generation
    pub embedding_model: Arc<EmbeddingModel>,
    /// Memory router for query analysis and routing
    pub router: Arc<MemoryRouter>,
    /// Router configuration
    pub router_config: RouterConfig,
}

/// The main proxy server
pub struct ProxyServer {
    config: ProxyConfig,
    store: Arc<TokioMutex<LanceStore>>,
    embedding_model: Arc<EmbeddingModel>,
    router: Arc<MemoryRouter>,
    router_config: RouterConfig,
}

impl ProxyServer {
    /// Create a new proxy server with the given configuration and components
    pub fn new(
        config: ProxyConfig,
        store: Arc<TokioMutex<LanceStore>>,
        embedding_model: Arc<EmbeddingModel>,
        router: Arc<MemoryRouter>,
        router_config: RouterConfig,
    ) -> Self {
        Self {
            config,
            store,
            embedding_model,
            router,
            router_config,
        }
    }

    /// Start the proxy server and listen for requests
    pub async fn serve(&self) -> Result<()> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(self.config.timeout_secs))
            .build()
            .map_err(|e| MnemoError::Proxy(format!("Failed to create HTTP client: {e}")))?;

        let app_state = Arc::new(AppState {
            config: self.config.clone(),
            client,
            store: self.store.clone(),
            embedding_model: self.embedding_model.clone(),
            router: self.router.clone(),
            router_config: self.router_config.clone(),
        });

        let app = create_router(app_state);

        let addr: SocketAddr = self
            .config
            .listen_addr
            .parse()
            .map_err(|e| MnemoError::Config(format!("Invalid listen address: {e}")))?;

        tracing::info!("Starting proxy server on {addr}");
        tracing::info!("Dynamic passthrough enabled via /p/{{url}}");
        if self.config.allowed_hosts.is_empty() {
            tracing::info!("Host allowlist: disabled (all hosts allowed)");
        } else {
            tracing::info!(
                "Host allowlist: {} hosts configured",
                self.config.allowed_hosts.len()
            );
        }
        if let Some(ref upstream) = self.config.upstream_url {
            tracing::info!("Configured upstream URL: {upstream}");
        } else {
            tracing::info!("No configured upstream URL (dynamic passthrough only)");
        }

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| MnemoError::Proxy(format!("Failed to bind to {addr}: {e}")))?;

        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .map_err(|e| MnemoError::Proxy(format!("Server error: {e}")))?;

        tracing::info!("Proxy server shut down gracefully");
        Ok(())
    }
}

/// Create the router with all routes configured
pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/p/{*upstream_url}", any(dynamic_proxy_handler))
        .fallback(configured_proxy_handler)
        .with_state(state)
}

/// Health check endpoint - returns JSON status
async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

/// Handle dynamic passthrough requests via /p/{url}
///
/// Extracts the target URL from the path, validates it against the allowlist,
/// and forwards the request to the upstream server.
async fn dynamic_proxy_handler(
    State(state): State<Arc<AppState>>,
    Path(upstream_url): Path<String>,
    RawQuery(query): RawQuery,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response<Body> {
    let path = format!("/p/{upstream_url}");

    let target = match UpstreamTarget::from_path(&path, query.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("Invalid passthrough URL: {e}");
            return create_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_url",
                &format!("Invalid upstream URL: {e}"),
            );
        }
    };

    if !target.is_allowed(&state.config) {
        tracing::warn!("Blocked request to disallowed host: {}", target.host);
        return create_error_response(
            StatusCode::FORBIDDEN,
            "host_not_allowed",
            &format!("Host '{}' is not in the allowlist", target.host),
        );
    }

    tracing::debug!("Proxying dynamic request to: {}", target.url);

    match forward_request(&state, &target.url, method, headers, body).await {
        Ok(response) => response,
        Err(e) => e.into_response(),
    }
}

/// Handle requests to the configured upstream URL (fallback handler)
///
/// Used when a request doesn't match `/p/*` - forwards to the configured
/// upstream URL if one exists.
async fn configured_proxy_handler(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
) -> Response<Body> {
    let upstream_base = match &state.config.upstream_url {
        Some(url) => url,
        None => {
            return create_error_response(
                StatusCode::NOT_FOUND,
                "no_upstream_configured",
                "No upstream URL configured. Use /p/{url} for dynamic passthrough or configure an upstream_url.",
            );
        }
    };

    let base_url = match Url::parse(upstream_base) {
        Ok(url) => url,
        Err(e) => {
            tracing::error!("Invalid configured upstream URL: {e}");
            return create_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid_upstream_config",
                "The configured upstream URL is invalid",
            );
        }
    };

    let path_and_query = request
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

    let target_url = match base_url.join(path_and_query) {
        Ok(url) => url,
        Err(e) => {
            tracing::error!("Failed to construct target URL: {e}");
            return create_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_path",
                &format!("Invalid request path: {e}"),
            );
        }
    };

    let method = request.method().clone();
    let headers = request.headers().clone();
    let body = request.into_body();

    tracing::debug!("Proxying configured request to: {target_url}");

    match forward_request(&state, &target_url, method, headers, body).await {
        Ok(response) => response,
        Err(e) => e.into_response(),
    }
}

/// Forward a request to the upstream server
///
/// This is the shared request forwarding logic used by both dynamic and
/// configured proxy handlers.
async fn forward_request(
    state: &AppState,
    target_url: &Url,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> std::result::Result<Response<Body>, super::ProxyError> {
    let mut forwarded_headers = HeaderMap::new();
    for (name, value) in headers.iter() {
        let name_str = name.as_str().to_lowercase();
        if !HOP_BY_HOP_HEADERS.contains(&name_str.as_str()) {
            forwarded_headers.insert(name.clone(), value.clone());
        }
    }

    if let Some(host) = target_url.host_str() {
        let host_value = if let Some(port) = target_url.port() {
            format!("{host}:{port}")
        } else {
            host.to_string()
        };
        if let Ok(header_value) = HeaderValue::from_str(&host_value) {
            forwarded_headers.insert("host", header_value);
        }
    }

    let body_bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .map_err(|e| super::ProxyError::Request(format!("Failed to read request body: {e}")))?;

    let final_body = match try_inject_memories(state, target_url, &headers, &body_bytes).await {
        Ok(modified) => modified,
        Err(e) => {
            tracing::debug!("Memory injection skipped: {e}");
            body_bytes.to_vec()
        }
    };

    let reqwest_method = match method.as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "DELETE" => reqwest::Method::DELETE,
        "PATCH" => reqwest::Method::PATCH,
        "HEAD" => reqwest::Method::HEAD,
        "OPTIONS" => reqwest::Method::OPTIONS,
        other => reqwest::Method::from_bytes(other.as_bytes())
            .map_err(|_| super::ProxyError::Request(format!("Invalid HTTP method: {other}")))?,
    };

    let mut reqwest_headers = reqwest::header::HeaderMap::new();
    for (name, value) in forwarded_headers.iter() {
        if let Ok(reqwest_name) = reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes())
        {
            if let Ok(reqwest_value) = reqwest::header::HeaderValue::from_bytes(value.as_bytes()) {
                reqwest_headers.insert(reqwest_name, reqwest_value);
            }
        }
    }

    let response = state
        .client
        .request(reqwest_method, target_url.clone())
        .headers(reqwest_headers)
        .body(final_body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                super::ProxyError::Network(format!("Request timed out: {e}"))
            } else if e.is_connect() {
                super::ProxyError::Network(format!("Failed to connect to upstream: {e}"))
            } else {
                super::ProxyError::Network(format!("Request failed: {e}"))
            }
        })?;

    let status = StatusCode::from_u16(response.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    let mut response_headers = HeaderMap::new();
    for (name, value) in response.headers().iter() {
        let name_str = name.as_str().to_lowercase();
        if !HOP_BY_HOP_HEADERS.contains(&name_str.as_str()) {
            if let Ok(axum_name) = axum::http::header::HeaderName::from_bytes(name.as_str().as_bytes()) {
                if let Ok(axum_value) = axum::http::header::HeaderValue::from_bytes(value.as_bytes()) {
                    response_headers.insert(axum_name, axum_value);
                }
            }
        }
    }

    let response_body = response
        .bytes()
        .await
        .map_err(|e| super::ProxyError::Network(format!("Failed to read response body: {e}")))?;

    if let Some(content) = try_capture_response(&body_bytes, target_url, &headers, &response_body) {
        tracing::debug!(
            "Captured response content ({} chars): {}...",
            content.len(),
            &content[..content.len().min(100)]
        );
    }

    let mut builder = Response::builder().status(status);
    for (name, value) in response_headers.iter() {
        builder = builder.header(name, value);
    }

    builder
        .body(Body::from(response_body))
        .map_err(|e| super::ProxyError::Network(format!("Failed to build response: {e}")))
}

fn try_capture_response(
    request_body: &[u8],
    target_url: &Url,
    headers: &HeaderMap,
    response_body: &[u8],
) -> Option<String> {
    let request_json: Value = serde_json::from_slice(request_body).ok()?;
    let provider = Provider::detect(target_url, headers, &request_json);

    let llm_provider: Box<dyn LLMProvider + Send + Sync> = match provider {
        Provider::OpenAI => Box::new(OpenAiProvider::new()),
        Provider::Anthropic => Box::new(AnthropicProvider::new()),
        Provider::Unknown => return None,
    };

    if let Ok(response_json) = serde_json::from_slice::<Value>(response_body) {
        if let Some(content) = llm_provider.parse_response_content(&response_json) {
            if !content.trim().is_empty() {
                return Some(content);
            }
        }
    }

    if let Ok(response_str) = std::str::from_utf8(response_body) {
        let extracted = llm_provider.parse_sse_content(response_str);
        if !extracted.content.trim().is_empty() {
            return Some(extracted.content);
        }
    }

    None
}

async fn try_inject_memories(
    state: &AppState,
    target_url: &Url,
    headers: &HeaderMap,
    body_bytes: &[u8],
) -> crate::error::Result<Vec<u8>> {
    let mut body_json: Value = serde_json::from_slice(body_bytes)
        .map_err(|e| crate::error::MnemoError::Proxy(format!("Invalid JSON: {e}")))?;

    let provider = Provider::detect(target_url, headers, &body_json);

    let llm_provider: Box<dyn LLMProvider + Send + Sync> = match provider {
        Provider::OpenAI => Box::new(OpenAiProvider::new()),
        Provider::Anthropic => Box::new(AnthropicProvider::new()),
        Provider::Unknown => {
            return Ok(body_bytes.to_vec());
        }
    };

    let query = match llm_provider.extract_user_query(&body_json) {
        Some(query) => query,
        None => return Ok(body_bytes.to_vec()),
    };

    let store = state.store.lock().await;
    let mut pipeline =
        RetrievalPipeline::with_defaults(&store, &state.embedding_model);
    let memories = pipeline
        .retrieve(&query, state.router_config.max_memories)
        .await?;
    drop(store);

    llm_provider.inject_memories(
        &mut body_json,
        &memories,
        state.config.max_injection_tokens,
    )?;

    let modified = serde_json::to_vec(&body_json)
        .map_err(|e| crate::error::MnemoError::Proxy(format!("Failed to serialize: {e}")))?;

    Ok(modified)
}

/// Create a JSON error response
fn create_error_response(status: StatusCode, error_type: &str, message: &str) -> Response<Body> {
    let body = serde_json::json!({
        "error": {
            "type": error_type,
            "message": message,
        }
    });

    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap()
        })
}

/// Wait for shutdown signal (Ctrl+C or SIGTERM)
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C, initiating graceful shutdown");
        },
        _ = terminate => {
            tracing::info!("Received SIGTERM, initiating graceful shutdown");
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn create_test_state_with_allowed_hosts(
        allowed_hosts: Vec<String>,
    ) -> Arc<AppState> {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
        store.create_memories_table().await.unwrap();
        std::mem::forget(temp_dir);

        Arc::new(AppState {
            config: ProxyConfig {
                listen_addr: "127.0.0.1:9999".to_string(),
                upstream_url: None,
                allowed_hosts,
                timeout_secs: 30,
                max_injection_tokens: 2000,
            },
            client: reqwest::Client::new(),
            store: Arc::new(TokioMutex::new(store)),
            embedding_model: Arc::new(EmbeddingModel::new().unwrap()),
            router: Arc::new(MemoryRouter::new().unwrap()),
            router_config: RouterConfig::default(),
        })
    }

    async fn create_test_state() -> Arc<AppState> {
        create_test_state_with_allowed_hosts(Vec::new()).await
    }

    #[tokio::test]
    async fn test_health_check() {
        let state = create_test_state().await;
        let app = create_router(state);

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
        
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("\"status\":\"ok\""));
    }

    #[tokio::test]
    async fn test_fallback_without_upstream_returns_not_found() {
        let state = create_test_state().await;
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/any/path")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("no_upstream_configured"));
    }

    #[tokio::test]
    async fn test_dynamic_passthrough_invalid_url() {
        let state = create_test_state().await;
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/p/not-a-valid-url")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("invalid_url"));
    }

    #[tokio::test]
    async fn test_dynamic_passthrough_blocked_host() {
        let state =
            create_test_state_with_allowed_hosts(vec!["api.openai.com".to_string()]).await;
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/p/https://evil.com/api")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_str.contains("host_not_allowed"));
    }

    #[test]
    fn test_hop_by_hop_headers_defined() {
        assert!(HOP_BY_HOP_HEADERS.contains(&"host"));
        assert!(HOP_BY_HOP_HEADERS.contains(&"connection"));
        assert!(HOP_BY_HOP_HEADERS.contains(&"keep-alive"));
        assert!(HOP_BY_HOP_HEADERS.contains(&"transfer-encoding"));
        assert!(HOP_BY_HOP_HEADERS.contains(&"proxy-connection"));
        assert!(HOP_BY_HOP_HEADERS.contains(&"te"));
        assert!(HOP_BY_HOP_HEADERS.contains(&"upgrade"));
    }
}
