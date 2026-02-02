//! Integration tests for storage layer
//!
//! Tests the LanceStore implementation with real database operations.

use mnemo_server::memory::types::{CompressionLevel, Memory, MemorySource, MemoryType, StorageTier};
use mnemo_server::storage::LanceStore;
use tempfile::tempdir;
use uuid::Uuid;

/// Test fixture: Create a test memory with given content and a dummy embedding
fn create_test_memory(content: &str) -> Memory {
    Memory::new(
        content.to_string(),
        vec![0.1; 384],
        MemoryType::Semantic,
        MemorySource::Manual,
    )
}

/// Test fixture: Create a test memory with specific embedding
fn create_memory_with_embedding(
    content: &str,
    embedding: Vec<f32>,
    memory_type: MemoryType,
) -> Memory {
    Memory::new(
        content.to_string(),
        embedding,
        memory_type,
        MemorySource::Manual,
    )
}

/// Test fixture: Create a slightly varied embedding based on a base
fn similar_embedding(base: &[f32], variation: f32) -> Vec<f32> {
    base.iter().map(|v| v + variation).collect()
}

/// Test fixture: Create a test store in a temporary directory
async fn create_test_store() -> (LanceStore, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let mut store = LanceStore::connect(dir.path()).await.unwrap();
    store.create_memories_table().await.unwrap();
    (store, dir)
}

mod insertion_tests {
    use super::*;

    #[tokio::test]
    async fn test_insert_and_retrieve_roundtrip() {
        let (store, _dir) = create_test_store().await;

        let memory = create_test_memory("Test memory content for roundtrip");
        let id = memory.id;

        store.insert(&memory).await.unwrap();

        let retrieved = store.get(id).await.unwrap();
        assert!(
            retrieved.is_some(),
            "Memory should be retrievable after insertion"
        );

        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, id);
        assert_eq!(retrieved.content, memory.content);
        assert_eq!(retrieved.embedding.len(), 384);
        assert_eq!(retrieved.memory_type, memory.memory_type);
        assert_eq!(retrieved.source, memory.source);
    }

    #[tokio::test]
    async fn test_insert_batch_multiple_memories() {
        let (store, _dir) = create_test_store().await;

        let memories: Vec<Memory> = (0..5)
            .map(|i| create_test_memory(&format!("Batch memory {i}")))
            .collect();

        let ids: Vec<Uuid> = memories.iter().map(|m| m.id).collect();

        store.insert_batch(&memories).await.unwrap();

        for (i, id) in ids.iter().enumerate() {
            let retrieved = store.get(*id).await.unwrap();
            assert!(retrieved.is_some(), "Memory {i} should be retrievable");
            assert_eq!(retrieved.unwrap().content, format!("Batch memory {i}"));
        }
    }

    #[tokio::test]
    async fn test_insert_batch_empty_does_nothing() {
        let (store, _dir) = create_test_store().await;

        let result = store.insert_batch(&[]).await;
        assert!(result.is_ok(), "Empty batch insert should succeed");
    }

    #[tokio::test]
    async fn test_retrieve_nonexistent_returns_none() {
        let (store, _dir) = create_test_store().await;

        let nonexistent_id = Uuid::new_v4();
        let result = store.get(nonexistent_id).await.unwrap();

        assert!(result.is_none(), "Nonexistent memory should return None");
    }
}

mod persistence_tests {
    use super::*;

    #[tokio::test]
    async fn test_persistence_across_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();

        // First session: create store and insert memories
        let memory_ids = {
            let mut store = LanceStore::connect(&path).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memories: Vec<Memory> = (0..3)
                .map(|i| create_test_memory(&format!("Persistent memory {i}")))
                .collect();

            let ids: Vec<Uuid> = memories.iter().map(|m| m.id).collect();
            store.insert_batch(&memories).await.unwrap();
            ids
        };

        {
            let mut store = LanceStore::connect(&path).await.unwrap();
            store.open_memories_table().await.unwrap();

            for (i, id) in memory_ids.iter().enumerate() {
                let retrieved = store.get(*id).await.unwrap();
                assert!(
                    retrieved.is_some(),
                    "Memory {i} should persist after reopen"
                );
                assert_eq!(retrieved.unwrap().content, format!("Persistent memory {i}"));
            }
        }
    }

    #[tokio::test]
    async fn test_table_exists_after_creation() {
        let dir = tempdir().unwrap();

        let mut store = LanceStore::connect(dir.path()).await.unwrap();

        assert!(!store.table_exists("memories").await.unwrap());

        store.create_memories_table().await.unwrap();
        assert!(store.table_exists("memories").await.unwrap());
    }
}

mod search_tests {
    use super::*;

    #[tokio::test]
    async fn test_vector_search_returns_relevant_results() {
        let (store, _dir) = create_test_store().await;

        let base_embedding: Vec<f32> = vec![0.5; 384];

        let memories = vec![
            create_memory_with_embedding(
                "Very similar memory",
                similar_embedding(&base_embedding, 0.01),
                MemoryType::Semantic,
            ),
            create_memory_with_embedding(
                "Somewhat similar memory",
                similar_embedding(&base_embedding, 0.05),
                MemoryType::Semantic,
            ),
            create_memory_with_embedding("Different memory", vec![0.9; 384], MemoryType::Semantic),
        ];

        store.insert_batch(&memories).await.unwrap();

        let results = store.search(&base_embedding, 10).await.unwrap();

        assert_eq!(results.len(), 3, "Should find all memories");
        assert!(
            results[0].content.contains("similar"),
            "Most similar should be ranked first"
        );
    }

