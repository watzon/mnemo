//! LLM Provider abstraction for multi-provider support

mod anthropic;
mod openai;

pub use anthropic::{AnthropicProvider, parse_anthropic_sse};
pub use openai::OpenAiProvider;

use crate::error::Result;
use crate::memory::retrieval::RetrievedMemory;
use crate::proxy::streaming::ExtractedContent;
use serde_json::Value;

/// Trait for LLM provider-specific operations
///
/// Implementations handle the differences between providers like OpenAI and Anthropic
/// in terms of request format, response parsing, and memory injection.
pub trait LLMProvider {
    /// Inject memories into the request body
    ///
    /// Modifies the request body in-place to include the memory context.
    /// For OpenAI: appends to system message in messages array
    /// For Anthropic: appends to top-level system field
    fn inject_memories(
        &self,
        request_body: &mut Value,
        memories: &[RetrievedMemory],
        max_tokens: usize,
    ) -> Result<()>;

    /// Extract user query from request for memory retrieval
    ///
    /// Returns the content of the last user message, which is used
    /// to search for relevant memories.
    fn extract_user_query(&self, request_body: &Value) -> Option<String>;

    /// Parse SSE stream events and extract text content
    ///
    /// Parses provider-specific SSE format and extracts text content.
    fn parse_sse_content(&self, raw_sse: &str) -> ExtractedContent;

    /// Parse non-streaming response and extract text content
    ///
    /// Extracts the assistant's text response from a non-streaming JSON response.
    fn parse_response_content(&self, response_body: &Value) -> Option<String>;
}
