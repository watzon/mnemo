mod capture;
mod error;
mod injection;
mod server;
mod streaming;

pub use capture::ResponseCapture;
pub use error::{
    handle_ingestion_error, handle_network_error, handle_proxy_error, handle_request_error,
    handle_retrieval_error, handle_router_error, handle_upstream_error, with_error_handling,
    PassthroughDecision, ProxyError,
};
pub use injection::{
    estimate_tokens, extract_user_query, format_memory_block, inject_memories, truncate_to_budget,
};
pub use server::ProxyServer;
pub use streaming::{BufferHandle, ExtractedContent, SseEvent, StreamingProxy, TeeResult};
