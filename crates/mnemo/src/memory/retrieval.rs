//! Memory Retrieval with Weight-Based Reranking
//!
//! This module implements a two-stage retrieval pipeline:
//! 1. Vector search for candidate memories based on semantic similarity
//! 2. Reranking based on effective weight (combining base weight, recency, access patterns)

use crate::embedding::EmbeddingModel;
use crate::error::Result;
use crate::memory::types::Memory;
use crate::memory::weight::{WeightConfig, calculate_effective_weight};
use crate::storage::LanceStore;
use crate::storage::filter::MemoryFilter;

/// A retrieved memory with scoring information
#[derive(Debug, Clone)]
pub struct RetrievedMemory {
    /// The retrieved memory
    pub memory: Memory,
    /// Similarity score from vector search (cosine similarity, 0-1)
    pub similarity_score: f32,
    /// Effective weight combining base weight, recency, and access patterns
    pub effective_weight: f32,
    /// Final combined score for ranking
    pub final_score: f32,
}

impl RetrievedMemory {
    /// Create a new RetrievedMemory with computed scores
    pub fn new(
        memory: Memory,
        similarity_score: f32,
        weight_config: &WeightConfig,
        similarity_weight: f32,
        rerank_weight: f32,
    ) -> Self {
        let effective_weight = calculate_effective_weight(&memory, weight_config);
        let final_score = similarity_score * similarity_weight + effective_weight * rerank_weight;

        Self {
            memory,
            similarity_score,
            effective_weight,
            final_score,
        }
    }
}

/// Configuration for the retrieval pipeline
#[derive(Debug, Clone)]
pub struct RetrievalConfig {
    /// Weight configuration for reranking
    pub weight_config: WeightConfig,
    /// Multiplier for candidate pool size (default: 3)
    pub candidate_multiplier: usize,
    /// Weight of similarity in final score (default: 0.7)
    pub similarity_weight: f32,
    /// Weight of effective weight in final score (default: 0.3)
    pub rerank_weight: f32,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            weight_config: WeightConfig::default(),
            candidate_multiplier: 3,
            similarity_weight: 0.7,
            rerank_weight: 0.3,
        }
    }
}

/// Two-stage retrieval pipeline with weight-based reranking
///
/// Stage 1: Vector search for semantic similarity (retrieves 3x candidates)
/// Stage 2: Rerank by effective weight (combines similarity + weight factors)
pub struct RetrievalPipeline<'a> {
    store: &'a LanceStore,
    embedding_model: &'a EmbeddingModel,
    config: RetrievalConfig,
}

impl<'a> RetrievalPipeline<'a> {
    /// Create a new retrieval pipeline
    pub fn new(
        store: &'a LanceStore,
        embedding_model: &'a EmbeddingModel,
        config: RetrievalConfig,
    ) -> Self {
        Self {
            store,
            embedding_model,
            config,
        }
    }

    /// Create a pipeline with default configuration
    pub fn with_defaults(store: &'a LanceStore, embedding_model: &'a EmbeddingModel) -> Self {
        Self::new(store, embedding_model, RetrievalConfig::default())
    }

    /// Retrieve memories matching a query text
    ///
    /// Performs two-stage retrieval:
    /// 1. Generate query embedding and search for 3x limit candidates
    /// 2. Rerank candidates by effective weight
    /// 3. Return top limit results with access stats updated
    pub async fn retrieve(&mut self, query: &str, limit: usize) -> Result<Vec<RetrievedMemory>> {
        self.retrieve_filtered(query, &MemoryFilter::default(), limit)
            .await
    }

    /// Retrieve memories with filter criteria
    pub async fn retrieve_filtered(
        &mut self,
        query: &str,
        filter: &MemoryFilter,
        limit: usize,
    ) -> Result<Vec<RetrievedMemory>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let query_embedding = self.embedding_model.embed(query)?;

        let candidate_limit = limit * self.config.candidate_multiplier;
        let candidates = self
            .store
            .search_filtered(&query_embedding, filter, candidate_limit)
            .await?;

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        let mut results: Vec<RetrievedMemory> = candidates
            .into_iter()
            .map(|memory| {
                let similarity_score = cosine_similarity(&query_embedding, &memory.embedding);
                RetrievedMemory::new(
                    memory,
                    similarity_score,
                    &self.config.weight_config,
                    self.config.similarity_weight,
                    self.config.rerank_weight,
                )
            })
            .collect();

        results.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        results.truncate(limit);

        for result in &results {
            self.store.update_access(result.memory.id).await?;
        }

