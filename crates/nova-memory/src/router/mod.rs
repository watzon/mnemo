//! Memory Router - Analyzes text to determine routing and extract metadata
//!
//! This module provides the `MemoryRouter` which analyzes input text to:
//! - Extract named entities (people, organizations, locations)
//! - Identify topics from entities and noun phrases
//! - Analyze emotional valence using keyword heuristics
//! - Generate query keys for memory retrieval
//! - Determine which memory types to search

pub mod ner;

pub use ner::{Entity, EntityLabel, NerModel};

use crate::memory::types::MemoryType;
use crate::NovaError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Maximum text length for NER processing (in characters).
/// DistilBERT has a 512 token limit; ~2000 chars is a safe approximation
/// that accounts for multi-byte UTF-8 characters and prevents index errors.
const MAX_NER_TEXT_LENGTH: usize = 2000;

/// Output from the memory router containing extracted metadata and routing information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterOutput {
    /// Extracted topics from the text (from entities + noun-like patterns)
    pub topics: Vec<String>,
    /// Named entities extracted via NER
    pub entities: Vec<Entity>,
    /// Emotional valence score from -1.0 (negative) to 1.0 (positive)
    pub emotional_valence: f32,
    /// Query keys for memory retrieval (significant terms)
    pub query_keys: Vec<String>,
    /// Memory types that should be searched based on content
    pub search_types: Vec<MemoryType>,
}

impl Default for RouterOutput {
    fn default() -> Self {
        Self {
            topics: Vec::new(),
            entities: Vec::new(),
            emotional_valence: 0.0,
            query_keys: Vec::new(),
            search_types: vec![MemoryType::Episodic, MemoryType::Semantic],
        }
    }
}

/// Memory router that analyzes text for routing decisions and metadata extraction
pub struct MemoryRouter {
    ner_model: NerModel,
}

impl MemoryRouter {
    /// Create a new memory router with the NER model
    pub fn new() -> Result<Self, NovaError> {
        Ok(Self {
            ner_model: NerModel::new()?,
        })
    }

    /// Route text and extract metadata for memory operations
    ///
    /// This method:
    /// 1. Extracts named entities using NER (with text truncated to BERT token limit)
    /// 2. Extracts topics from entity names and noun patterns
    /// 3. Analyzes emotional valence using keyword heuristics
    /// 4. Generates query keys from significant terms
    /// 5. Determines which memory types to search
    pub fn route(&self, text: &str) -> Result<RouterOutput, NovaError> {
        if text.trim().is_empty() {
            return Ok(RouterOutput::default());
        }

        // Extract named entities using truncated text to respect BERT's 512 token limit
        let ner_text = self.truncate_for_ner(text);
        let entities = self.ner_model.extract_entities(&ner_text)?;

        // Extract topics from entities and noun phrases
        let topics = self.extract_topics(text, &entities);

        // Simple sentiment analysis using keyword heuristics
        let emotional_valence = self.analyze_sentiment(text);

        // Generate query keys from entities and topics
        let query_keys = self.generate_query_keys(&entities, &topics);

        // Determine which memory types to search based on content analysis
        let search_types = self.determine_search_types(text, &entities);

        Ok(RouterOutput {
            topics,
            entities,
            emotional_valence,
            query_keys,
            search_types,
        })
    }

