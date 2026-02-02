//! Curator types for memory extraction and curation
//!
//! Defines core data structures for the curator system, including
//! extracted memories, curation results, and curator-specific errors.

use crate::memory::types::MemoryType;

/// A memory extracted by the curator
#[derive(Debug, Clone)]
pub struct CuratedMemory {
    /// Type of memory (episodic, semantic, procedural)
    pub memory_type: MemoryType,
    /// Content to store
    pub content: String,
    /// Importance score 0.0-1.0
    pub importance: f32,
    /// Extracted entities
    pub entities: Vec<String>,
    /// Optional hint about memories this supersedes
    pub supersedes_hint: Option<String>,
}

impl CuratedMemory {
    /// Create a new curated memory
    pub fn new(
        memory_type: MemoryType,
        content: String,
        importance: f32,
        entities: Vec<String>,
    ) -> Self {
        Self {
            memory_type,
            content,
            importance: importance.clamp(0.0, 1.0),
            entities,
            supersedes_hint: None,
        }
    }

    /// Set the supersedes hint
    pub fn with_supersedes_hint(mut self, hint: String) -> Self {
        self.supersedes_hint = Some(hint);
        self
    }
}

/// Result of curation analysis
#[derive(Debug, Clone)]
pub struct CurationResult {
    /// Whether this content should be stored
    pub should_store: bool,
    /// Extracted memories (empty if should_store is false)
    pub memories: Vec<CuratedMemory>,
    /// LLM's reasoning for the decision
    pub reasoning: String,
}

impl CurationResult {
    /// Create a new curation result indicating content should be stored
    pub fn should_store(memories: Vec<CuratedMemory>, reasoning: String) -> Self {
        Self {
            should_store: true,
            memories,
            reasoning,
        }
    }

    /// Create a new curation result indicating content should not be stored
    pub fn should_not_store(reasoning: String) -> Self {
        Self {
            should_store: false,
            memories: Vec::new(),
            reasoning,
        }
    }
}

/// Curator-specific errors
#[derive(Debug, thiserror::Error)]
pub enum CuratorError {
    #[error("Model not found: {0}")]
    ModelNotFound(String),
    #[error("Model loading failed: {0}")]
    ModelLoadFailed(String),
    #[error("Inference failed: {0}")]
    InferenceFailed(String),
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

/// Result type alias for curator operations
pub type Result<T> = std::result::Result<T, CuratorError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_curated_memory_new() {
        let memory = CuratedMemory::new(
            MemoryType::Semantic,
            "User prefers dark mode".to_string(),
            0.8,
            vec!["dark mode".to_string(), "preferences".to_string()],
        );

        assert_eq!(memory.memory_type, MemoryType::Semantic);
        assert_eq!(memory.content, "User prefers dark mode");
        assert_eq!(memory.importance, 0.8);
        assert_eq!(memory.entities.len(), 2);
        assert!(memory.supersedes_hint.is_none());
    }

    #[test]
    fn test_curated_memory_importance_clamping() {
        let memory_high = CuratedMemory::new(MemoryType::Episodic, "Test".to_string(), 1.5, vec![]);
        assert_eq!(memory_high.importance, 1.0);

        let memory_low =
            CuratedMemory::new(MemoryType::Procedural, "Test".to_string(), -0.5, vec![]);
        assert_eq!(memory_low.importance, 0.0);
    }

    #[test]
    fn test_curated_memory_with_supersedes_hint() {
        let memory = CuratedMemory::new(
            MemoryType::Semantic,
            "Updated preference".to_string(),
            0.9,
            vec![],
        )
        .with_supersedes_hint("old-memory-uuid".to_string());

        assert_eq!(memory.supersedes_hint, Some("old-memory-uuid".to_string()));
    }

    #[test]
    fn test_curation_result_should_store() {
        let memories = vec![CuratedMemory::new(
            MemoryType::Semantic,
            "Test".to_string(),
            0.5,
            vec![],
        )];
        let result = CurationResult::should_store(memories.clone(), "Important info".to_string());

        assert!(result.should_store);
        assert_eq!(result.memories.len(), 1);
        assert_eq!(result.reasoning, "Important info");
    }

    #[test]
    fn test_curation_result_should_not_store() {
        let result = CurationResult::should_not_store("Not relevant".to_string());

        assert!(!result.should_store);
        assert!(result.memories.is_empty());
        assert_eq!(result.reasoning, "Not relevant");
    }

    #[test]
    fn test_curator_error_display() {
        let err = CuratorError::ModelNotFound("llama-7b".to_string());
        assert_eq!(err.to_string(), "Model not found: llama-7b");

        let err = CuratorError::InferenceFailed("OOM".to_string());
        assert_eq!(err.to_string(), "Inference failed: OOM");
    }
}
