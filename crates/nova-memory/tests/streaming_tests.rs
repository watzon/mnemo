//! Integration tests for SSE streaming functionality
//!
//! Tests SSE event parsing, content extraction, buffer accumulation,
//! and response reconstruction from OpenAI streaming format.

use bytes::Bytes;
use futures::stream::{self, StreamExt};
use nova_memory::proxy::{SseEvent, StreamingProxy};

mod sse_event_parsing_tests {
    use super::*;

    #[test]
    fn test_parse_single_data_event() {
        let raw = "data: {\"text\":\"hello\"}\n\n";
        let events = StreamingProxy::parse_sse_events(raw);

        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], SseEvent::Data(s) if s == "{\"text\":\"hello\"}"));
    }

    #[test]
    fn test_parse_multiple_data_events() {
        let raw = "data: first\n\ndata: second\n\ndata: third\n\n";
        let events = StreamingProxy::parse_sse_events(raw);

        assert_eq!(events.len(), 3);
        assert!(matches!(&events[0], SseEvent::Data(s) if s == "first"));
        assert!(matches!(&events[1], SseEvent::Data(s) if s == "second"));
        assert!(matches!(&events[2], SseEvent::Data(s) if s == "third"));
    }

    #[test]
    fn test_parse_done_marker() {
        let raw = "data: {\"content\":\"test\"}\n\ndata: [DONE]\n";
        let events = StreamingProxy::parse_sse_events(raw);

        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], SseEvent::Data(_)));
        assert!(matches!(&events[1], SseEvent::Done));
    }

    #[test]
    fn test_parse_ignores_comments() {
        let raw = ": this is a comment\ndata: actual data\n\n: another comment\ndata: [DONE]\n";
        let events = StreamingProxy::parse_sse_events(raw);

        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], SseEvent::Data(s) if s == "actual data"));
        assert!(matches!(&events[1], SseEvent::Done));
    }

    #[test]
    fn test_parse_empty_input() {
        let events = StreamingProxy::parse_sse_events("");
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_whitespace_only() {
        let events = StreamingProxy::parse_sse_events("   \n\n   \n");
        assert!(events.is_empty());
    }

    #[test]
    fn test_parse_no_trailing_newline() {
        let raw = "data: {\"text\":\"test\"}";
        let events = StreamingProxy::parse_sse_events(raw);

        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], SseEvent::Data(s) if s == "{\"text\":\"test\"}"));
    }

    #[test]
    fn test_parse_openai_chunk_format() {
        let raw = r#"data: {"id":"chatcmpl-123","object":"chat.completion.chunk","choices":[{"delta":{"content":"Hi"}}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","choices":[{"delta":{"content":"!"}}]}

data: [DONE]
"#;
        let events = StreamingProxy::parse_sse_events(raw);

        assert_eq!(events.len(), 3);
        assert!(matches!(&events[2], SseEvent::Done));
    }
}

mod openai_content_extraction_tests {
    use super::*;

    #[test]
    fn test_extract_single_content_delta() {
        let events = vec![SseEvent::Data(
            r#"{"id":"1","choices":[{"delta":{"content":"Hello"}}]}"#.to_string(),
        )];

        let content = StreamingProxy::extract_openai_content(&events);
        assert_eq!(content, "Hello");
    }

