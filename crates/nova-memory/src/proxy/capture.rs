//! Response Capture and Ingestion
//!
//! This module captures assistant responses from OpenAI streaming format
//! and triggers memory ingestion for episodic memory storage.

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::memory::ingestion::IngestionPipeline;
use crate::memory::types::MemorySource;
use crate::proxy::streaming::StreamingProxy;

/// Minimum content length for ingestion (in characters)
const MIN_RESPONSE_LENGTH: usize = 10;

/// Response capture and ingestion handler.
///
/// Parses assistant responses from buffered SSE streams and
/// triggers background ingestion into the memory system.
pub struct ResponseCapture;

impl ResponseCapture {
    /// Parse assistant content from buffered SSE response.
    ///
    /// Uses `StreamingProxy::extract_response_content` to parse the raw SSE
    /// data and extract all content deltas into a single string.
    ///
    /// # Arguments
    /// * `buffered` - Raw SSE data accumulated during streaming
    ///
    /// # Returns
    /// * `Some(String)` - Extracted content if non-empty
    /// * `None` - If content is empty or parsing fails
    pub fn parse_assistant_content(buffered: &str) -> Option<String> {
        let extracted = StreamingProxy::extract_response_content(buffered);

        if extracted.content.trim().is_empty() {
            debug!("Parsed SSE content is empty");
            return None;
        }

        debug!(
            "Extracted {} chars from {} SSE events (complete: {})",
            extracted.content.len(),
            extracted.event_count,
            extracted.is_complete
        );

        Some(extracted.content)
    }

    /// Check if response content should be ingested.
    ///
    /// Filters out content that shouldn't be stored in memory:
    /// - Empty or whitespace-only content
    /// - Very short responses (< 10 chars)
    /// - Error-like responses
    /// - Apology patterns that indicate failure
    ///
    /// # Arguments
    /// * `content` - The extracted response content
    ///
    /// # Returns
    /// * `true` - Content should be ingested
    /// * `false` - Content should be skipped
    pub fn should_ingest(content: &str) -> bool {
        let trimmed = content.trim();

        if trimmed.is_empty() {
            debug!("Skipping ingestion: empty content");
            return false;
        }

        if trimmed.len() < MIN_RESPONSE_LENGTH {
            debug!(
                "Skipping ingestion: content too short ({} chars)",
                trimmed.len()
            );
            return false;
        }

        let is_error = trimmed.starts_with("Error:")
            || trimmed.starts_with("error:")
            || trimmed.starts_with("ERROR:");
        if is_error {
            debug!("Skipping ingestion: error response");
            return false;
        }

        let is_refusal = trimmed.starts_with("I'm sorry")
            || trimmed.starts_with("I apologize")
            || trimmed.starts_with("I cannot")
            || trimmed.starts_with("I can't");
        if is_refusal {
            debug!("Skipping ingestion: apology/refusal response");
            return false;
        }

        true
    }