    #[tokio::test]
    async fn test_search_respects_limit() {
        let (store, _dir) = create_test_store().await;

        let base_embedding: Vec<f32> = vec![0.5; 384];
        let memories: Vec<Memory> = (0..10)
            .map(|i| {
                create_memory_with_embedding(
                    &format!("Memory {i}"),
                    similar_embedding(&base_embedding, i as f32 * 0.01),
                    MemoryType::Semantic,
                )
            })
            .collect();

        store.insert_batch(&memories).await.unwrap();

        let results = store.search(&base_embedding, 3).await.unwrap();
        assert_eq!(results.len(), 3, "Should respect the limit parameter");
    }

    #[tokio::test]
    async fn test_search_returns_empty_when_no_memories() {
        let (store, _dir) = create_test_store().await;

        let query_embedding: Vec<f32> = vec![0.5; 384];
        let results = store.search(&query_embedding, 10).await.unwrap();

        assert!(
            results.is_empty(),
            "Search on empty store should return empty results"
        );
    }
}

mod update_tests {
    use super::*;

    #[tokio::test]
    async fn test_update_access_increments_count() {
        let (store, _dir) = create_test_store().await;

        let memory = create_test_memory("Access test memory");
        let id = memory.id;
        let original_count = memory.access_count;

        store.insert(&memory).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        store.update_access(id).await.unwrap();

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.access_count, original_count + 1);
        assert!(updated.last_accessed > memory.last_accessed);
    }

    #[tokio::test]
    async fn test_delete_existing_memory() {
        let (store, _dir) = create_test_store().await;

        let memory = create_test_memory("To be deleted");
        let id = memory.id;

        store.insert(&memory).await.unwrap();
        assert!(store.get(id).await.unwrap().is_some());

        let deleted = store.delete(id).await.unwrap();
        assert!(deleted, "Delete should return true for existing memory");

        assert!(
            store.get(id).await.unwrap().is_none(),
            "Memory should be gone after delete"
        );
    }

    #[tokio::test]
    async fn test_delete_nonexistent_returns_false() {
        let (store, _dir) = create_test_store().await;

        let nonexistent_id = Uuid::new_v4();
        let deleted = store.delete(nonexistent_id).await.unwrap();

        assert!(
            !deleted,
            "Delete should return false for nonexistent memory"
        );
    }
}

mod field_preservation_tests {
    use super::*;

    #[tokio::test]
    async fn test_all_fields_preserved_in_roundtrip() {
        let (store, _dir) = create_test_store().await;

        let mut memory = Memory::new(
            "Complete test memory".to_string(),
            vec![0.7; 384],
            MemoryType::Episodic,
            MemorySource::Conversation,
        );
        memory.weight = 0.85;
        memory.conversation_id = Some("test-conv-123".to_string());
        memory.tier = StorageTier::Warm;
        memory.compression = CompressionLevel::Summary;

        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let retrieved = store.get(id).await.unwrap().unwrap();

        assert_eq!(retrieved.id, memory.id);
        assert_eq!(retrieved.content, memory.content);
        assert_eq!(retrieved.memory_type, memory.memory_type);
        assert_eq!(retrieved.weight, memory.weight);
        assert_eq!(retrieved.conversation_id, memory.conversation_id);
        assert_eq!(retrieved.source, memory.source);
        assert_eq!(retrieved.tier, memory.tier);
        assert_eq!(retrieved.compression, memory.compression);
    }

    #[tokio::test]
    async fn test_different_memory_types_stored_correctly() {
        let (store, _dir) = create_test_store().await;

        let types = vec![
            (MemoryType::Episodic, "Episodic memory"),
            (MemoryType::Semantic, "Semantic memory"),
            (MemoryType::Procedural, "Procedural memory"),
        ];

        let mut ids = vec![];
        for (mem_type, content) in &types {
            let memory = create_memory_with_embedding(content, vec![0.5; 384], *mem_type);
            let id = memory.id;
            store.insert(&memory).await.unwrap();
            ids.push((id, *mem_type, content.to_string()));
        }

        for (id, expected_type, expected_content) in ids {
            let retrieved = store.get(id).await.unwrap().unwrap();
            assert_eq!(retrieved.memory_type, expected_type);
            assert_eq!(retrieved.content, expected_content);
        }
    }

    #[tokio::test]
    async fn test_different_sources_stored_correctly() {
        let (store, _dir) = create_test_store().await;

        let sources = vec![
            (MemorySource::Conversation, "Conversation memory"),
            (MemorySource::File, "File memory"),
            (MemorySource::Web, "Web memory"),
            (MemorySource::Manual, "Manual memory"),
        ];

        let mut ids = vec![];
        for (source, content) in &sources {
            let mut memory = create_test_memory(content);
            memory.source = *source;
            let id = memory.id;
            store.insert(&memory).await.unwrap();
            ids.push((id, *source));
        }

        for (id, expected_source) in ids {
            let retrieved = store.get(id).await.unwrap().unwrap();
            assert_eq!(retrieved.source, expected_source);
        }
    }
}
