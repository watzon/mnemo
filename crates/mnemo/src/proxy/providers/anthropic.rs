use crate::error::{MnemoError, Result};
use crate::memory::retrieval::RetrievedMemory;
use crate::proxy::providers::LLMProvider;
use crate::proxy::streaming::ExtractedContent;
use crate::proxy::{format_memory_block, truncate_to_budget};
use serde_json::Value;

pub struct AnthropicProvider;

impl AnthropicProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AnthropicProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LLMProvider for AnthropicProvider {
    fn inject_memories(
        &self,
        request_body: &mut Value,
        memories: &[RetrievedMemory],
        max_tokens: usize,
    ) -> Result<()> {
        if memories.is_empty() {
            return Ok(());
        }

        let truncated = truncate_to_budget(memories, max_tokens);
        if truncated.is_empty() {
            return Ok(());
        }

        let memory_block = format_memory_block(&truncated);

        let obj = request_body
            .as_object_mut()
            .ok_or_else(|| MnemoError::Proxy("Request body is not an object".into()))?;

        match obj.get_mut("system") {
            Some(system_value) => {
                if let Some(system_str) = system_value.as_str() {
                    let new_content = if system_str.is_empty() {
                        memory_block
                    } else {
                        format!("{system_str}\n\n{memory_block}")
                    };
                    *system_value = Value::String(new_content);
                } else {
                    return Err(MnemoError::Proxy("System field is not a string".into()));
                }
            }
            None => {
                obj.insert("system".to_string(), Value::String(memory_block));
            }
        }

        Ok(())
    }

    fn extract_user_query(&self, request_body: &Value) -> Option<String> {
        let messages = request_body.get("messages")?.as_array()?;

        messages
            .iter()
            .rev()
            .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
            .and_then(|m| {
                let content = m.get("content")?;

                if let Some(s) = content.as_str() {
                    return Some(s.to_string());
                }

                if let Some(arr) = content.as_array() {
                    for block in arr {
                        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                return Some(text.to_string());
                            }
                        }
                    }
                }

                None
            })
    }

    fn parse_sse_content(&self, raw_sse: &str) -> ExtractedContent {
        parse_anthropic_sse(raw_sse)
    }

    fn parse_response_content(&self, response_body: &Value) -> Option<String> {
        let content = response_body.get("content")?.as_array()?;
        let mut result = String::new();

        for block in content {
            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    result.push_str(text);
                }
            }
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }
}

