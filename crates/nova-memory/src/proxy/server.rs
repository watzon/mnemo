use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::signal;

use crate::config::ProxyConfig;
use crate::error::{NovaError, Result};

pub struct ProxyServer {
    config: ProxyConfig,
}

impl ProxyServer {
    pub fn new(config: ProxyConfig) -> Self {
        Self { config }
    }

    pub async fn serve(&self) -> Result<()> {
        let app = Router::new()
            .route("/health", get(health_check))
            .fallback(proxy_handler);

        let addr: SocketAddr = self
            .config
            .listen_addr
            .parse()
            .map_err(|e| NovaError::Config(format!("Invalid listen address: {}", e)))?;

        tracing::info!("Starting proxy server on {}", addr);

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| NovaError::Proxy(format!("Failed to bind to {}: {}", addr, e)))?;

        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .map_err(|e| NovaError::Proxy(format!("Server error: {}", e)))?;

        tracing::info!("Proxy server shut down gracefully");
        Ok(())
    }
}

async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

async fn proxy_handler() -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "Proxy not implemented yet")
}

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

    #[tokio::test]
    async fn test_health_check() {
        let app = Router::new().route("/health", get(health_check));

        let response = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_fallback_returns_not_implemented() {
        let app = Router::new().fallback(proxy_handler);

        let response = app
            .oneshot(Request::builder().uri("/any/path").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    }
}