    /// Extract topics from text using entities and simple noun-like pattern detection
    fn extract_topics(&self, text: &str, entities: &[Entity]) -> Vec<String> {
        let mut topics: HashSet<String> = HashSet::new();

        // Add entity text as topics (normalized to lowercase)
        for entity in entities {
            let normalized = entity.text.to_lowercase();
            if normalized.len() >= 2 {
                topics.insert(normalized);
            }
        }

        // Extract capitalized noun phrases (simple heuristic)
        // Words starting with uppercase that aren't at the start of sentences
        let words: Vec<&str> = text.split_whitespace().collect();
        for (i, word) in words.iter().enumerate() {
            // Skip first word of text (sentence start)
            if i == 0 {
                continue;
            }

            // Check if previous char was sentence-ending punctuation
            if i > 0 {
                let prev = words[i - 1];
                if prev.ends_with('.') || prev.ends_with('!') || prev.ends_with('?') {
                    continue;
                }
            }

            // Check for capitalized words that might be proper nouns/topics
            let clean_word = word.trim_matches(|c: char| !c.is_alphanumeric());
            if !clean_word.is_empty()
                && clean_word
                    .chars()
                    .next()
                    .map(|c| c.is_uppercase())
                    .unwrap_or(false)
                && clean_word.len() >= 3
            {
                // Don't add if it's already an entity (avoid duplicates)
                let lower = clean_word.to_lowercase();
                if !entities
                    .iter()
                    .any(|e| e.text.to_lowercase().contains(&lower))
                {
                    topics.insert(lower);
                }
            }
        }

        // Extract significant noun-like words (simple pattern: longer words, not stopwords)
        let stopwords = Self::get_stopwords();
        for word in text.split_whitespace() {
            let clean = word
                .to_lowercase()
                .chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>();

            if clean.len() >= 5 && !stopwords.contains(&clean.as_str()) {
                topics.insert(clean);
            }
        }

        topics.into_iter().collect()
    }

    /// Analyze emotional valence using simple keyword heuristics
    /// Returns a score from -1.0 (very negative) to 1.0 (very positive)
    fn analyze_sentiment(&self, text: &str) -> f32 {
        let text_lower = text.to_lowercase();

        let positive_words = [
            "love",
            "great",
            "excellent",
            "happy",
            "good",
            "best",
            "wonderful",
            "amazing",
            "fantastic",
            "beautiful",
            "awesome",
            "perfect",
            "enjoy",
            "pleased",
            "delighted",
            "excited",
            "glad",
            "thankful",
            "grateful",
            "brilliant",
            "outstanding",
            "superb",
            "terrific",
            "marvelous",
            "joyful",
        ];

        let negative_words = [
            "hate",
            "bad",
            "terrible",
            "sad",
            "worst",
            "awful",
            "horrible",
            "disgusting",
            "angry",
            "frustrated",
            "disappointed",
            "upset",
            "annoying",
            "boring",
            "poor",
            "wrong",
            "fail",
            "failed",
            "ugly",
            "stupid",
            "pathetic",
            "dreadful",
            "miserable",
            "depressed",
            "worried",
        ];

        let mut positive_count = 0;
        let mut negative_count = 0;

        for word in text_lower.split_whitespace() {
            let clean_word: String = word.chars().filter(|c| c.is_alphabetic()).collect();

            if positive_words.contains(&clean_word.as_str()) {
                positive_count += 1;
            }
            if negative_words.contains(&clean_word.as_str()) {
                negative_count += 1;
            }
        }

        let total = positive_count + negative_count;
        if total == 0 {
            return 0.0; // Neutral
        }

        // Calculate valence: (positive - negative) / total
        // This gives a range of -1.0 to 1.0
        let valence = (positive_count as f32 - negative_count as f32) / total as f32;
        valence.clamp(-1.0, 1.0)
    }

    /// Generate query keys from entities and topics for memory retrieval
    fn generate_query_keys(&self, entities: &[Entity], topics: &[String]) -> Vec<String> {
        let mut keys: HashSet<String> = HashSet::new();

        // Add entity text as query keys (higher priority)
        for entity in entities {
            let normalized = entity.text.to_lowercase();
            if normalized.len() >= 2 {
                keys.insert(normalized);
            }
        }

        // Add topics as query keys
        for topic in topics {
            if topic.len() >= 2 {
                keys.insert(topic.clone());
            }
        }

        keys.into_iter().collect()
    }

