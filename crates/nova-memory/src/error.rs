//! Error types for Nova Memory

use thiserror::Error;

/// Main error type for Nova Memory operations
#[derive(Error, Debug)]
pub enum NovaError {
    /// Storage-related errors (LanceDB, file system, etc.)
    #[error("Storage error: {0}")]
    Storage(String),

    /// Embedding generation errors
    #[error("Embedding error: {0}")]
    Embedding(String),

    /// Configuration errors
    #[error("Configuration error: {0}")]
    Config(String),

    /// Router errors (DistilBERT classification)
    #[error("Router error: {0}")]
    Router(String),

    /// Proxy/HTTP errors
    #[error("Proxy error: {0}")]
    Proxy(String),

    /// Memory operation errors
    #[error("Memory error: {0}")]
    Memory(String),

    /// I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization errors
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// General errors
    #[error("{0}")]
    General(String),
}

/// Result type alias for Nova Memory operations
pub type Result<T> = std::result::Result<T, NovaError>;
