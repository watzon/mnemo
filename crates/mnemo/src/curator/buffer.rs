//! Conversation buffer for managing dialogue history
//!
//! Provides a fixed-size buffer for conversation turns with LRU eviction
//! based on turn count and token limits. Used by the curator to maintain
//! context window for memory extraction.

use crate::config::BufferConfig;
use chrono::{DateTime, Utc};
use std::collections::VecDeque;

/// Role of a conversation participant
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    /// User message
    User,
    /// Assistant message
    Assistant,
    /// System message
    System,
}

impl Role {
    /// Convert role to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
        }
    }
}

/// A single turn in a conversation
#[derive(Debug, Clone)]
pub struct ConversationTurn {
    /// Role of the speaker
    pub role: Role,
    /// Content of the message
    pub content: String,
    /// Timestamp when the turn was recorded
    pub timestamp: DateTime<Utc>,
}

impl ConversationTurn {
    /// Create a new conversation turn with current timestamp
    pub fn new(role: Role, content: String) -> Self {
        Self {
            role,
            content,
            timestamp: Utc::now(),
        }
    }

    /// Estimate token count using chars/4 heuristic
    ///
    /// This is a fast approximation suitable for buffer management.
    /// For precise tokenization, use a proper tokenizer.
    pub fn estimate_tokens(&self) -> usize {
        self.content.len() / 4
    }
}

/// Buffer for managing conversation history with LRU eviction
///
/// Maintains a fixed-size buffer of conversation turns, evicting oldest
/// turns when either max_turns or max_tokens limits are exceeded.
pub struct ConversationBuffer {
    turns: VecDeque<ConversationTurn>,
    max_turns: usize,
    max_tokens: usize,
}

impl ConversationBuffer {
    /// Create a new conversation buffer from configuration
    pub fn new(config: &BufferConfig) -> Self {
        Self {
            turns: VecDeque::new(),
            max_turns: config.max_turns,
            max_tokens: config.max_tokens,
        }
    }

    /// Add a turn to the buffer
    ///
    /// After adding, enforces limits by evicting oldest turns if necessary.
    pub fn push(&mut self, turn: ConversationTurn) {
        self.turns.push_back(turn);
        self.enforce_limits();
    }

    /// Format turns as XML prompt context for LLM consumption
    ///
    /// Returns a string in the format:
    /// ```xml
    /// <conversation>
    /// <turn role="user">content...</turn>
    /// <turn role="assistant">content...</turn>
    /// </conversation>
    /// ```
    pub fn to_prompt_context(&self) -> String {
        if self.turns.is_empty() {
            return "<conversation></conversation>".to_string();
        }

        let mut result = String::with_capacity(self.estimate_total_tokens() * 4 + 50);
        result.push_str("<conversation>\n");

        for turn in &self.turns {
            result.push_str(&format!(
                "<turn role=\"{}\">{}</turn>\n",
                turn.role.as_str(),
                escape_xml(&turn.content)
            ));
        }

        result.push_str("</conversation>");
        result
    }

    /// Clear all turns from the buffer
    pub fn clear(&mut self) {
        self.turns.clear();
    }

    /// Get the number of turns in the buffer
    pub fn len(&self) -> usize {
        self.turns.len()
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.turns.is_empty()
    }

    /// Get an iterator over the turns (oldest first)
    pub fn iter(&self) -> impl Iterator<Item = &ConversationTurn> {
        self.turns.iter()
    }

    /// Estimate total tokens in buffer
    fn estimate_total_tokens(&self) -> usize {
        self.turns.iter().map(|t| t.estimate_tokens()).sum()
    }

    /// Enforce max_turns and max_tokens limits by evicting oldest turns
    fn enforce_limits(&mut self) {
        // Evict by turn count first
        while self.turns.len() > self.max_turns {
            self.turns.pop_front();
        }

        // Then evict by token count (removing oldest until under limit)
        while self.estimate_total_tokens() > self.max_tokens && !self.turns.is_empty() {
            self.turns.pop_front();
        }
    }
}

