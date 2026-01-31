//! SSE streaming passthrough with tee functionality
//!
//! This module provides streaming proxy capabilities that:
//! - Forward SSE streams to clients with zero added latency
//! - Buffer the stream content for post-completion ingestion
//! - Parse SSE events and extract content from OpenAI format

use bytes::Bytes;
use futures::stream::Stream;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::{Mutex, oneshot};

/// Result of teeing a stream - provides both the forwarding stream and a handle to get buffered content
pub struct TeeResult<S> {
    /// Stream to forward to client (yields original chunks unchanged)
    pub client_stream: S,
    /// Handle to retrieve buffered content after stream completes
    pub buffer_handle: BufferHandle,
}

/// Handle to retrieve buffered stream content after completion
pub struct BufferHandle {
    receiver: oneshot::Receiver<Vec<u8>>,
}

impl BufferHandle {
    /// Get the buffered content after the stream completes
    /// Returns the raw bytes accumulated during streaming
    pub async fn get_raw_content(self) -> Vec<u8> {
        self.receiver.await.unwrap_or_default()
    }

    /// Get the buffered content as a string (lossy UTF-8 conversion)
    pub async fn get_content_string(self) -> String {
        let bytes = self.get_raw_content().await;
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

/// A stream wrapper that buffers all chunks while forwarding them
pub struct TeeStream<S>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Unpin,
{
    inner: S,
    buffer: Arc<Mutex<Vec<u8>>>,
    sender: Option<oneshot::Sender<Vec<u8>>>,
}

impl<S> Stream for TeeStream<S>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Unpin,
{
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;

        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                let buffer = Arc::clone(&this.buffer);
                let bytes_clone = bytes.clone();
                tokio::spawn(async move {
                    let mut buf = buffer.lock().await;
                    buf.extend_from_slice(&bytes_clone);
                });

                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => {
                if let Some(sender) = this.sender.take() {
                    let buffer = Arc::clone(&this.buffer);
                    tokio::spawn(async move {
                        let buf = buffer.lock().await;
                        let _ = sender.send(buf.clone());
                    });
                }
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// SSE streaming proxy functionality
pub struct StreamingProxy;

impl StreamingProxy {
    /// Tee an incoming stream - forwards all chunks to client while buffering for later ingestion
    ///
    /// # Arguments
    /// * `incoming` - The incoming byte stream (typically from upstream HTTP response)
    ///
    /// # Returns
    /// A `TeeResult` containing:
    /// - `client_stream` - Forward this to the client response
    /// - `buffer_handle` - Use this to get the complete buffered content after stream ends
    pub fn tee_stream<S>(incoming: S) -> TeeResult<TeeStream<S>>
    where
        S: Stream<Item = Result<Bytes, std::io::Error>> + Unpin + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let buffer = Arc::new(Mutex::new(Vec::new()));

        let tee_stream = TeeStream {
            inner: incoming,
            buffer,
            sender: Some(tx),
        };

        TeeResult {
            client_stream: tee_stream,
            buffer_handle: BufferHandle { receiver: rx },
        }
    }

    /// Parse raw SSE data and extract all events
    ///
    /// SSE format:
    /// ```text
    /// data: {"json": "content"}
    ///
    /// data: more content
    ///
    /// data: [DONE]
    /// ```
    pub fn parse_sse_events(raw: &str) -> Vec<SseEvent> {
        let mut events = Vec::new();
        let mut current_data = String::new();

        for line in raw.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    events.push(SseEvent::Done);
                } else if !current_data.is_empty() {
                    current_data.push('\n');
                    current_data.push_str(data);
                } else {
                    current_data = data.to_string();
                }
            } else if line.is_empty() && !current_data.is_empty() {
                events.push(SseEvent::Data(std::mem::take(&mut current_data)));
            }
        }

        if !current_data.is_empty() {
            events.push(SseEvent::Data(current_data));
        }

        events
    }

    /// Extract content from OpenAI streaming format
    ///
    /// OpenAI chunks look like:
    /// ```json
    /// {"id":"chatcmpl-123","choices":[{"index":0,"delta":{"content":"Hello"}}]}
    /// ```
    pub fn extract_openai_content(events: &[SseEvent]) -> String {
        let mut content = String::new();

        for event in events {
            if let SseEvent::Data(data) = event {
                if let Some(delta_content) = Self::parse_openai_delta(data) {
                    content.push_str(&delta_content);
                }
            }
        }

        content
    }

    fn parse_openai_delta(json_str: &str) -> Option<String> {
        let value: serde_json::Value = serde_json::from_str(json_str).ok()?;

        value
            .get("choices")?
            .get(0)?
            .get("delta")?
            .get("content")?
            .as_str()
            .map(|s| s.to_string())
    }

    /// Extract full response from buffered SSE stream
    ///
    /// This is the main entry point for post-stream content extraction.
    /// It parses the raw SSE data and extracts all content deltas.
    pub fn extract_response_content(raw_sse: &str) -> ExtractedContent {
        let events = Self::parse_sse_events(raw_sse);
        let content = Self::extract_openai_content(&events);
        let is_complete = events.iter().any(|e| matches!(e, SseEvent::Done));

        ExtractedContent {
            content,
            is_complete,
            event_count: events.len(),
        }
    }
}

/// Represents a parsed SSE event
#[derive(Debug, Clone, PartialEq)]
pub enum SseEvent {
    /// Data event containing the payload
    Data(String),
    /// Terminal [DONE] marker
    Done,
}