pub fn parse_anthropic_sse(raw: &str) -> ExtractedContent {
    let mut content = String::new();
    let mut is_complete = false;
    let mut event_count = 0;

    let mut lines = raw.lines().peekable();

    while let Some(line) = lines.next() {
        if line.starts_with("event:") {
            let event_type = line.strip_prefix("event:").unwrap_or("").trim();

            if let Some(data_line) = lines.next() {
                if let Some(data) = data_line.strip_prefix("data:") {
                    let data = data.trim();
                    event_count += 1;

                    match event_type {
                        "message_stop" => {
                            is_complete = true;
                        }
                        "content_block_delta" => {
                            if let Ok(json) = serde_json::from_str::<Value>(data) {
                                if let Some(delta) = json.get("delta") {
                                    let delta_type = delta.get("type").and_then(|t| t.as_str());

                                    if delta_type == Some("text_delta") {
                                        if let Some(text) =
                                            delta.get("text").and_then(|t| t.as_str())
                                        {
                                            content.push_str(text);
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    ExtractedContent {
        content,
        is_complete,
        event_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_anthropic_text_delta() {
        let raw = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n";
        let result = parse_anthropic_sse(raw);
        assert_eq!(result.content, "Hello");
    }

    #[test]
    fn test_parse_anthropic_full_stream() {
        let raw = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[]}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" World"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_stop
data: {"type":"message_stop"}

"#;
        let result = parse_anthropic_sse(raw);
        assert_eq!(result.content, "Hello World");
        assert!(result.is_complete);
    }

    #[test]
    fn test_anthropic_skip_thinking_delta() {
        let raw = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me think..."}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Answer"}}

"#;
        let result = parse_anthropic_sse(raw);
        assert_eq!(result.content, "Answer");
        assert!(!result.content.contains("think"));
    }

    #[test]
    fn test_anthropic_skip_tool_use_delta() {
        let raw = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"loc"}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Here's the result"}}

"#;
        let result = parse_anthropic_sse(raw);
        assert_eq!(result.content, "Here's the result");
    }

    #[test]
    fn test_anthropic_handles_ping() {
        let raw = r#"event: ping
data: {"type":"ping"}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

event: ping
data: {"type":"ping"}

"#;
        let result = parse_anthropic_sse(raw);
        assert_eq!(result.content, "Hello");
    }

    #[test]
    fn test_anthropic_parse_response_content() {
        let provider = AnthropicProvider::new();
        let response = serde_json::json!({
            "content": [{"type": "text", "text": "Hello World"}],
            "role": "assistant"
        });

        assert_eq!(
            provider.parse_response_content(&response),
            Some("Hello World".to_string())
        );
    }

    #[test]
    fn test_anthropic_parse_response_multiple_blocks() {
        let provider = AnthropicProvider::new();
        let response = serde_json::json!({
            "content": [
                {"type": "text", "text": "Hello"},
                {"type": "text", "text": " World"}
            ]
        });

        assert_eq!(
            provider.parse_response_content(&response),
            Some("Hello World".to_string())
        );
    }

    #[test]
    fn test_anthropic_skip_tool_use_block() {
        let provider = AnthropicProvider::new();
        let response = serde_json::json!({
            "content": [
                {"type": "tool_use", "id": "toolu_123", "name": "get_weather"},
                {"type": "text", "text": "Here's the result"}
            ]
        });

        assert_eq!(
            provider.parse_response_content(&response),
            Some("Here's the result".to_string())
        );
    }

    #[test]
    fn test_anthropic_inject_to_existing_system() {
        let provider = AnthropicProvider::new();
        let mut request = serde_json::json!({
            "system": "You are helpful.",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        use crate::memory::retrieval::RetrievedMemory;
        use crate::memory::types::{Memory, MemorySource, MemoryType};

        let memory = Memory::new(
            "Test memory".to_string(),
            vec![0.0; 384],
            MemoryType::Semantic,
            MemorySource::Manual,
        );
        let rm = RetrievedMemory {
            memory,
            similarity_score: 0.9,
            effective_weight: 0.8,
            final_score: 0.85,
        };

        provider.inject_memories(&mut request, &[rm], 2000).unwrap();

        let system = request["system"].as_str().unwrap();
        assert!(system.starts_with("You are helpful."));
        assert!(system.contains("<mnemo-memories>"));
    }

    #[test]
    fn test_anthropic_inject_creates_system() {
        let provider = AnthropicProvider::new();
        let mut request = serde_json::json!({
            "messages": [{"role": "user", "content": "Hello"}]
        });

        use crate::memory::retrieval::RetrievedMemory;
        use crate::memory::types::{Memory, MemorySource, MemoryType};

        let memory = Memory::new(
            "Test memory".to_string(),
            vec![0.0; 384],
            MemoryType::Semantic,
            MemorySource::Manual,
        );
        let rm = RetrievedMemory {
            memory,
            similarity_score: 0.9,
            effective_weight: 0.8,
            final_score: 0.85,
        };

        provider.inject_memories(&mut request, &[rm], 2000).unwrap();

        assert!(request.get("system").is_some());
        let system = request["system"].as_str().unwrap();
        assert!(system.contains("<mnemo-memories>"));
    }

    #[test]
    fn test_anthropic_extract_query_string_content() {
        let provider = AnthropicProvider::new();
        let request = serde_json::json!({
            "messages": [{"role": "user", "content": "Hello"}]
        });

        assert_eq!(
            provider.extract_user_query(&request),
            Some("Hello".to_string())
        );
    }

    #[test]
    fn test_anthropic_extract_query_array_content() {
        let provider = AnthropicProvider::new();
        let request = serde_json::json!({
            "messages": [{"role": "user", "content": [{"type": "text", "text": "Hello"}]}]
        });

        assert_eq!(
            provider.extract_user_query(&request),
            Some("Hello".to_string())
        );
    }

    #[test]
    fn test_anthropic_extract_query_with_image() {
        let provider = AnthropicProvider::new();
        let request = serde_json::json!({
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "image", "source": {"type": "base64", "data": "..."}},
                    {"type": "text", "text": "What is this?"}
                ]
            }]
        });

        assert_eq!(
            provider.extract_user_query(&request),
            Some("What is this?".to_string())
        );
    }

    #[test]
    fn test_anthropic_inject_empty_memories_noop() {
        let provider = AnthropicProvider::new();
        let original = serde_json::json!({
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let mut request = original.clone();

        provider.inject_memories(&mut request, &[], 2000).unwrap();

        assert_eq!(request, original);
    }
}
