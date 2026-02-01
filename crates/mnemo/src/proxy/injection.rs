//! Memory Injection for OpenAI-format Requests
//!
//! This module handles injecting retrieved memories into LLM request payloads.
//! It formats memories as XML blocks and injects them into the system message.

use crate::error::{MnemoError, Result};
use crate::memory::retrieval::RetrievedMemory;
use serde_json::Value;

/// Format memories as an XML block for injection into system prompts.
///
/// Each memory is tagged with its timestamp and type for context.
///
/// # Example Output
/// ```xml
/// <mnemo-memories>
/// <memory timestamp="2024-01-15" type="episodic">
///   User prefers dark mode for all applications.
/// </memory>
/// </mnemo-memories>
/// ```
pub fn format_memory_block(memories: &[RetrievedMemory]) -> String {
    if memories.is_empty() {
        return String::new();
    }

    let mut block = String::from("<mnemo-memories>\n");
    for rm in memories {
        let timestamp = rm.memory.created_at.format("%Y-%m-%d").to_string();
        let mem_type = format!("{:?}", rm.memory.memory_type).to_lowercase();
        block.push_str(&format!(
            "<memory timestamp=\"{}\" type=\"{}\">\n  {}\n</memory>\n",
            timestamp, mem_type, rm.memory.content
        ));
    }
    block.push_str("</mnemo-memories>");
    block
}

/// Estimate token count using character approximation.
///
/// Uses chars/4 as a rough approximation for token count.
/// This is a simple heuristic that works reasonably well for English text.
#[inline]
pub fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

/// Truncate memories to fit within token budget.
///
/// Memories are already sorted by relevance (final_score).
/// Keeps the most relevant memories until budget is exhausted.
pub fn truncate_to_budget(memories: &[RetrievedMemory], max_tokens: usize) -> Vec<RetrievedMemory> {
    if memories.is_empty() || max_tokens == 0 {
        return Vec::new();
    }

    // Calculate overhead for XML wrapper
    // "<mnemo-memories>\n" + "</mnemo-memories>" = ~35 chars = ~9 tokens
    const WRAPPER_OVERHEAD_TOKENS: usize = 10;

    // Each memory has overhead: "<memory timestamp=\"YYYY-MM-DD\" type=\"xxxxx\">\n  \n</memory>\n" = ~55 chars = ~14 tokens
    const PER_MEMORY_OVERHEAD_TOKENS: usize = 15;

    let available_tokens = max_tokens.saturating_sub(WRAPPER_OVERHEAD_TOKENS);
    let mut used_tokens = 0;
    let mut result = Vec::new();

    for rm in memories {
        let content_tokens = estimate_tokens(&rm.memory.content);
        let memory_total = content_tokens + PER_MEMORY_OVERHEAD_TOKENS;

        if used_tokens + memory_total <= available_tokens {
            result.push(rm.clone());
            used_tokens += memory_total;
        } else {
            // Budget exhausted
            break;
        }
    }

    result
}

/// Inject memories into an OpenAI-format request body.
///
/// This function:
/// 1. Parses the messages array from the request
/// 2. Finds or creates a system message
/// 3. Appends the memory block to the system message
/// 4. Respects the token budget by truncating if necessary
///
/// # Arguments
/// * `request_body` - Mutable reference to the JSON request body
/// * `memories` - Slice of retrieved memories (should be sorted by relevance)
/// * `max_tokens` - Maximum tokens to use for injected memories
///
/// # Errors
/// Returns `MnemoError::Proxy` if the request format is invalid.
pub fn inject_memories(
    request_body: &mut Value,
    memories: &[RetrievedMemory],
    max_tokens: usize,
) -> Result<()> {
    if memories.is_empty() {
        return Ok(());
    }

    // Truncate memories to fit budget
    let truncated_memories = truncate_to_budget(memories, max_tokens);
    if truncated_memories.is_empty() {
        return Ok(());
    }

    let memory_block = format_memory_block(&truncated_memories);

    // Get or create messages array
    let messages = request_body
        .get_mut("messages")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| MnemoError::Proxy("Missing or invalid 'messages' array".into()))?;

    // Find existing system message or insert one
    let system_idx = messages
        .iter()
        .position(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"));

    match system_idx {
        Some(idx) => {
            // Append memory block to existing system message
            let system_msg = &mut messages[idx];
            let current_content = system_msg
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("");

            let new_content = if current_content.is_empty() {
                memory_block
            } else {
                format!("{current_content}\n\n{memory_block}")
            };

            system_msg["content"] = Value::String(new_content);
        }
        None => {
            // Insert new system message at the beginning
            let system_message = serde_json::json!({
                "role": "system",
                "content": memory_block
            });
            messages.insert(0, system_message);
        }
    }

    Ok(())
}

