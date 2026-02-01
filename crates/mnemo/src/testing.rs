//! Test utilities for mnemo - shared models and mocks
//!
//! This module provides utilities to speed up test execution:
//! - Shared model instances (loaded once per test binary)
//! - Mock implementations for fast unit tests

use std::sync::LazyLock;

use crate::embedding::EmbeddingModel;
use crate::router::MemoryRouter;

/// Shared embedding model instance - loaded once per test binary.
/// Use this instead of `EmbeddingModel::new()` in tests to avoid repeated model loading.
pub static SHARED_EMBEDDING_MODEL: LazyLock<EmbeddingModel> =
    LazyLock::new(|| EmbeddingModel::new().expect("Failed to load embedding model for tests"));

/// Shared memory router instance - loaded once per test binary.
/// Use this instead of `MemoryRouter::new()` in tests to avoid repeated model loading.
pub static SHARED_MEMORY_ROUTER: LazyLock<MemoryRouter> =
    LazyLock::new(|| MemoryRouter::new().expect("Failed to load memory router for tests"));

/// Mock embedding model for fast unit tests that don't need real ML.
/// Produces deterministic 384-dimensional vectors based on input text hash.
#[derive(Debug, Clone, Default)]
pub struct MockEmbeddingModel;

impl MockEmbeddingModel {
    pub fn new() -> Self {
        Self
    }

    /// Generate a deterministic "embedding" from text using hashing.
    /// Returns a 384-dim vector (matching real model dimensions) in range [-1, 1].
    pub fn embed(&self, text: &str) -> Vec<f32> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let seed = hasher.finish();

        (0..384)
            .map(|i| {
                // Use seed + index to generate pseudo-random but deterministic values
                let x = seed
                    .wrapping_mul(i as u64 + 1)
                    .wrapping_add(0x9e3779b97f4a7c15);
                let normalized = (x as f32) / (u64::MAX as f32);
                (normalized * 2.0) - 1.0 // Range [-1, 1]
            })
            .collect()
    }

    /// Generate embeddings for multiple texts.
    pub fn embed_batch(&self, texts: &[&str]) -> Vec<Vec<f32>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_embedding_is_deterministic() {
        let model = MockEmbeddingModel::new();
        let emb1 = model.embed("hello world");
        let emb2 = model.embed("hello world");
        assert_eq!(emb1, emb2);
    }

    #[test]
    fn mock_embedding_has_correct_dimensions() {
        let model = MockEmbeddingModel::new();
        let emb = model.embed("test");
        assert_eq!(emb.len(), 384);
    }

    #[test]
    fn mock_embedding_values_in_range() {
        let model = MockEmbeddingModel::new();
        let emb = model.embed("test input");
        for val in &emb {
            assert!(*val >= -1.0 && *val <= 1.0, "Value {} out of range", val);
        }
    }

    #[test]
    fn mock_embedding_different_for_different_inputs() {
        let model = MockEmbeddingModel::new();
        let emb1 = model.embed("hello");
        let emb2 = model.embed("world");
        assert_ne!(emb1, emb2);
    }
}