    /// Determine which memory types should be searched based on content
    fn determine_search_types(&self, text: &str, entities: &[Entity]) -> Vec<MemoryType> {
        let text_lower = text.to_lowercase();
        let mut types: Vec<MemoryType> = Vec::new();

        // Procedural indicators - questions about "how to" do things
        let procedural_patterns = [
            "how to",
            "how do",
            "how can",
            "steps to",
            "guide",
            "tutorial",
            "instructions",
            "procedure",
            "process",
            "method",
        ];

        // Semantic indicators - factual knowledge
        let semantic_patterns = [
            "what is",
            "what are",
            "define",
            "explain",
            "means",
            "meaning",
            "definition",
            "fact",
            "information",
        ];

        // Episodic indicators - events and experiences
        let episodic_patterns = [
            "remember",
            "when",
            "yesterday",
            "last time",
            "happened",
            "event",
            "meeting",
            "conversation",
            "talked about",
            "discussed",
            "told me",
        ];

        // Check for procedural content
        if procedural_patterns.iter().any(|p| text_lower.contains(p)) {
            types.push(MemoryType::Procedural);
        }

        // Check for semantic content
        if semantic_patterns.iter().any(|p| text_lower.contains(p)) {
            types.push(MemoryType::Semantic);
        }

        // Check for episodic content (events, conversations)
        if episodic_patterns.iter().any(|p| text_lower.contains(p)) {
            types.push(MemoryType::Episodic);
        }

        // Person entities suggest episodic memories
        if entities.iter().any(|e| e.label == EntityLabel::Person) {
            if !types.contains(&MemoryType::Episodic) {
                types.push(MemoryType::Episodic);
            }
        }

        // If no specific type detected, default to Episodic and Semantic
        if types.is_empty() {
            types.push(MemoryType::Episodic);
            types.push(MemoryType::Semantic);
        }

        types
    }

    /// Truncate text for NER processing at word boundaries.
    /// Returns text unchanged if under MAX_NER_TEXT_LENGTH.
    /// Otherwise truncates at the last space before the limit to avoid cutting words.
    fn truncate_for_ner(&self, text: &str) -> String {
        if text.len() <= MAX_NER_TEXT_LENGTH {
            return text.to_string();
        }

        // Find the last space before MAX_NER_TEXT_LENGTH to avoid cutting words
        let truncated = &text[..MAX_NER_TEXT_LENGTH];
        match truncated.rfind(' ') {
            Some(pos) => text[..pos].to_string(),
            None => truncated.to_string(), // No space found, hard truncate
        }
    }

    /// Get a set of common stopwords to filter out from topics
    fn get_stopwords() -> HashSet<&'static str> {
        [
            "the", "a", "an", "and", "or", "but", "is", "are", "was", "were", "be", "been",
            "being", "have", "has", "had", "do", "does", "did", "will", "would", "could", "should",
            "may", "might", "must", "shall", "can", "need", "dare", "ought", "used", "to", "of",
            "in", "for", "on", "with", "at", "by", "from", "as", "into", "through", "during",
            "before", "after", "above", "below", "between", "under", "again", "further", "then",
            "once", "here", "there", "when", "where", "why", "how", "all", "each", "few", "more",
            "most", "other", "some", "such", "no", "nor", "not", "only", "own", "same", "so",
            "than", "too", "very", "just", "also", "now", "this", "that", "these", "those", "it",
            "its", "they", "them", "their", "what", "which", "who", "whom", "whose", "i", "you",
            "he", "she", "we", "my", "your", "his", "her", "our", "about",
        ]
        .into_iter()
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_output_default() {
        let output = RouterOutput::default();
        assert!(output.topics.is_empty());
        assert!(output.entities.is_empty());
        assert_eq!(output.emotional_valence, 0.0);
        assert!(output.query_keys.is_empty());
        assert_eq!(output.search_types.len(), 2);
    }

    #[test]
    fn test_router_output_serialization() {
        let output = RouterOutput {
            topics: vec!["rust".to_string(), "programming".to_string()],
            entities: vec![],
            emotional_valence: 0.5,
            query_keys: vec!["rust".to_string()],
            search_types: vec![MemoryType::Semantic],
        };

        let json = serde_json::to_string(&output).expect("Failed to serialize");
        let deserialized: RouterOutput =
            serde_json::from_str(&json).expect("Failed to deserialize");

        assert_eq!(output.topics, deserialized.topics);
        assert_eq!(output.emotional_valence, deserialized.emotional_valence);
        assert_eq!(output.query_keys, deserialized.query_keys);
        assert_eq!(output.search_types, deserialized.search_types);
    }

