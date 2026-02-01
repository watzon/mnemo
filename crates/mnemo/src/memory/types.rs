//! Memory types for the Mnemo system
//!
//! Defines core data structures for storing and retrieving memories,
//! including the main Memory struct and supporting enums for classification.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single memory unit stored in the Mnemo system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    /// Unique identifier for this memory
    pub id: Uuid,
    /// The actual content of the memory
    pub content: String,
    /// Vector embedding (384 dimensions for e5-small)
    pub embedding: Vec<f32>,
    /// Classification of what kind of memory this is
    pub memory_type: MemoryType,
    /// Current weight (importance score)
    pub weight: f32,
    /// When this memory was created
    pub created_at: DateTime<Utc>,
    /// When this memory was last accessed
    pub last_accessed: DateTime<Utc>,
    /// How many times this memory has been accessed
    pub access_count: u32,
    /// Optional conversation ID this memory belongs to
    pub conversation_id: Option<String>,
    /// Extracted entities (names, places, etc.)
    pub entities: Vec<String>,
    /// Where this memory originated from
    pub source: MemorySource,
    /// Which storage tier this memory is in
    pub tier: StorageTier,
    /// Level of content compression
    pub compression: CompressionLevel,
}

impl Memory {
    /// Create a new memory with default values
    pub fn new(
        content: String,
        embedding: Vec<f32>,
        memory_type: MemoryType,
        source: MemorySource,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            content,
            embedding,
            memory_type,
            weight: 1.0,
            created_at: now,
            last_accessed: now,
            access_count: 0,
            conversation_id: None,
            entities: Vec::new(),
            source,
            tier: StorageTier::Hot,
            compression: CompressionLevel::Full,
        }
    }

    /// Mark this memory as accessed, updating access count and timestamp
    pub fn mark_accessed(&mut self) {
        self.access_count += 1;
        self.last_accessed = Utc::now();
    }

    /// Update the weight of this memory
    pub fn set_weight(&mut self, weight: f32) {
        self.weight = weight.clamp(0.0, 1.0);
    }
}

/// Classification of memory types based on cognitive psychology
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryType {
    /// What happened (conversations, events)
    Episodic,
    /// Facts and knowledge
    Semantic,
    /// How to do things
    Procedural,
}

/// Source of the memory - where it originated from
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemorySource {
    /// From a conversation
    Conversation,
    /// From a file
    File,
    /// From web content
    Web,
    /// Manually added
    Manual,
}

/// Storage tier indicating access frequency and retention priority
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageTier {
    /// Recently accessed, kept in memory
    Hot,
    /// Less frequently accessed, on disk
    Warm,
    /// Rarely accessed, archived
    Cold,
}

/// Level of compression applied to memory content
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionLevel {
    /// Full content preserved
    Full,
    /// Summarized content
    Summary,
    /// Only keywords preserved
    Keywords,
    /// Just a hash reference
    Hash,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_serialization() {
        let memory = Memory::new(
            "Test content".to_string(),
            vec![0.1; 384],
            MemoryType::Semantic,
            MemorySource::Manual,
        );

        let json = serde_json::to_string(&memory).expect("Failed to serialize memory");
        let deserialized: Memory =
            serde_json::from_str(&json).expect("Failed to deserialize memory");

        assert_eq!(memory.id, deserialized.id);
        assert_eq!(memory.content, deserialized.content);
        assert_eq!(memory.embedding.len(), deserialized.embedding.len());
        assert_eq!(memory.memory_type, deserialized.memory_type);
        assert_eq!(memory.source, deserialized.source);
    }

    #[test]
    fn test_memory_new_defaults() {
        let memory = Memory::new(
            "Test content".to_string(),
            vec![0.1; 384],
            MemoryType::Episodic,
            MemorySource::Conversation,
        );

        assert_eq!(memory.weight, 1.0);
        assert_eq!(memory.access_count, 0);
        assert!(memory.conversation_id.is_none());
        assert!(memory.entities.is_empty());
        assert_eq!(memory.tier, StorageTier::Hot);
        assert_eq!(memory.compression, CompressionLevel::Full);
    }

    #[test]
    fn test_memory_mark_accessed() {
        let mut memory = Memory::new(
            "Test".to_string(),
            vec![0.1; 10],
            MemoryType::Procedural,
            MemorySource::File,
        );

        let before_access = memory.last_accessed;
        memory.mark_accessed();

        assert_eq!(memory.access_count, 1);
        assert!(memory.last_accessed >= before_access);
    }

    #[test]
    fn test_memory_set_weight() {
        let mut memory = Memory::new(
            "Test".to_string(),
            vec![0.1; 10],
            MemoryType::Semantic,
            MemorySource::Web,
        );

        memory.set_weight(0.5);
        assert_eq!(memory.weight, 0.5);

        // Test clamping
        memory.set_weight(1.5);
        assert_eq!(memory.weight, 1.0);

        memory.set_weight(-0.5);
        assert_eq!(memory.weight, 0.0);
    }

    #[test]
    fn test_memory_type_serialization() {
        let types = vec![
            MemoryType::Episodic,
            MemoryType::Semantic,
            MemoryType::Procedural,
        ];

        for mem_type in types {
            let json = serde_json::to_string(&mem_type).expect("Failed to serialize");
            let deserialized: MemoryType =
                serde_json::from_str(&json).expect("Failed to deserialize");
            assert_eq!(mem_type, deserialized);
        }
    }

    #[test]
    fn test_memory_source_serialization() {
        let sources = vec![
            MemorySource::Conversation,
            MemorySource::File,
            MemorySource::Web,
            MemorySource::Manual,
        ];

        for source in sources {
            let json = serde_json::to_string(&source).expect("Failed to serialize");
            let deserialized: MemorySource =
                serde_json::from_str(&json).expect("Failed to deserialize");
            assert_eq!(source, deserialized);
        }
    }

    #[test]
    fn test_storage_tier_serialization() {
        let tiers = vec![StorageTier::Hot, StorageTier::Warm, StorageTier::Cold];

        for tier in tiers {
            let json = serde_json::to_string(&tier).expect("Failed to serialize");
            let deserialized: StorageTier =
                serde_json::from_str(&json).expect("Failed to deserialize");
            assert_eq!(tier, deserialized);
        }
    }

    #[test]
    fn test_compression_level_serialization() {
        let levels = vec![
            CompressionLevel::Full,
            CompressionLevel::Summary,
            CompressionLevel::Keywords,
            CompressionLevel::Hash,
        ];

        for level in levels {
            let json = serde_json::to_string(&level).expect("Failed to serialize");
            let deserialized: CompressionLevel =
                serde_json::from_str(&json).expect("Failed to deserialize");
            assert_eq!(level, deserialized);
        }
    }
}
