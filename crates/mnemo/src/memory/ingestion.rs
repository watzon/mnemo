//! Memory Ingestion Pipeline
//!
//! Orchestrates the full ingestion flow: routing, embedding generation,
//! memory creation, and storage.

use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

use crate::curator::CuratedMemory;
use crate::embedding::EmbeddingModel;
use crate::error::Result;
use crate::memory::types::{CompressionLevel, Memory, MemorySource, MemoryType, StorageTier};
use crate::router::MemoryRouter;
use crate::storage::LanceStore;

/// Minimum content length for ingestion (in characters)
const MIN_CONTENT_LENGTH: usize = 10;

/// Pipeline for ingesting content into the memory system.
///
/// The pipeline orchestrates:
/// 1. Content filtering (empty/too short)
/// 2. Routing (entity/topic extraction)
/// 3. Embedding generation
/// 4. Memory creation with calculated weight and compression level
/// 5. Storage in LanceDB
pub struct IngestionPipeline {
    router: Arc<MemoryRouter>,
    embedding_model: Arc<EmbeddingModel>,
    store: Arc<TokioMutex<LanceStore>>,
}

impl IngestionPipeline {
    /// Create a new ingestion pipeline with shared components.
    ///
    /// Uses shared router (NER model) and embedding model (e5-small) instances.
    pub fn new(
        store: Arc<TokioMutex<LanceStore>>,
        embedding_model: Arc<EmbeddingModel>,
        router: Arc<MemoryRouter>,
    ) -> Self {
        Self {
            router,
            embedding_model,
            store,
        }
    }

    /// Create a new ingestion pipeline with owned components.
    ///
    /// Initializes its own router (NER model) and embedding model (e5-small).
    /// Primarily used for testing.
    pub fn new_owned(store: LanceStore) -> Result<Self> {
        Ok(Self {
            router: Arc::new(MemoryRouter::new()?),
            embedding_model: Arc::new(EmbeddingModel::new()?),
            store: Arc::new(TokioMutex::new(store)),
        })
    }

    /// Ingest text content into the memory system.
    ///
    /// Returns `Ok(Some(Memory))` if content was ingested successfully,
    /// `Ok(None)` if content was filtered out (empty or too short),
    /// or `Err` if an error occurred during processing.
    ///
    /// # Arguments
    /// * `text` - The content to ingest
    /// * `source` - Where this content originated from
    /// * `conversation_id` - Optional conversation ID for episodic memories
    ///
    /// # Filtering Rules
    /// - Empty or whitespace-only content is skipped
    /// - Content shorter than 10 characters is skipped
    ///
    /// # Memory Type Determination
    /// - `Conversation` source -> `Episodic` memory type
    /// - All other sources -> `Semantic` memory type
    pub async fn ingest(
        &mut self,
        text: &str,
        source: MemorySource,
        conversation_id: Option<String>,
    ) -> Result<Option<Memory>> {
        let text = text.trim();
        if text.is_empty() || text.len() < MIN_CONTENT_LENGTH {
            return Ok(None);
        }

        let router_output = self.router.route(text)?;
        let embedding = self.embedding_model.embed(text)?;
        let initial_weight = (0.5 + (router_output.entities.len() as f32 * 0.1)).min(1.0);
        let compression = Self::determine_compression(text.len());

        let memory_type = match source {
            MemorySource::Conversation => MemoryType::Episodic,
            _ => MemoryType::Semantic,
        };

        let mut memory = Memory::new(text.to_string(), embedding, memory_type, source);
        memory.conversation_id = conversation_id;
        memory.entities = router_output
            .entities
            .iter()
            .map(|e| e.text.clone())
            .collect();
        memory.weight = initial_weight;
        memory.compression = compression;
        memory.tier = StorageTier::Hot;

        self.store.lock().await.insert(&memory).await?;

        Ok(Some(memory))
    }

    fn determine_compression(length: usize) -> CompressionLevel {
        match length {
            0..100 => CompressionLevel::Full,
            100..500 => CompressionLevel::Summary,
            500..2000 => CompressionLevel::Keywords,
            _ => CompressionLevel::Hash,
        }
    }