    #[test]
    fn test_memory_router_creation() {
        let router = MemoryRouter::new();
        assert!(
            router.is_ok(),
            "MemoryRouter should be created successfully"
        );
    }

    #[test]
    fn test_route_empty_text() {
        let router = MemoryRouter::new().expect("Failed to create router");
        let output = router.route("").expect("Failed to route empty text");

        assert!(output.topics.is_empty());
        assert!(output.entities.is_empty());
        assert_eq!(output.emotional_valence, 0.0);
        assert!(output.query_keys.is_empty());
    }

    #[test]
    fn test_route_with_entities() {
        let router = MemoryRouter::new().expect("Failed to create router");
        let text = "John Smith works at Microsoft in Seattle.";
        let output = router.route(text).expect("Failed to route text");

        // Should have extracted some entities
        assert!(
            !output.entities.is_empty() || !output.topics.is_empty(),
            "Should extract entities or topics from text with named entities"
        );

        // Query keys should be generated
        // (may be empty if no entities were extracted, but topics might exist)
    }

    #[test]
    fn test_sentiment_positive() {
        let router = MemoryRouter::new().expect("Failed to create router");
        let text = "I love this amazing wonderful product, it's great!";
        let output = router.route(text).expect("Failed to route text");

        assert!(
            output.emotional_valence > 0.0,
            "Positive text should have positive valence, got: {}",
            output.emotional_valence
        );
    }

    #[test]
    fn test_sentiment_negative() {
        let router = MemoryRouter::new().expect("Failed to create router");
        let text = "I hate this terrible awful product, it's the worst!";
        let output = router.route(text).expect("Failed to route text");

        assert!(
            output.emotional_valence < 0.0,
            "Negative text should have negative valence, got: {}",
            output.emotional_valence
        );
    }

    #[test]
    fn test_sentiment_neutral() {
        let router = MemoryRouter::new().expect("Failed to create router");
        let text = "The weather today is cloudy.";
        let output = router.route(text).expect("Failed to route text");

        assert!(
            output.emotional_valence.abs() <= 0.5,
            "Neutral text should have near-zero valence, got: {}",
            output.emotional_valence
        );
    }

    #[test]
    fn test_search_types_procedural() {
        let router = MemoryRouter::new().expect("Failed to create router");
        let text = "How to write a function in Rust?";
        let output = router.route(text).expect("Failed to route text");

        assert!(
            output.search_types.contains(&MemoryType::Procedural),
            "How-to questions should include Procedural type"
        );
    }

    #[test]
    fn test_search_types_semantic() {
        let router = MemoryRouter::new().expect("Failed to create router");
        let text = "What is the definition of machine learning?";
        let output = router.route(text).expect("Failed to route text");

        assert!(
            output.search_types.contains(&MemoryType::Semantic),
            "Definition questions should include Semantic type"
        );
    }

    #[test]
    fn test_search_types_episodic() {
        let router = MemoryRouter::new().expect("Failed to create router");
        let text = "Remember when we discussed the project yesterday?";
        let output = router.route(text).expect("Failed to route text");

        assert!(
            output.search_types.contains(&MemoryType::Episodic),
            "Memory recall questions should include Episodic type"
        );
    }

    #[test]
    fn test_query_keys_generation() {
        let router = MemoryRouter::new().expect("Failed to create router");
        let text = "Tell me about the Rust programming language features.";
        let output = router.route(text).expect("Failed to route text");

        assert!(
            !output.query_keys.is_empty() || !output.topics.is_empty(),
            "Should generate query keys or topics from meaningful text"
        );
    }

    #[test]
    fn test_emotional_valence_range() {
        let router = MemoryRouter::new().expect("Failed to create router");

        // Test various texts
        let texts = [
            "I absolutely love this!",
            "This is terrible and awful!",
            "The cat sat on the mat.",
            "Great wonderful amazing fantastic!",
            "Bad horrible terrible worst!",
        ];

        for text in texts {
            let output = router.route(text).expect("Failed to route text");
            assert!(
                output.emotional_valence >= -1.0 && output.emotional_valence <= 1.0,
                "Emotional valence should be between -1.0 and 1.0, got: {} for text: {}",
                output.emotional_valence,
                text
            );
        }
    }
}