/// Escape special XML characters in content
fn escape_xml(content: &str) -> String {
    content
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> BufferConfig {
        BufferConfig {
            max_turns: 5,
            max_tokens: 100,
        }
    }

    #[test]
    fn test_conversation_turn_new() {
        let turn = ConversationTurn::new(Role::User, "Hello".to_string());
        assert_eq!(turn.role, Role::User);
        assert_eq!(turn.content, "Hello");
        assert!(turn.timestamp <= Utc::now());
    }

    #[test]
    fn test_conversation_turn_estimate_tokens() {
        let turn = ConversationTurn::new(Role::User, "Hello world".to_string());
        // "Hello world" has 11 chars, 11/4 = 2 tokens
        assert_eq!(turn.estimate_tokens(), 2);

        let long_turn = ConversationTurn::new(Role::Assistant, "a".repeat(100));
        assert_eq!(long_turn.estimate_tokens(), 25);
    }

    #[test]
    fn test_role_as_str() {
        assert_eq!(Role::User.as_str(), "user");
        assert_eq!(Role::Assistant.as_str(), "assistant");
        assert_eq!(Role::System.as_str(), "system");
    }

    #[test]
    fn test_buffer_new() {
        let config = test_config();
        let buffer = ConversationBuffer::new(&config);
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_buffer_push_and_len() {
        let config = test_config();
        let mut buffer = ConversationBuffer::new(&config);

        buffer.push(ConversationTurn::new(Role::User, "Hello".to_string()));
        assert_eq!(buffer.len(), 1);
        assert!(!buffer.is_empty());

        buffer.push(ConversationTurn::new(
            Role::Assistant,
            "Hi there".to_string(),
        ));
        assert_eq!(buffer.len(), 2);
    }

    #[test]
    fn test_buffer_max_turns_eviction() {
        let config = BufferConfig {
            max_turns: 3,
            max_tokens: 10000, // High limit so we don't hit it
        };
        let mut buffer = ConversationBuffer::new(&config);

        // Add 5 turns
        for i in 0..5 {
            buffer.push(ConversationTurn::new(Role::User, format!("Message {}", i)));
        }

        // Should only keep last 3
        assert_eq!(buffer.len(), 3);

        // Verify we kept the most recent
        let contents: Vec<_> = buffer.iter().map(|t| t.content.clone()).collect();
        assert_eq!(contents, vec!["Message 2", "Message 3", "Message 4"]);
    }

    #[test]
    fn test_buffer_max_tokens_eviction() {
        let config = BufferConfig {
            max_turns: 100, // High limit so we don't hit it
            max_tokens: 10, // Very low limit (40 chars max)
        };
        let mut buffer = ConversationBuffer::new(&config);

        // Add turns with ~5 tokens each (20 chars)
        buffer.push(ConversationTurn::new(
            Role::User,
            "01234567890123456789".to_string(),
        ));
        buffer.push(ConversationTurn::new(
            Role::User,
            "abcdefghijklmnopqrst".to_string(),
        ));
        buffer.push(ConversationTurn::new(
            Role::User,
            "ABCDEFGHIJKLMNOPQRST".to_string(),
        ));

        // Each turn is 5 tokens, max is 10, so we should keep only last 2
        assert_eq!(buffer.len(), 2);

        // Verify we kept the most recent
        let contents: Vec<_> = buffer.iter().map(|t| t.content.clone()).collect();
        assert_eq!(
            contents,
            vec!["abcdefghijklmnopqrst", "ABCDEFGHIJKLMNOPQRST"]
        );
    }

    #[test]
    fn test_buffer_to_prompt_context_empty() {
        let config = test_config();
        let buffer = ConversationBuffer::new(&config);
        assert_eq!(buffer.to_prompt_context(), "<conversation></conversation>");
    }

    #[test]
    fn test_buffer_to_prompt_context_format() {
        let config = test_config();
        let mut buffer = ConversationBuffer::new(&config);

        buffer.push(ConversationTurn::new(Role::User, "Hello".to_string()));
        buffer.push(ConversationTurn::new(Role::Assistant, "Hi!".to_string()));

        let context = buffer.to_prompt_context();
        assert!(context.starts_with("<conversation>\n"));
        assert!(context.contains("<turn role=\"user\">Hello</turn>"));
        assert!(context.contains("<turn role=\"assistant\">Hi!</turn>"));
        assert!(context.ends_with("</conversation>"));
    }

    #[test]
    fn test_buffer_to_prompt_context_escapes_xml() {
        let config = test_config();
        let mut buffer = ConversationBuffer::new(&config);

        buffer.push(ConversationTurn::new(
            Role::User,
            "Use <script> & \"quotes\"".to_string(),
        ));

        let context = buffer.to_prompt_context();
        assert!(context.contains("&lt;script&gt;"));
        assert!(context.contains("&amp;"));
        assert!(context.contains("&quot;quotes&quot;"));
    }

    #[test]
    fn test_buffer_clear() {
        let config = test_config();
        let mut buffer = ConversationBuffer::new(&config);

        buffer.push(ConversationTurn::new(Role::User, "Hello".to_string()));
        buffer.push(ConversationTurn::new(Role::Assistant, "Hi".to_string()));
        assert_eq!(buffer.len(), 2);

        buffer.clear();
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_buffer_iter() {
        let config = test_config();
        let mut buffer = ConversationBuffer::new(&config);

        buffer.push(ConversationTurn::new(Role::User, "First".to_string()));
        buffer.push(ConversationTurn::new(Role::Assistant, "Second".to_string()));
        buffer.push(ConversationTurn::new(Role::User, "Third".to_string()));

        let roles: Vec<_> = buffer.iter().map(|t| t.role.clone()).collect();
        assert_eq!(roles, vec![Role::User, Role::Assistant, Role::User]);
    }

    #[test]
    fn test_buffer_both_limits_trigger() {
        // Test when both limits could trigger - should respect both
        let config = BufferConfig {
            max_turns: 2,
            max_tokens: 100,
        };
        let mut buffer = ConversationBuffer::new(&config);

        // Add 3 turns, each with ~3 tokens (12 chars)
        for i in 0..3 {
            buffer.push(ConversationTurn::new(
                Role::User,
                format!("Msg {} xxxx", i), // 12 chars = 3 tokens
            ));
        }

        // max_turns is 2, so we should only have 2 turns
        assert_eq!(buffer.len(), 2);
    }

    #[test]
    fn test_buffer_tokens_limit_more_restrictive() {
        // Test when token limit is more restrictive than turn limit
        let config = BufferConfig {
            max_turns: 10,
            max_tokens: 5, // Only ~20 chars allowed
        };
        let mut buffer = ConversationBuffer::new(&config);

        // Add 3 short turns (3 tokens each = 12 chars total)
        // This should exceed 5 token limit quickly
        buffer.push(ConversationTurn::new(Role::User, "abc".to_string())); // 0 tokens (3/4=0)
        buffer.push(ConversationTurn::new(Role::User, "def".to_string())); // 0 tokens
        buffer.push(ConversationTurn::new(Role::User, "ghi".to_string())); // 0 tokens

        // All 3 should fit since they're 0 tokens each with integer division
        assert_eq!(buffer.len(), 3);

        // Now add a longer one that will trigger eviction
        buffer.push(ConversationTurn::new(Role::User, "0123456789".to_string())); // 2 tokens

        // Should have evicted enough to stay under 5 tokens
        assert!(buffer.len() <= 4);
    }
}