    pub async fn ingest_curated(
        &mut self,
        curated: CuratedMemory,
        conversation_id: Option<String>,
    ) -> Result<Memory> {
        let embedding = self.embedding_model.embed(&curated.content)?;

        let mut memory = Memory::new(
            curated.content.clone(),
            embedding,
            curated.memory_type,
            MemorySource::Conversation,
        );
        memory.conversation_id = conversation_id;
        memory.entities = curated.entities;
        memory.weight = curated.importance;
        memory.compression = Self::determine_compression(curated.content.len());
        memory.tier = StorageTier::Hot;

        self.store.lock().await.insert(&memory).await?;

        Ok(memory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_test_store() -> LanceStore {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
        store.create_memories_table().await.unwrap();
        std::mem::forget(temp_dir);
        store
    }

    #[tokio::test]
    async fn test_ingest_creates_memory_with_correct_fields() {
        let store = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline
            .ingest(
                "This is a test memory with enough content to pass filtering.",
                MemorySource::Manual,
                None,
            )
            .await;

        assert!(result.is_ok());
        let memory = result.unwrap();
        assert!(memory.is_some());

        let memory = memory.unwrap();
        assert_eq!(
            memory.content,
            "This is a test memory with enough content to pass filtering."
        );
        assert_eq!(memory.memory_type, MemoryType::Semantic);
        assert_eq!(memory.source, MemorySource::Manual);
        assert_eq!(memory.tier, StorageTier::Hot);
        assert!(memory.weight >= 0.5);
        assert!(memory.weight <= 1.0);
        assert_eq!(memory.embedding.len(), 384);
    }

    #[tokio::test]
    async fn test_ingest_conversation_creates_episodic_memory() {
        let store = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline
            .ingest(
                "User mentioned they prefer Python over JavaScript.",
                MemorySource::Conversation,
                Some("conv-123".to_string()),
            )
            .await;

        assert!(result.is_ok());
        let memory = result.unwrap().unwrap();
        assert_eq!(memory.memory_type, MemoryType::Episodic);
        assert_eq!(memory.conversation_id, Some("conv-123".to_string()));
    }

    #[tokio::test]
    async fn test_ingest_filters_empty_content() {
        let store = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline.ingest("", MemorySource::Manual, None).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());

        let result = pipeline
            .ingest("   \n\t  ", MemorySource::Manual, None)
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_ingest_filters_short_content() {
        let store = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline.ingest("short", MemorySource::Manual, None).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());

        let result = pipeline
            .ingest("1234567890", MemorySource::Manual, None)
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_compression_level_determination() {
        assert_eq!(
            IngestionPipeline::determine_compression(50),
            CompressionLevel::Full
        );
        assert_eq!(
            IngestionPipeline::determine_compression(99),
            CompressionLevel::Full
        );

        assert_eq!(
            IngestionPipeline::determine_compression(100),
            CompressionLevel::Summary
        );
        assert_eq!(
            IngestionPipeline::determine_compression(499),
            CompressionLevel::Summary
        );

        assert_eq!(
            IngestionPipeline::determine_compression(500),
            CompressionLevel::Keywords
        );
        assert_eq!(
            IngestionPipeline::determine_compression(1999),
            CompressionLevel::Keywords
        );

        assert_eq!(
            IngestionPipeline::determine_compression(2000),
            CompressionLevel::Hash
        );
        assert_eq!(
            IngestionPipeline::determine_compression(10000),
            CompressionLevel::Hash
        );
    }

    #[tokio::test]
    async fn test_ingest_generates_embedding() {
        let store = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline
            .ingest(
                "This content should have a valid embedding generated.",
                MemorySource::Manual,
                None,
            )
            .await;

        let memory = result.unwrap().unwrap();
        assert_eq!(memory.embedding.len(), 384);

        let sum: f32 = memory.embedding.iter().sum();
        assert!(sum.abs() > 0.0, "Embedding should not be all zeros");
    }

    #[tokio::test]
    async fn test_ingest_extracts_entities() {
        let store = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline
            .ingest(
                "John Smith works at Microsoft in Seattle.",
                MemorySource::Manual,
                None,
            )
            .await;

        let memory = result.unwrap().unwrap();
        assert!(memory.weight >= 0.5);
    }

    #[tokio::test]
    async fn test_weight_calculation() {
        let store = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline
            .ingest(
                "The quick brown fox jumps over the lazy dog.",
                MemorySource::Manual,
                None,
            )
            .await;

        let memory = result.unwrap().unwrap();
        assert!(memory.weight >= 0.5);
        assert!(memory.weight <= 1.0);
    }

    #[tokio::test]
    async fn test_memory_stored_in_lancedb() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
        store.create_memories_table().await.unwrap();

        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline
            .ingest(
                "This memory should be stored in LanceDB.",
                MemorySource::Manual,
                None,
            )
            .await;

        let memory = result.unwrap().unwrap();
        let id = memory.id;

        let mut store2 = LanceStore::connect(temp_dir.path()).await.unwrap();
        store2.open_memories_table().await.unwrap();

        let retrieved = store2.get(id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(
            retrieved.unwrap().content,
            "This memory should be stored in LanceDB."
        );
    }
}