/// Extracted content from an SSE stream
#[derive(Debug, Clone)]
pub struct ExtractedContent {
    /// The concatenated content from all delta chunks
    pub content: String,
    /// Whether the stream completed with [DONE]
    pub is_complete: bool,
    /// Number of events parsed
    pub event_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream::{self, StreamExt};

    #[test]
    fn test_parse_sse_events_basic() {
        let raw = r#"data: {"text":"Hello"}

data: {"text":" world"}

data: [DONE]
"#;

        let events = StreamingProxy::parse_sse_events(raw);

        assert_eq!(events.len(), 3);
        assert_eq!(events[0], SseEvent::Data(r#"{"text":"Hello"}"#.to_string()));
        assert_eq!(
            events[1],
            SseEvent::Data(r#"{"text":" world"}"#.to_string())
        );
        assert_eq!(events[2], SseEvent::Done);
    }

    #[test]
    fn test_parse_sse_events_openai_format() {
        let raw = r#"data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1234567890,"model":"gpt-4","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1234567890,"model":"gpt-4","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1234567890,"model":"gpt-4","choices":[{"index":0,"delta":{"content":"!"},"finish_reason":null}]}

data: [DONE]
"#;

        let events = StreamingProxy::parse_sse_events(raw);
        assert_eq!(events.len(), 4);

        let content = StreamingProxy::extract_openai_content(&events);
        assert_eq!(content, "Hello!");
    }

    #[test]
    fn test_parse_openai_delta_with_content() {
        let json = r#"{"id":"chatcmpl-123","choices":[{"index":0,"delta":{"content":"test"}}]}"#;
        let content = StreamingProxy::parse_openai_delta(json);
        assert_eq!(content, Some("test".to_string()));
    }

    #[test]
    fn test_parse_openai_delta_role_only() {
        let json = r#"{"id":"chatcmpl-123","choices":[{"index":0,"delta":{"role":"assistant"}}]}"#;
        let content = StreamingProxy::parse_openai_delta(json);
        assert_eq!(content, None);
    }

    #[test]
    fn test_parse_openai_delta_empty_delta() {
        let json =
            r#"{"id":"chatcmpl-123","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#;
        let content = StreamingProxy::parse_openai_delta(json);
        assert_eq!(content, None);
    }

    #[test]
    fn test_extract_response_content() {
        let raw = r#"data: {"id":"1","choices":[{"index":0,"delta":{"content":"Hello"}}]}

data: {"id":"1","choices":[{"index":0,"delta":{"content":" "}}]}

data: {"id":"1","choices":[{"index":0,"delta":{"content":"World"}}]}

data: [DONE]
"#;

        let extracted = StreamingProxy::extract_response_content(raw);

        assert_eq!(extracted.content, "Hello World");
        assert!(extracted.is_complete);
        assert_eq!(extracted.event_count, 4);
    }

    #[test]
    fn test_extract_response_content_incomplete() {
        let raw = r#"data: {"id":"1","choices":[{"index":0,"delta":{"content":"Partial"}}]}

"#;

        let extracted = StreamingProxy::extract_response_content(raw);

        assert_eq!(extracted.content, "Partial");
        assert!(!extracted.is_complete);
    }

    #[test]
    fn test_parse_sse_with_comments() {
        let raw = r#": this is a comment
data: {"text":"Hello"}

: another comment
data: [DONE]
"#;

        let events = StreamingProxy::parse_sse_events(raw);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], SseEvent::Data(r#"{"text":"Hello"}"#.to_string()));
        assert_eq!(events[1], SseEvent::Done);
    }

    #[test]
    fn test_parse_sse_no_trailing_newline() {
        let raw = "data: {\"text\":\"test\"}";

        let events = StreamingProxy::parse_sse_events(raw);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], SseEvent::Data(r#"{"text":"test"}"#.to_string()));
    }

    #[tokio::test]
    async fn test_tee_stream_buffers_content() {
        let chunks = vec![
            Ok(Bytes::from("data: {\"a\":1}\n\n")),
            Ok(Bytes::from("data: {\"b\":2}\n\n")),
            Ok(Bytes::from("data: [DONE]\n\n")),
        ];

        let incoming = stream::iter(chunks);
        let TeeResult {
            mut client_stream,
            buffer_handle,
        } = StreamingProxy::tee_stream(incoming);

        let mut forwarded = Vec::new();
        while let Some(chunk) = client_stream.next().await {
            if let Ok(bytes) = chunk {
                forwarded.extend_from_slice(&bytes);
            }
        }

        let buffered = buffer_handle.get_content_string().await;

        let forwarded_str = String::from_utf8_lossy(&forwarded);
        assert_eq!(forwarded_str, buffered);
        assert!(buffered.contains("data: {\"a\":1}"));
        assert!(buffered.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn test_tee_stream_forwards_immediately() {
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(10);

        let incoming = tokio_stream::wrappers::ReceiverStream::new(rx);
        let TeeResult {
            mut client_stream,
            buffer_handle: _,
        } = StreamingProxy::tee_stream(incoming);

        tx.send(Ok(Bytes::from("chunk1"))).await.unwrap();

        let first = client_stream.next().await;
        assert!(first.is_some());
        assert_eq!(first.unwrap().unwrap(), Bytes::from("chunk1"));

        tx.send(Ok(Bytes::from("chunk2"))).await.unwrap();
        let second = client_stream.next().await;
        assert_eq!(second.unwrap().unwrap(), Bytes::from("chunk2"));

        drop(tx);

        let end = client_stream.next().await;
        assert!(end.is_none());
    }
}