        Ok(results)
    }

    /// Retrieve memories using a pre-computed embedding
    pub async fn retrieve_by_embedding(
        &mut self,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<RetrievedMemory>> {
        self.retrieve_by_embedding_filtered(embedding, &MemoryFilter::default(), limit)
            .await
    }

    /// Retrieve memories using a pre-computed embedding with filter
    pub async fn retrieve_by_embedding_filtered(
        &mut self,
        embedding: &[f32],
        filter: &MemoryFilter,
        limit: usize,
    ) -> Result<Vec<RetrievedMemory>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let candidate_limit = limit * self.config.candidate_multiplier;
        let candidates = self
            .store
            .search_filtered(embedding, filter, candidate_limit)
            .await?;

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        let mut results: Vec<RetrievedMemory> = candidates
            .into_iter()
            .map(|memory| {
                let similarity_score = cosine_similarity(embedding, &memory.embedding);
                RetrievedMemory::new(
                    memory,
                    similarity_score,
                    &self.config.weight_config,
                    self.config.similarity_weight,
                    self.config.rerank_weight,
                )
            })
            .collect();

        results.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        results.truncate(limit);

        for result in &results {
            self.store.update_access(result.memory.id).await?;
        }

        Ok(results)
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    (dot / (norm_a * norm_b)).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::types::{MemorySource, MemoryType};

    fn create_test_memory(content: &str, weight: f32, access_count: u32) -> Memory {
        let mut memory = Memory::new(
            content.to_string(),
            vec![0.5; 384],
            MemoryType::Semantic,
            MemorySource::Manual,
        );
        memory.weight = weight;
        memory.access_count = access_count;
        memory
    }

    fn create_memory_with_embedding(content: &str, embedding: Vec<f32>) -> Memory {
        Memory::new(
            content.to_string(),
            embedding,
            MemoryType::Semantic,
            MemorySource::Manual,
        )
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let v1 = vec![1.0, 0.0, 0.0];
        let v2 = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&v1, &v2);
        assert!(
            (sim - 1.0).abs() < 0.001,
            "Identical vectors should have similarity ~1.0, got: {sim}"
        );
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let v1 = vec![1.0, 0.0, 0.0];
        let v2 = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&v1, &v2);
        assert!(
            sim.abs() < 0.001,
            "Orthogonal vectors should have similarity ~0.0, got: {sim}"
        );
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let v1 = vec![1.0, 0.0, 0.0];
        let v2 = vec![-1.0, 0.0, 0.0];
        let sim = cosine_similarity(&v1, &v2);
        assert!(
            (sim + 1.0).abs() < 0.001,
            "Opposite vectors should have similarity ~-1.0, got: {sim}"
        );
    }

    #[test]
    fn test_cosine_similarity_empty() {
        let v1: Vec<f32> = vec![];
        let v2: Vec<f32> = vec![];
        let sim = cosine_similarity(&v1, &v2);
        assert_eq!(sim, 0.0, "Empty vectors should have similarity 0.0");
    }

    #[test]
    fn test_cosine_similarity_mismatched_length() {
        let v1 = vec![1.0, 0.0];
        let v2 = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&v1, &v2);
        assert_eq!(sim, 0.0, "Mismatched vectors should have similarity 0.0");
    }

    #[test]
    fn test_retrieved_memory_final_score_calculation() {
        let config = WeightConfig::default();
        let memory = create_test_memory("Test", 1.0, 10);
        let similarity = 0.9;
        let similarity_weight = 0.7;
        let rerank_weight = 0.3;

        let retrieved = RetrievedMemory::new(
            memory,
            similarity,
            &config,
            similarity_weight,
            rerank_weight,
        );

        let expected_final =
            similarity * similarity_weight + retrieved.effective_weight * rerank_weight;
        assert!(
            (retrieved.final_score - expected_final).abs() < 0.001,
            "Final score should be ~{}, got: {}",
            expected_final,
            retrieved.final_score
        );
    }

    #[test]
    fn test_retrieval_config_default() {
        let config = RetrievalConfig::default();
        assert_eq!(config.candidate_multiplier, 3);
        assert_eq!(config.similarity_weight, 0.7);
        assert_eq!(config.rerank_weight, 0.3);
    }

    mod integration {
        use super::*;

        #[tokio::test]
        async fn test_retrieval_returns_sorted_by_final_score() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let base_embedding: Vec<f32> = vec![0.5; 384];

            let mut high_weight =
                create_memory_with_embedding("High weight memory", base_embedding.clone());
            high_weight.weight = 0.9;
            high_weight.access_count = 50;

            let mut low_weight =
                create_memory_with_embedding("Low weight memory", base_embedding.clone());
            low_weight.weight = 0.1;
            low_weight.access_count = 1;

            let mut medium_weight =
                create_memory_with_embedding("Medium weight memory", base_embedding.clone());
            medium_weight.weight = 0.5;
            medium_weight.access_count = 10;

            store.insert(&low_weight).await.unwrap();
            store.insert(&high_weight).await.unwrap();
            store.insert(&medium_weight).await.unwrap();

            let mut embedding_model = EmbeddingModel::new().unwrap();
            let mut pipeline = RetrievalPipeline::with_defaults(&store, &mut embedding_model);

            let results = pipeline
                .retrieve_by_embedding(&base_embedding, 10)
                .await
                .unwrap();

            assert_eq!(results.len(), 3);

            for i in 0..results.len() - 1 {
                assert!(
                    results[i].final_score >= results[i + 1].final_score,
                    "Results should be sorted by final_score descending: {} vs {}",
                    results[i].final_score,
                    results[i + 1].final_score
                );
            }

            assert!(
                results[0].memory.content.contains("High weight"),
                "High weight memory should be first, got: {}",
                results[0].memory.content
            );
        }

        #[tokio::test]
        async fn test_retrieval_updates_access_stats() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let base_embedding: Vec<f32> = vec![0.5; 384];
            let memory = create_memory_with_embedding("Access test memory", base_embedding.clone());
            let id = memory.id;
            let original_access_count = memory.access_count;

            store.insert(&memory).await.unwrap();

            let mut embedding_model = EmbeddingModel::new().unwrap();
            let mut pipeline = RetrievalPipeline::with_defaults(&store, &mut embedding_model);

            let results = pipeline
                .retrieve_by_embedding(&base_embedding, 10)
                .await
                .unwrap();
            assert_eq!(results.len(), 1);

            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(
                updated.access_count,
                original_access_count + 1,
                "Access count should be incremented after retrieval"
            );
        }

        #[tokio::test]
        async fn test_retrieval_respects_limit() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let base_embedding: Vec<f32> = vec![0.5; 384];

            for i in 0..10 {
                let memory =
                    create_memory_with_embedding(&format!("Memory {i}"), base_embedding.clone());
                store.insert(&memory).await.unwrap();
            }

            let mut embedding_model = EmbeddingModel::new().unwrap();
            let mut pipeline = RetrievalPipeline::with_defaults(&store, &mut embedding_model);

            let results = pipeline
                .retrieve_by_embedding(&base_embedding, 3)
                .await
                .unwrap();
            assert_eq!(
                results.len(),
                3,
                "Should return exactly the requested limit"
            );
        }

        #[tokio::test]
        async fn test_retrieval_empty_results() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let mut embedding_model = EmbeddingModel::new().unwrap();
            let mut pipeline = RetrievalPipeline::with_defaults(&store, &mut embedding_model);

            let results = pipeline.retrieve("test query", 10).await.unwrap();
            assert!(
                results.is_empty(),
                "Should return empty results when no memories exist"
            );
        }

        #[tokio::test]
        async fn test_retrieval_with_zero_limit() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let base_embedding: Vec<f32> = vec![0.5; 384];
            let memory = create_memory_with_embedding("Test", base_embedding.clone());
            store.insert(&memory).await.unwrap();

            let mut embedding_model = EmbeddingModel::new().unwrap();
            let mut pipeline = RetrievalPipeline::with_defaults(&store, &mut embedding_model);

            let results = pipeline
                .retrieve_by_embedding(&base_embedding, 0)
                .await
                .unwrap();
            assert!(results.is_empty(), "Zero limit should return empty results");
        }

        #[tokio::test]
        async fn test_higher_weight_ranks_higher_at_equal_similarity() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let embedding: Vec<f32> = vec![0.5; 384];

            let mut low_weight_memory =
                create_memory_with_embedding("Low weight", embedding.clone());
            low_weight_memory.weight = 0.1;

            let mut high_weight_memory =
                create_memory_with_embedding("High weight", embedding.clone());
            high_weight_memory.weight = 1.0;

            store.insert(&low_weight_memory).await.unwrap();
            store.insert(&high_weight_memory).await.unwrap();

            let mut embedding_model = EmbeddingModel::new().unwrap();
            let mut pipeline = RetrievalPipeline::with_defaults(&store, &mut embedding_model);

            let results = pipeline
                .retrieve_by_embedding(&embedding, 10)
                .await
                .unwrap();

            assert_eq!(results.len(), 2);

            let sim_diff = (results[0].similarity_score - results[1].similarity_score).abs();
            assert!(
                sim_diff < 0.001,
                "Similarity scores should be equal: {} vs {}",
                results[0].similarity_score,
                results[1].similarity_score
            );

            assert!(
                results[0].effective_weight > results[1].effective_weight,
                "Higher weight memory should have higher effective_weight"
            );
            assert!(
                results[0].final_score > results[1].final_score,
                "Higher weight memory should have higher final_score"
            );
        }
    }
}