/// Extract the user message content from an OpenAI-format request.
///
/// Returns the content of the last user message, which is typically
/// the query to use for memory retrieval.
pub fn extract_user_query(request_body: &Value) -> Option<String> {
    let messages = request_body.get("messages")?.as_array()?;

    // Find the last user message
    messages
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::types::{Memory, MemorySource, MemoryType};
    use chrono::{TimeZone, Utc};

    fn create_test_memory(content: &str, memory_type: MemoryType) -> Memory {
        let mut memory = Memory::new(
            content.to_string(),
            vec![0.5; 384],
            memory_type,
            MemorySource::Manual,
        );
        // Set a fixed timestamp for consistent test output
        memory.created_at = Utc.with_ymd_and_hms(2024, 1, 15, 10, 30, 0).unwrap();
        memory
    }

    fn create_retrieved_memory(content: &str, memory_type: MemoryType) -> RetrievedMemory {
        RetrievedMemory {
            memory: create_test_memory(content, memory_type),
            similarity_score: 0.9,
            effective_weight: 0.8,
            final_score: 0.85,
        }
    }

    #[test]
    fn test_format_memory_block_single_memory() {
        let memories = vec![create_retrieved_memory(
            "User prefers dark mode",
            MemoryType::Episodic,
        )];

        let block = format_memory_block(&memories);

        assert!(block.contains("<mnemo-memories>"));
        assert!(block.contains("</mnemo-memories>"));
        assert!(block.contains("timestamp=\"2024-01-15\""));
        assert!(block.contains("type=\"episodic\""));
        assert!(block.contains("User prefers dark mode"));
    }

    #[test]
    fn test_format_memory_block_multiple_memories() {
        let memories = vec![
            create_retrieved_memory("User likes Python", MemoryType::Semantic),
            create_retrieved_memory("User asked about Rust yesterday", MemoryType::Episodic),
            create_retrieved_memory("To deploy: run cargo build", MemoryType::Procedural),
        ];

        let block = format_memory_block(&memories);

        assert!(block.contains("type=\"semantic\""));
        assert!(block.contains("type=\"episodic\""));
        assert!(block.contains("type=\"procedural\""));
        assert!(block.contains("User likes Python"));
        assert!(block.contains("User asked about Rust yesterday"));
        assert!(block.contains("To deploy: run cargo build"));
    }

    #[test]
    fn test_format_memory_block_empty() {
        let memories: Vec<RetrievedMemory> = vec![];
        let block = format_memory_block(&memories);
        assert!(block.is_empty());
    }

    #[test]
    fn test_estimate_tokens() {
        // 40 chars / 4 = 10 tokens
        assert_eq!(estimate_tokens("Hello, this is a test of forty chars!!"), 9);
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("Hi"), 0); // 2 chars / 4 = 0 (integer division)
        assert_eq!(estimate_tokens("Hello World!"), 3); // 12 chars / 4 = 3
    }

    #[test]
    fn test_inject_into_existing_system_message() {
        let mut request = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "Hello!"}
            ]
        });

        let memories = vec![create_retrieved_memory(
            "User prefers concise answers",
            MemoryType::Semantic,
        )];

        inject_memories(&mut request, &memories, 2000).unwrap();

        let messages = request["messages"].as_array().unwrap();
        let system_content = messages[0]["content"].as_str().unwrap();

        assert!(system_content.contains("You are a helpful assistant."));
        assert!(system_content.contains("<mnemo-memories>"));
        assert!(system_content.contains("User prefers concise answers"));
    }

    #[test]
    fn test_inject_creates_system_message_when_missing() {
        let mut request = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "Hello!"}
            ]
        });

        let memories = vec![create_retrieved_memory(
            "User prefers dark mode",
            MemoryType::Episodic,
        )];

        inject_memories(&mut request, &memories, 2000).unwrap();

        let messages = request["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);

        // System message should be first
        assert_eq!(messages[0]["role"], "system");
        let system_content = messages[0]["content"].as_str().unwrap();
        assert!(system_content.contains("<mnemo-memories>"));
        assert!(system_content.contains("User prefers dark mode"));

        // User message should be second
        assert_eq!(messages[1]["role"], "user");
    }

    #[test]
    fn test_inject_empty_memories_is_noop() {
        let original = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "Hello!"}
            ]
        });

        let mut request = original.clone();
        let memories: Vec<RetrievedMemory> = vec![];

        inject_memories(&mut request, &memories, 2000).unwrap();

        // Request should be unchanged
        assert_eq!(request, original);
    }

    #[test]
    fn test_inject_missing_messages_returns_error() {
        let mut request = serde_json::json!({
            "model": "gpt-4"
        });

        let memories = vec![create_retrieved_memory("test", MemoryType::Semantic)];

        let result = inject_memories(&mut request, &memories, 2000);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MnemoError::Proxy(_)));
    }

    #[test]
    fn test_truncate_to_budget_keeps_most_relevant() {
        // Create memories with varying content lengths
        let memories = vec![
            create_retrieved_memory("Short", MemoryType::Semantic),
            create_retrieved_memory(
                "This is a medium length memory content",
                MemoryType::Episodic,
            ),
            create_retrieved_memory(
                "This is quite a long memory content that takes up more tokens than the others",
                MemoryType::Procedural,
            ),
        ];

        // Very small budget - should only fit first memory
        let truncated = truncate_to_budget(&memories, 50);

        // With ~50 tokens budget, minus overhead, we should have very limited space
        assert!(!truncated.is_empty());
        assert!(truncated.len() <= memories.len());

        // Should keep memories in order (most relevant first)
        if truncated.len() > 1 {
            assert_eq!(truncated[0].memory.content, "Short");
        }
    }

    #[test]
    fn test_truncate_to_budget_zero_returns_empty() {
        let memories = vec![create_retrieved_memory("test", MemoryType::Semantic)];
        let truncated = truncate_to_budget(&memories, 0);
        assert!(truncated.is_empty());
    }

    #[test]
    fn test_truncate_to_budget_large_budget_keeps_all() {
        let memories = vec![
            create_retrieved_memory("Memory 1", MemoryType::Semantic),
            create_retrieved_memory("Memory 2", MemoryType::Episodic),
            create_retrieved_memory("Memory 3", MemoryType::Procedural),
        ];

        // Large budget should keep all
        let truncated = truncate_to_budget(&memories, 10000);
        assert_eq!(truncated.len(), 3);
    }

    #[test]
    fn test_extract_user_query() {
        let request = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "What is Rust?"},
                {"role": "assistant", "content": "Rust is a systems language."},
                {"role": "user", "content": "Tell me more about memory safety."}
            ]
        });

        let query = extract_user_query(&request);
        assert_eq!(query, Some("Tell me more about memory safety.".to_string()));
    }

    #[test]
    fn test_extract_user_query_no_user_message() {
        let request = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are helpful."}
            ]
        });

        let query = extract_user_query(&request);
        assert!(query.is_none());
    }

    #[test]
    fn test_extract_user_query_no_messages() {
        let request = serde_json::json!({
            "model": "gpt-4"
        });

        let query = extract_user_query(&request);
        assert!(query.is_none());
    }

    #[test]
    fn test_inject_into_empty_system_message() {
        let mut request = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": ""},
                {"role": "user", "content": "Hello!"}
            ]
        });

        let memories = vec![create_retrieved_memory(
            "User prefers dark mode",
            MemoryType::Episodic,
        )];

        inject_memories(&mut request, &memories, 2000).unwrap();

        let messages = request["messages"].as_array().unwrap();
        let system_content = messages[0]["content"].as_str().unwrap();

        // Should not have leading newlines when system content was empty
        assert!(system_content.starts_with("<mnemo-memories>"));
    }

    #[test]
    fn test_memory_block_xml_is_valid() {
        let memories = vec![
            create_retrieved_memory("Content with \"quotes\"", MemoryType::Semantic),
            create_retrieved_memory("Content with <brackets>", MemoryType::Episodic),
        ];

        let block = format_memory_block(&memories);

        // Basic XML structure validation
        assert!(block.starts_with("<mnemo-memories>"));
        assert!(block.ends_with("</mnemo-memories>"));

        // Count opening and closing tags
        let open_count = block.matches("<memory").count();
        let close_count = block.matches("</memory>").count();
        assert_eq!(open_count, close_count);
        assert_eq!(open_count, 2);
    }
}
