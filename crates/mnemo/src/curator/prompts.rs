//! Prompts for the curator LLM-based memory extraction
//!
//! These prompts are used by both local and remote curator providers
//! to classify conversations and extract memories.

/// Classification prompt to determine if a conversation contains memory-worthy information
///
/// Placeholder: {conversation} - the conversation text to analyze
pub const CLASSIFICATION_PROMPT: &str = r#"Analyze the following conversation and determine if it contains information worth remembering for future interactions.

Consider memory-worthy information to include:
- Facts about the user (preferences, background, goals)
- Important decisions or conclusions reached
- Technical solutions or workarounds discovered
- Project details or requirements discussed
- Personal information the user explicitly shares

Do NOT consider memory-worthy:
- Casual greetings or small talk
- General questions without specific context
- Temporary or transient information
- Information already clearly established in previous memories

Conversation:
{conversation}

Should this conversation be stored as a memory? Respond with ONLY "YES" or "NO" and a brief reason."#;

/// Extraction prompt to pull out specific memories from a conversation
///
/// Placeholder: {conversation} - the conversation text to analyze
pub const EXTRACTION_PROMPT: &str = r#"Extract specific memories from the following conversation.

For each memory you identify, provide:
1. Type: "episodic" (event/conversation), "semantic" (fact/knowledge), or "procedural" (how-to/process)
2. Content: The specific information to remember (be concise but complete)
3. Importance: Score from 0.0 to 1.0 based on how valuable this is for future interactions
4. Entities: Key nouns/entities mentioned (people, projects, technologies, etc.)

Conversation:
{conversation}

Respond with a JSON array of memories in this exact format:
[
  {
    "type": "semantic",
    "content": "User prefers dark mode interfaces",
    "importance": 0.8,
    "entities": ["dark mode", "UI preferences"]
  }
]

Only include the JSON array, no other text."#;
