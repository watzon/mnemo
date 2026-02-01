use crate::error::Result;
use crate::memory::retrieval::RetrievedMemory;
use crate::proxy::providers::LLMProvider;
use crate::proxy::streaming::ExtractedContent;
use crate::proxy::streaming::StreamingProxy;
use crate::proxy::{extract_user_query as do_extract_query, inject_memories as do_inject_memories};
use serde_json::Value;

pub struct OpenAiProvider;

impl OpenAiProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OpenAiProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LLMProvider for OpenAiProvider {
    fn inject_memories(
        &self,
        request_body: &mut Value,
        memories: &[RetrievedMemory],
        max_tokens: usize,
    ) -> Result<()> {
        do_inject_memories(request_body, memories, max_tokens)
    }

    fn extract_user_query(&self, request_body: &Value) -> Option<String> {
        do_extract_query(request_body)
    }

    fn parse_sse_content(&self, raw_sse: &str) -> ExtractedContent {
        StreamingProxy::extract_response_content(raw_sse)
    }

    fn parse_response_content(&self, response_body: &Value) -> Option<String> {
        response_body
            .get("choices")?
            .get(0)?
            .get("message")?
            .get("content")?
            .as_str()
            .map(|s| s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_parse_response_content() {
        let provider = OpenAiProvider::new();
        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello World"
                }
            }]
        });

        assert_eq!(
            provider.parse_response_content(&response),
            Some("Hello World".to_string())
        );
    }

    #[test]
    fn test_openai_parse_response_content_empty() {
        let provider = OpenAiProvider::new();
        let response = serde_json::json!({
            "choices": []
        });

        assert_eq!(provider.parse_response_content(&response), None);
    }
}