    #[test]
    fn test_extract_multiple_content_deltas() {
        let events = vec![
            SseEvent::Data(r#"{"choices":[{"delta":{"content":"Hello"}}]}"#.to_string()),
            SseEvent::Data(r#"{"choices":[{"delta":{"content":" "}}]}"#.to_string()),
            SseEvent::Data(r#"{"choices":[{"delta":{"content":"World"}}]}"#.to_string()),
            SseEvent::Done,
        ];

        let content = StreamingProxy::extract_openai_content(&events);
        assert_eq!(content, "Hello World");
    }

    #[test]
    fn test_extract_ignores_role_delta() {
        let events = vec![
            SseEvent::Data(r#"{"choices":[{"delta":{"role":"assistant"}}]}"#.to_string()),
            SseEvent::Data(r#"{"choices":[{"delta":{"content":"Response"}}]}"#.to_string()),
        ];

        let content = StreamingProxy::extract_openai_content(&events);
        assert_eq!(content, "Response");
    }

    #[test]
    fn test_extract_ignores_empty_delta() {
        let events = vec![
            SseEvent::Data(r#"{"choices":[{"delta":{"content":"Start"}}]}"#.to_string()),
            SseEvent::Data(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#.to_string()),
            SseEvent::Done,
        ];

        let content = StreamingProxy::extract_openai_content(&events);
        assert_eq!(content, "Start");
    }

    #[test]
    fn test_extract_empty_events() {
        let events: Vec<SseEvent> = vec![];
        let content = StreamingProxy::extract_openai_content(&events);
        assert!(content.is_empty());
    }

    #[test]
    fn test_extract_done_only() {
        let events = vec![SseEvent::Done];
        let content = StreamingProxy::extract_openai_content(&events);
        assert!(content.is_empty());
    }

    #[test]
    fn test_extract_handles_invalid_json() {
        let events = vec![
            SseEvent::Data("not valid json".to_string()),
            SseEvent::Data(r#"{"choices":[{"delta":{"content":"Valid"}}]}"#.to_string()),
        ];

        let content = StreamingProxy::extract_openai_content(&events);
        assert_eq!(content, "Valid");
    }

    #[test]
    fn test_extract_preserves_special_characters() {
        let events = vec![
            SseEvent::Data(r#"{"choices":[{"delta":{"content":"Hello\n"}}]}"#.to_string()),
            SseEvent::Data(r#"{"choices":[{"delta":{"content":"World\t!"}}]}"#.to_string()),
        ];

        let content = StreamingProxy::extract_openai_content(&events);
        assert_eq!(content, "Hello\nWorld\t!");
    }
}

mod response_reconstruction_tests {
    use super::*;

    #[test]
    fn test_extract_response_content_complete() {
        let raw = r#"data: {"choices":[{"delta":{"content":"Complete"}}]}

data: {"choices":[{"delta":{"content":" response"}}]}

data: [DONE]
"#;

        let extracted = StreamingProxy::extract_response_content(raw);

        assert_eq!(extracted.content, "Complete response");
        assert!(extracted.is_complete);
        assert_eq!(extracted.event_count, 3);
    }

    #[test]
    fn test_extract_response_content_incomplete() {
        let raw = r#"data: {"choices":[{"delta":{"content":"Partial"}}]}

data: {"choices":[{"delta":{"content":" content"}}]}

"#;

        let extracted = StreamingProxy::extract_response_content(raw);

        assert_eq!(extracted.content, "Partial content");
        assert!(!extracted.is_complete);
    }

    #[test]
    fn test_extract_response_content_empty() {
        let extracted = StreamingProxy::extract_response_content("");

        assert!(extracted.content.is_empty());
        assert!(!extracted.is_complete);
        assert_eq!(extracted.event_count, 0);
    }

    #[test]
    fn test_extract_response_realistic_stream() {
        let raw = r#"data: {"id":"chatcmpl-abc","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}

data: {"id":"chatcmpl-abc","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4","choices":[{"index":0,"delta":{"content":"The"},"finish_reason":null}]}

data: {"id":"chatcmpl-abc","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4","choices":[{"index":0,"delta":{"content":" answer"},"finish_reason":null}]}

data: {"id":"chatcmpl-abc","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4","choices":[{"index":0,"delta":{"content":" is"},"finish_reason":null}]}

data: {"id":"chatcmpl-abc","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4","choices":[{"index":0,"delta":{"content":" 42."},"finish_reason":null}]}

data: {"id":"chatcmpl-abc","object":"chat.completion.chunk","created":1700000000,"model":"gpt-4","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
"#;

        let extracted = StreamingProxy::extract_response_content(raw);

        assert_eq!(extracted.content, "The answer is 42.");
        assert!(extracted.is_complete);
        assert_eq!(extracted.event_count, 7);
    }
}

mod tee_stream_tests {
    use super::*;
    use nova_memory::proxy::TeeResult;

    #[tokio::test]
    async fn test_tee_stream_forwards_all_chunks() {
        let chunks = vec![
            Ok(Bytes::from("chunk1")),
            Ok(Bytes::from("chunk2")),
            Ok(Bytes::from("chunk3")),
        ];

        let incoming = stream::iter(chunks);
        let TeeResult {
            mut client_stream,
            buffer_handle: _,
        } = StreamingProxy::tee_stream(incoming);

        let mut received = Vec::new();
        while let Some(result) = client_stream.next().await {
            if let Ok(bytes) = result {
                received.push(String::from_utf8_lossy(&bytes).to_string());
            }
        }

        assert_eq!(received, vec!["chunk1", "chunk2", "chunk3"]);
    }

    #[tokio::test]
    async fn test_tee_stream_buffers_all_content() {
        let chunks = vec![
            Ok(Bytes::from("data: first\n\n")),
            Ok(Bytes::from("data: second\n\n")),
            Ok(Bytes::from("data: [DONE]\n\n")),
        ];

        let incoming = stream::iter(chunks);
        let TeeResult {
            mut client_stream,
            buffer_handle,
        } = StreamingProxy::tee_stream(incoming);

        while client_stream.next().await.is_some() {}

        let buffered = buffer_handle.get_content_string().await;

        assert!(buffered.contains("data: first"));
        assert!(buffered.contains("data: second"));
        assert!(buffered.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn test_tee_stream_client_and_buffer_match() {
        let chunks = vec![
            Ok(Bytes::from("A")),
            Ok(Bytes::from("B")),
            Ok(Bytes::from("C")),
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

        let buffered = buffer_handle.get_raw_content().await;

        assert_eq!(forwarded, buffered);
    }

    #[tokio::test]
    async fn test_tee_stream_empty_input() {
        let chunks: Vec<Result<Bytes, std::io::Error>> = vec![];

        let incoming = stream::iter(chunks);
        let TeeResult {
            mut client_stream,
            buffer_handle,
        } = StreamingProxy::tee_stream(incoming);

        let mut count = 0;
        while client_stream.next().await.is_some() {
            count += 1;
        }

        let buffered = buffer_handle.get_content_string().await;

        assert_eq!(count, 0);
        assert!(buffered.is_empty());
    }

    #[tokio::test]
    async fn test_tee_stream_with_sse_parsing() {
        let sse_data = r#"data: {"choices":[{"delta":{"content":"Test"}}]}

data: {"choices":[{"delta":{"content":" message"}}]}

data: [DONE]
"#;
        let chunks = vec![Ok(Bytes::from(sse_data))];

        let incoming = stream::iter(chunks);
        let TeeResult {
            mut client_stream,
            buffer_handle,
        } = StreamingProxy::tee_stream(incoming);

        while client_stream.next().await.is_some() {}

        let buffered = buffer_handle.get_content_string().await;
        let extracted = StreamingProxy::extract_response_content(&buffered);

        assert_eq!(extracted.content, "Test message");
        assert!(extracted.is_complete);
    }
}

mod buffer_handle_tests {
    use super::*;
    use nova_memory::proxy::TeeResult;

    #[tokio::test]
    async fn test_buffer_handle_get_raw_content() {
        let chunks = vec![Ok(Bytes::from(vec![0x48, 0x69]))]; // "Hi"

        let incoming = stream::iter(chunks);
        let TeeResult {
            mut client_stream,
            buffer_handle,
        } = StreamingProxy::tee_stream(incoming);

        while client_stream.next().await.is_some() {}

        let raw = buffer_handle.get_raw_content().await;
        assert_eq!(raw, vec![0x48, 0x69]);
    }

    #[tokio::test]
    async fn test_buffer_handle_get_content_string() {
        let chunks = vec![Ok(Bytes::from("Hello UTF-8"))];

        let incoming = stream::iter(chunks);
        let TeeResult {
            mut client_stream,
            buffer_handle,
        } = StreamingProxy::tee_stream(incoming);

        while client_stream.next().await.is_some() {}

        let content = buffer_handle.get_content_string().await;
        assert_eq!(content, "Hello UTF-8");
    }
}

mod edge_case_tests {
    use super::*;

    #[test]
    fn test_parse_very_long_content() {
        let long_content = "x".repeat(10000);
        let raw = format!("data: {long_content}\n\n");
        let events = StreamingProxy::parse_sse_events(&raw);

        assert_eq!(events.len(), 1);
        if let SseEvent::Data(data) = &events[0] {
            assert_eq!(data.len(), 10000);
        }
    }

    #[test]
    fn test_parse_unicode_content() {
        let raw = "data: {\"content\":\"Hello 世界 🌍\"}\n\n";
        let events = StreamingProxy::parse_sse_events(raw);

        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], SseEvent::Data(s) if s.contains("世界") && s.contains("🌍")));
    }

    #[test]
    fn test_extract_content_with_newlines() {
        let events = vec![
            SseEvent::Data(r#"{"choices":[{"delta":{"content":"Line 1\n"}}]}"#.to_string()),
            SseEvent::Data(r#"{"choices":[{"delta":{"content":"Line 2\n"}}]}"#.to_string()),
            SseEvent::Data(r#"{"choices":[{"delta":{"content":"Line 3"}}]}"#.to_string()),
        ];

        let content = StreamingProxy::extract_openai_content(&events);
        assert_eq!(content, "Line 1\nLine 2\nLine 3");
    }

    #[test]
    fn test_parse_consecutive_done_markers() {
        let raw = "data: [DONE]\ndata: [DONE]\n";
        let events = StreamingProxy::parse_sse_events(raw);

        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|e| matches!(e, SseEvent::Done)));
    }

    #[test]
    fn test_extract_from_mixed_valid_invalid() {
        let events = vec![
            SseEvent::Data("invalid".to_string()),
            SseEvent::Data(r#"{"choices":[{"delta":{"content":"Valid1"}}]}"#.to_string()),
            SseEvent::Data("{}".to_string()),
            SseEvent::Data(r#"{"choices":[{"delta":{"content":"Valid2"}}]}"#.to_string()),
            SseEvent::Done,
        ];

        let content = StreamingProxy::extract_openai_content(&events);
        assert_eq!(content, "Valid1Valid2");
    }
}