    /// Capture response and trigger ingestion (fire and forget).
    ///
    /// This method spawns a background task to:
    /// 1. Parse the buffered SSE content
    /// 2. Check if content should be ingested
    /// 3. Call the ingestion pipeline
    ///
    /// The ingestion runs asynchronously and does not block the caller.
    /// Any errors during ingestion are logged but not propagated.
    ///
    /// # Arguments
    /// * `buffered` - Raw SSE data accumulated during streaming
    /// * `conversation_id` - Conversation ID for episodic memory association
    /// * `pipeline` - Shared ingestion pipeline instance
    pub fn capture_and_ingest(
        buffered: String,
        conversation_id: String,
        pipeline: Arc<Mutex<IngestionPipeline>>,
    ) {
        tokio::spawn(async move {
            let content = match Self::parse_assistant_content(&buffered) {
                Some(c) => c,
                None => {
                    debug!("No content to ingest from response");
                    return;
                }
            };

            if !Self::should_ingest(&content) {
                return;
            }

            debug!(
                "Ingesting response ({} chars) for conversation {}",
                content.len(),
                conversation_id
            );

            let mut pipeline = pipeline.lock().await;
            match pipeline
                .ingest(&content, MemorySource::Conversation, Some(conversation_id.clone()))
                .await
            {
                Ok(Some(memory)) => {
                    debug!(
                        "Successfully ingested response as memory {} (type: {:?})",
                        memory.id, memory.memory_type
                    );
                }
                Ok(None) => {
                    debug!("Content was filtered by ingestion pipeline");
                }
                Err(e) => {
                    warn!(
                        "Failed to ingest response for conversation {}: {}",
                        conversation_id, e
                    );
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_assistant_content_valid_sse() {
        let raw = r#"data: {"id":"1","choices":[{"index":0,"delta":{"role":"assistant"}}]}

data: {"id":"1","choices":[{"index":0,"delta":{"content":"Hello"}}]}

data: {"id":"1","choices":[{"index":0,"delta":{"content":" "}}]}

data: {"id":"1","choices":[{"index":0,"delta":{"content":"World"}}]}

data: {"id":"1","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
"#;

        let content = ResponseCapture::parse_assistant_content(raw);
        assert!(content.is_some());
        assert_eq!(content.unwrap(), "Hello World");
    }

    #[test]
    fn test_parse_assistant_content_empty() {
        let raw = r#"data: {"id":"1","choices":[{"index":0,"delta":{"role":"assistant"}}]}

data: {"id":"1","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
"#;

        let content = ResponseCapture::parse_assistant_content(raw);
        assert!(content.is_none());
    }

    #[test]
    fn test_parse_assistant_content_whitespace_only() {
        let raw = r#"data: {"id":"1","choices":[{"index":0,"delta":{"content":"   "}}]}

data: [DONE]
"#;

        let content = ResponseCapture::parse_assistant_content(raw);
        assert!(content.is_none());
    }

    #[test]
    fn test_should_ingest_valid_content() {
        assert!(ResponseCapture::should_ingest(
            "This is a valid response with enough content."
        ));
        assert!(ResponseCapture::should_ingest(
            "Here is some helpful information about your question."
        ));
    }

    #[test]
    fn test_should_ingest_empty() {
        assert!(!ResponseCapture::should_ingest(""));
        assert!(!ResponseCapture::should_ingest("   "));
        assert!(!ResponseCapture::should_ingest("\n\t"));
    }

    #[test]
    fn test_should_ingest_too_short() {
        assert!(!ResponseCapture::should_ingest("Hi"));
        assert!(!ResponseCapture::should_ingest("OK"));
        assert!(!ResponseCapture::should_ingest("123456789"));
        assert!(ResponseCapture::should_ingest("1234567890"));
    }

    #[test]
    fn test_should_ingest_error_responses() {
        assert!(!ResponseCapture::should_ingest(
            "Error: Something went wrong with the API."
        ));
        assert!(!ResponseCapture::should_ingest(
            "error: invalid request format"
        ));
        assert!(!ResponseCapture::should_ingest(
            "ERROR: Rate limit exceeded"
        ));
    }

    #[test]
    fn test_should_ingest_apology_responses() {
        assert!(!ResponseCapture::should_ingest(
            "I'm sorry, I cannot help with that request."
        ));
        assert!(!ResponseCapture::should_ingest(
            "I apologize, but I'm unable to process that."
        ));
        assert!(!ResponseCapture::should_ingest(
            "I cannot assist with this type of request."
        ));
        assert!(!ResponseCapture::should_ingest(
            "I can't provide that information."
        ));
    }

    #[test]
    fn test_should_ingest_borderline_content() {
        assert!(ResponseCapture::should_ingest(
            "Errors can be handled using try-catch blocks in JavaScript."
        ));
        assert!(ResponseCapture::should_ingest(
            "I'm happy to help you with your coding question!"
        ));
    }

    #[test]
    fn test_full_parsing_flow() {
        let raw = r#"data: {"id":"chatcmpl-abc123","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}

data: {"id":"chatcmpl-abc123","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4","choices":[{"index":0,"delta":{"content":"The"},"finish_reason":null}]}

data: {"id":"chatcmpl-abc123","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4","choices":[{"index":0,"delta":{"content":" capital"},"finish_reason":null}]}

data: {"id":"chatcmpl-abc123","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4","choices":[{"index":0,"delta":{"content":" of"},"finish_reason":null}]}

data: {"id":"chatcmpl-abc123","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4","choices":[{"index":0,"delta":{"content":" France"},"finish_reason":null}]}

data: {"id":"chatcmpl-abc123","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4","choices":[{"index":0,"delta":{"content":" is"},"finish_reason":null}]}

data: {"id":"chatcmpl-abc123","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4","choices":[{"index":0,"delta":{"content":" Paris."},"finish_reason":null}]}

data: {"id":"chatcmpl-abc123","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
"#;

        let content = ResponseCapture::parse_assistant_content(raw);
        assert!(content.is_some());
        let content = content.unwrap();
        assert_eq!(content, "The capital of France is Paris.");

        assert!(ResponseCapture::should_ingest(&content));
    }

    #[test]
    fn test_incomplete_stream() {
        let raw = r#"data: {"id":"1","choices":[{"index":0,"delta":{"content":"Partial response that got"}}]}

data: {"id":"1","choices":[{"index":0,"delta":{"content":" cut off..."}}]}

"#;

        let content = ResponseCapture::parse_assistant_content(raw);
        assert!(content.is_some());
        assert_eq!(content.unwrap(), "Partial response that got cut off...");
    }
}
