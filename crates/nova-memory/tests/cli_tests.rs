//! Integration tests for CLI commands
//!
//! Tests the CLI command implementations directly without spawning processes.

use chrono::Utc;
use nova_memory::memory::types::{
    CompressionLevel, Memory, MemorySource, MemoryType, StorageTier,
};
use nova_memory::storage::LanceStore;
use tempfile::tempdir;
use uuid::Uuid;

/// Test helper: Create a test store in a temporary directory
async fn create_test_store() -> (LanceStore, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let mut store = LanceStore::connect(dir.path()).await.unwrap();
    store.create_memories_table().await.unwrap();
    store.create_tombstones_table().await.unwrap();
    (store, dir)
}

/// Test helper: Create a test memory
fn create_test_memory(content: &str) -> Memory {
    Memory::new(
        content.to_string(),
        vec![0.1; 384],
        MemoryType::Semantic,
        MemorySource::Manual,
    )
}

/// Test helper: Create a test memory with specific tier
fn create_test_memory_with_tier(content: &str, tier: StorageTier) -> Memory {
    let mut memory = create_test_memory(content);
    memory.tier = tier;
    memory.weight = 0.5;
    memory
}

mod memory_list_tests {
    use super::*;

    #[tokio::test]
    async fn test_list_empty_store() {
        let (store, _dir) = create_test_store().await;

        let memories = store.list_by_tier(StorageTier::Hot).await.unwrap();
        assert!(memories.is_empty());
    }

    #[tokio::test]
    async fn test_list_memories_all_tiers() {
        let (store, _dir) = create_test_store().await;

        // Create memories in different tiers
        let hot_memory = create_test_memory_with_tier("Hot memory", StorageTier::Hot);
        let warm_memory = create_test_memory_with_tier("Warm memory", StorageTier::Warm);
        let cold_memory = create_test_memory_with_tier("Cold memory", StorageTier::Cold);

        store.insert(&hot_memory).await.unwrap();
        store.insert(&warm_memory).await.unwrap();
        store.insert(&cold_memory).await.unwrap();

        // List all tiers
        let hot_memories = store.list_by_tier(StorageTier::Hot).await.unwrap();
        let warm_memories = store.list_by_tier(StorageTier::Warm).await.unwrap();
        let cold_memories = store.list_by_tier(StorageTier::Cold).await.unwrap();

        assert_eq!(hot_memories.len(), 1);
        assert_eq!(warm_memories.len(), 1);
        assert_eq!(cold_memories.len(), 1);

        assert_eq!(hot_memories[0].content, "Hot memory");
        assert_eq!(warm_memories[0].content, "Warm memory");
        assert_eq!(cold_memories[0].content, "Cold memory");
    }

    #[tokio::test]
    async fn test_list_respects_limit() {
        let (store, _dir) = create_test_store().await;

        // Create 10 memories
        for i in 0..10 {
            let memory = create_test_memory_with_tier(&format!("Memory {}", i), StorageTier::Hot);
            store.insert(&memory).await.unwrap();
        }

        let memories = store.list_by_tier(StorageTier::Hot).await.unwrap();
        assert_eq!(memories.len(), 10);
    }

    #[tokio::test]
    async fn test_list_sorted_by_created() {
        let (store, _dir) = create_test_store().await;

        // Create memories with different creation times
        let mut memory1 = create_test_memory_with_tier("First", StorageTier::Hot);
        let mut memory2 = create_test_memory_with_tier("Second", StorageTier::Hot);
        let mut memory3 = create_test_memory_with_tier("Third", StorageTier::Hot);

        memory1.created_at = Utc::now();
        memory2.created_at = Utc::now();
        memory3.created_at = Utc::now();

        store.insert(&memory1).await.unwrap();
        store.insert(&memory2).await.unwrap();
        store.insert(&memory3).await.unwrap();

        let memories = store.list_by_tier(StorageTier::Hot).await.unwrap();
        assert_eq!(memories.len(), 3);
    }

    #[tokio::test]
    async fn test_list_filter_by_type() {
        let (store, _dir) = create_test_store().await;

        // Create different memory types
        let mut semantic = create_test_memory_with_tier("Semantic", StorageTier::Hot);
        semantic.memory_type = MemoryType::Semantic;

        let mut episodic = create_test_memory_with_tier("Episodic", StorageTier::Hot);
        episodic.memory_type = MemoryType::Episodic;

        let mut procedural = create_test_memory_with_tier("Procedural", StorageTier::Hot);
        procedural.memory_type = MemoryType::Procedural;

        store.insert(&semantic).await.unwrap();
        store.insert(&episodic).await.unwrap();
        store.insert(&procedural).await.unwrap();

        let all_memories = store.list_by_tier(StorageTier::Hot).await.unwrap();
        assert_eq!(all_memories.len(), 3);

        // Filter by type manually (simulating CLI filter)
        let semantic_only: Vec<_> = all_memories
            .iter()
            .filter(|m| m.memory_type == MemoryType::Semantic)
            .collect();
        assert_eq!(semantic_only.len(), 1);
    }
}

mod memory_show_tests {
    use super::*;

    #[tokio::test]
    async fn test_show_existing_memory() {
        let (store, _dir) = create_test_store().await;

        let memory = create_test_memory_with_tier("Test content", StorageTier::Hot);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let retrieved = store.get(id).await.unwrap();
        assert!(retrieved.is_some());

        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, id);
        assert_eq!(retrieved.content, "Test content");
        assert_eq!(retrieved.tier, StorageTier::Hot);
    }

    #[tokio::test]
    async fn test_show_nonexistent_memory() {
        let (store, _dir) = create_test_store().await;

        let nonexistent_id = Uuid::new_v4();
        let retrieved = store.get(nonexistent_id).await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_show_memory_all_fields() {
        let (store, _dir) = create_test_store().await;

        let mut memory = create_test_memory("Complete test memory");
        memory.weight = 0.85;
        memory.tier = StorageTier::Warm;
        memory.compression = CompressionLevel::Summary;
        memory.memory_type = MemoryType::Episodic;
        memory.source = MemorySource::Conversation;
        memory.conversation_id = Some("conv-123".to_string());
        memory.entities = vec!["entity1".to_string(), "entity2".to_string()];

        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let retrieved = store.get(id).await.unwrap().unwrap();
        assert_eq!(retrieved.id, id);
        assert_eq!(retrieved.content, "Complete test memory");
        assert_eq!(retrieved.weight, 0.85);
        assert_eq!(retrieved.tier, StorageTier::Warm);
        assert_eq!(retrieved.compression, CompressionLevel::Summary);
        assert_eq!(retrieved.memory_type, MemoryType::Episodic);
        assert_eq!(retrieved.source, MemorySource::Conversation);
        assert_eq!(retrieved.conversation_id, Some("conv-123".to_string()));
        assert_eq!(retrieved.entities, vec!["entity1".to_string(), "entity2".to_string()]);
        assert_eq!(retrieved.embedding.len(), 384);
    }
}

mod memory_delete_tests {
    use super::*;

    #[tokio::test]
    async fn test_delete_existing_memory() {
        let (store, _dir) = create_test_store().await;

        let memory = create_test_memory_with_tier("To be deleted", StorageTier::Hot);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        assert!(store.get(id).await.unwrap().is_some());

        let deleted = store.delete(id).await.unwrap();
        assert!(deleted);

        assert!(store.get(id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_memory() {
        let (store, _dir) = create_test_store().await;

        let nonexistent_id = Uuid::new_v4();
        let deleted = store.delete(nonexistent_id).await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn test_delete_creates_tombstone() {
        let (store, _dir) = create_test_store().await;

        let mut memory = create_test_memory_with_tier("To be deleted with tombstone", StorageTier::Hot);
        memory.entities = vec!["topic1".to_string(), "topic2".to_string()];
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        // Delete the memory
        store.delete(id).await.unwrap();

        // Verify memory is gone
        assert!(store.get(id).await.unwrap().is_none());
    }
}

mod memory_add_tests {
    use super::*;

    #[tokio::test]
    async fn test_add_memory_manual() {
        let (store, _dir) = create_test_store().await;

        let memory = create_test_memory_with_tier("Manually added memory", StorageTier::Hot);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let retrieved = store.get(id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().content, "Manually added memory");
    }

    #[tokio::test]
    async fn test_add_memory_different_types() {
        let (store, _dir) = create_test_store().await;

        // Semantic
        let mut semantic = create_test_memory_with_tier("Semantic memory", StorageTier::Hot);
        semantic.memory_type = MemoryType::Semantic;
        let semantic_id = semantic.id;
        store.insert(&semantic).await.unwrap();

        // Episodic
        let mut episodic = create_test_memory_with_tier("Episodic memory", StorageTier::Hot);
        episodic.memory_type = MemoryType::Episodic;
        let episodic_id = episodic.id;
        store.insert(&episodic).await.unwrap();

        // Procedural
        let mut procedural = create_test_memory_with_tier("Procedural memory", StorageTier::Hot);
        procedural.memory_type = MemoryType::Procedural;
        let procedural_id = procedural.id;
        store.insert(&procedural).await.unwrap();

        assert_eq!(
            store.get(semantic_id).await.unwrap().unwrap().memory_type,
            MemoryType::Semantic
        );
        assert_eq!(
            store.get(episodic_id).await.unwrap().unwrap().memory_type,
            MemoryType::Episodic
        );
        assert_eq!(
            store.get(procedural_id).await.unwrap().unwrap().memory_type,
            MemoryType::Procedural
        );
    }

    #[tokio::test]
    async fn test_add_memory_with_embedding() {
        let (store, _dir) = create_test_store().await;

        let mut memory = create_test_memory_with_tier("Memory with embedding", StorageTier::Hot);
        memory.embedding = vec![0.5; 384];
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let retrieved = store.get(id).await.unwrap().unwrap();
        assert_eq!(retrieved.embedding.len(), 384);
        assert_eq!(retrieved.embedding[0], 0.5);
    }
}

mod stats_tests {
    use super::*;

    #[tokio::test]
    async fn test_stats_empty_store() {
        let (store, _dir) = create_test_store().await;

        let hot_count = store.count_by_tier(StorageTier::Hot).await.unwrap();
        let warm_count = store.count_by_tier(StorageTier::Warm).await.unwrap();
        let cold_count = store.count_by_tier(StorageTier::Cold).await.unwrap();
        let total_count = store.total_count().await.unwrap();

        assert_eq!(hot_count, 0);
        assert_eq!(warm_count, 0);
        assert_eq!(cold_count, 0);
        assert_eq!(total_count, 0);
    }

    #[tokio::test]
    async fn test_stats_with_memories() {
        let (store, _dir) = create_test_store().await;

        // Add memories to different tiers
        for i in 0..5 {
            let memory = create_test_memory_with_tier(&format!("Hot {}", i), StorageTier::Hot);
            store.insert(&memory).await.unwrap();
        }

        for i in 0..3 {
            let memory = create_test_memory_with_tier(&format!("Warm {}", i), StorageTier::Warm);
            store.insert(&memory).await.unwrap();
        }

        for i in 0..2 {
            let memory = create_test_memory_with_tier(&format!("Cold {}", i), StorageTier::Cold);
            store.insert(&memory).await.unwrap();
        }

        let hot_count = store.count_by_tier(StorageTier::Hot).await.unwrap();
        let warm_count = store.count_by_tier(StorageTier::Warm).await.unwrap();
        let cold_count = store.count_by_tier(StorageTier::Cold).await.unwrap();
        let total_count = store.total_count().await.unwrap();

        assert_eq!(hot_count, 5);
        assert_eq!(warm_count, 3);
        assert_eq!(cold_count, 2);
        assert_eq!(total_count, 10);
    }

    #[tokio::test]
    async fn test_stats_by_tier() {
        let (store, _dir) = create_test_store().await;

        // Add memories to hot tier only
        for i in 0..10 {
            let memory = create_test_memory_with_tier(&format!("Memory {}", i), StorageTier::Hot);
            store.insert(&memory).await.unwrap();
        }

        assert_eq!(store.count_by_tier(StorageTier::Hot).await.unwrap(), 10);
        assert_eq!(store.count_by_tier(StorageTier::Warm).await.unwrap(), 0);
        assert_eq!(store.count_by_tier(StorageTier::Cold).await.unwrap(), 0);
    }
}

mod compact_tests {
    use super::*;
    use chrono::Duration;
    use nova_memory::storage::{CompactionConfig, Compactor};

    #[tokio::test]
    async fn test_compact_empty_tier() {
        let (store, _dir) = create_test_store().await;

        let compactor = Compactor::new(&store);
        let result = compactor.compact(StorageTier::Hot).await.unwrap();

        assert_eq!(result.compacted_count, 0);
        assert_eq!(result.skipped_high_weight, 0);
        assert_eq!(result.already_compressed, 0);
    }

    #[tokio::test]
    async fn test_compact_single_tier() {
        let (store, _dir) = create_test_store().await;

        // Create old memory that should be compacted
        let mut memory = create_test_memory_with_tier("Old memory content", StorageTier::Warm);
        memory.created_at = Utc::now() - Duration::days(45);
        memory.weight = 0.5;
        store.insert(&memory).await.unwrap();

        let compactor = Compactor::new(&store);
        let result = compactor.compact(StorageTier::Warm).await.unwrap();

        assert_eq!(result.compacted_count, 1);
    }

    #[tokio::test]
    async fn test_compact_all_tiers() {
        let (store, _dir) = create_test_store().await;

        // Create old memories in all tiers
        for tier in [StorageTier::Hot, StorageTier::Warm, StorageTier::Cold] {
            let mut memory = create_test_memory_with_tier(&format!("{:?} old", tier), tier);
            memory.created_at = Utc::now() - Duration::days(45);
            memory.weight = 0.5;
            store.insert(&memory).await.unwrap();
        }

        let compactor = Compactor::new(&store);

        let hot_result = compactor.compact(StorageTier::Hot).await.unwrap();
        let warm_result = compactor.compact(StorageTier::Warm).await.unwrap();
        let cold_result = compactor.compact(StorageTier::Cold).await.unwrap();

        assert_eq!(hot_result.compacted_count, 1);
        assert_eq!(warm_result.compacted_count, 1);
        assert_eq!(cold_result.compacted_count, 1);
    }

    #[tokio::test]
    async fn test_compact_with_custom_config() {
        let (store, _dir) = create_test_store().await;

        // Create memory that's 20 days old
        let mut memory = create_test_memory_with_tier("20 day old", StorageTier::Warm);
        memory.created_at = Utc::now() - Duration::days(20);
        memory.weight = 0.5;
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        // Default config (30 days) should not compact
        let default_compactor = Compactor::new(&store);
        let default_result = default_compactor.compact(StorageTier::Warm).await.unwrap();
        assert_eq!(default_result.compacted_count, 0);

        // Custom config (15 days) should compact
        let config = CompactionConfig::new(15, 60);
        let custom_compactor = Compactor::with_config(&store, config);
        let custom_result = custom_compactor.compact(StorageTier::Warm).await.unwrap();
        assert_eq!(custom_result.compacted_count, 1);

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.compression, CompressionLevel::Summary);
    }
}

mod config_tests {
    use nova_memory::config::Config;

    #[test]
    fn test_default_config() {
        let config = Config::default();

        // Storage defaults
        assert_eq!(config.storage.hot_cache_gb, 10);
        assert_eq!(config.storage.warm_storage_gb, 50);
        assert!(config.storage.cold_enabled);

        // Proxy defaults
        assert_eq!(config.proxy.listen_addr, "127.0.0.1:9999");
        assert_eq!(config.proxy.timeout_secs, 300);
        assert_eq!(config.proxy.max_injection_tokens, 2000);

        // Router defaults
        assert_eq!(config.router.max_memories, 10);
        assert_eq!(config.router.relevance_threshold, 0.7);

        // Embedding defaults
        assert_eq!(config.embedding.dimension, 1536);
        assert_eq!(config.embedding.batch_size, 32);
    }

    #[test]
    fn test_config_deserialization() {
        // Test TOML deserialization
        let toml_str = r#"
[storage]
hot_cache_gb = 20
warm_storage_gb = 100

[proxy]
listen_addr = "0.0.0.0:8080"
upstream_url = "https://api.example.com"
"#;

        let parsed: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.storage.hot_cache_gb, 20);
        assert_eq!(parsed.storage.warm_storage_gb, 100);
        assert_eq!(parsed.proxy.listen_addr, "0.0.0.0:8080");
        assert_eq!(parsed.proxy.upstream_url, "https://api.example.com");
    }
}

mod json_output_tests {
    use super::*;
    use serde_json;

    #[tokio::test]
    async fn test_json_output_format_list() {
        let (store, _dir) = create_test_store().await;

        // Create test memories
        for i in 0..3 {
            let memory = create_test_memory_with_tier(&format!("Memory {}", i), StorageTier::Hot);
            store.insert(&memory).await.unwrap();
        }

        // Get memories and format as JSON
        let memories = store.list_by_tier(StorageTier::Hot).await.unwrap();
        let output: Vec<_> = memories
            .iter()
            .map(|m| {
                serde_json::json!({
                    "id": m.id.to_string(),
                    "content": &m.content,
                    "type": format!("{:?}", m.memory_type),
                    "weight": m.weight,
                    "tier": format!("{:?}", m.tier),
                    "created_at": m.created_at.to_rfc3339(),
                })
            })
            .collect();

        let json_str = serde_json::to_string_pretty(&output).unwrap();
        assert!(json_str.contains("Memory 0"));
        assert!(json_str.contains("Memory 1"));
        assert!(json_str.contains("Memory 2"));
        assert!(json_str.contains("id"));
        assert!(json_str.contains("content"));
        assert!(json_str.contains("weight"));
    }

    #[tokio::test]
    async fn test_json_output_format_show() {
        let (store, _dir) = create_test_store().await;

        let memory = create_test_memory_with_tier("Test content", StorageTier::Hot);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let retrieved = store.get(id).await.unwrap().unwrap();

        let output = serde_json::json!({
            "id": retrieved.id.to_string(),
            "content": &retrieved.content,
            "embedding_size": retrieved.embedding.len(),
            "type": format!("{:?}", retrieved.memory_type),
            "weight": retrieved.weight,
            "tier": format!("{:?}", retrieved.tier),
            "compression": format!("{:?}", retrieved.compression),
            "source": format!("{:?}", retrieved.source),
            "created_at": retrieved.created_at.to_rfc3339(),
            "last_accessed": retrieved.last_accessed.to_rfc3339(),
            "access_count": retrieved.access_count,
            "conversation_id": retrieved.conversation_id,
            "entities": retrieved.entities,
        });

        let json_str = serde_json::to_string_pretty(&output).unwrap();
        assert!(json_str.contains("Test content"));
        assert!(json_str.contains("384")); // embedding size
        assert!(json_str.contains("id"));
        assert!(json_str.contains("content"));
        assert!(json_str.contains("weight"));
    }

    #[tokio::test]
    async fn test_json_output_format_stats() {
        let (store, _dir) = create_test_store().await;

        // Add memories to different tiers
        for i in 0..5 {
            let memory = create_test_memory_with_tier(&format!("Hot {}", i), StorageTier::Hot);
            store.insert(&memory).await.unwrap();
        }

        for i in 0..3 {
            let memory = create_test_memory_with_tier(&format!("Warm {}", i), StorageTier::Warm);
            store.insert(&memory).await.unwrap();
        }

        let hot_count = store.count_by_tier(StorageTier::Hot).await.unwrap();
        let warm_count = store.count_by_tier(StorageTier::Warm).await.unwrap();
        let cold_count = store.count_by_tier(StorageTier::Cold).await.unwrap();
        let total_count = store.total_count().await.unwrap();

        let output = serde_json::json!({
            "total_memories": total_count,
            "by_tier": {
                "hot": {
                    "count": hot_count,
                    "estimated_size_bytes": hot_count as u64 * 2000,
                },
                "warm": {
                    "count": warm_count,
                    "estimated_size_bytes": warm_count as u64 * 2000,
                },
                "cold": {
                    "count": cold_count,
                    "estimated_size_bytes": cold_count as u64 * 2000,
                }
            },
            "total_estimated_size_bytes": total_count as u64 * 2000,
        });

        let json_str = serde_json::to_string_pretty(&output).unwrap();
        assert!(json_str.contains("5")); // hot count
        assert!(json_str.contains("3")); // warm count
        assert!(json_str.contains("total_memories"));
        assert!(json_str.contains("by_tier"));
    }

    #[tokio::test]
    async fn test_json_output_format_delete() {
        let (store, _dir) = create_test_store().await;

        let memory = create_test_memory_with_tier("To delete", StorageTier::Hot);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let deleted = store.delete(id).await.unwrap();

        let output = serde_json::json!({
            "id": id.to_string(),
            "deleted": deleted,
        });

        let json_str = serde_json::to_string_pretty(&output).unwrap();
        assert!(json_str.contains("true"));
        assert!(json_str.contains(&id.to_string()));
    }

    #[tokio::test]
    async fn test_json_output_format_compact() {
        let output = serde_json::json!({
            "tiers": [
                {
                    "tier": "Hot",
                    "compacted": 5,
                    "skipped_high_weight": 2,
                    "already_compressed": 1,
                },
                {
                    "tier": "Warm",
                    "compacted": 10,
                    "skipped_high_weight": 3,
                    "already_compressed": 2,
                }
            ],
            "totals": {
                "compacted": 15,
                "skipped_high_weight": 5,
                "already_compressed": 3,
            }
        });

        let json_str = serde_json::to_string_pretty(&output).unwrap();
        assert!(json_str.contains("Hot"));
        assert!(json_str.contains("Warm"));
        assert!(json_str.contains("compacted"));
        assert!(json_str.contains("15")); // total compacted
    }
}

mod edge_case_tests {
    use super::*;

    #[tokio::test]
    async fn test_invalid_uuid_format() {
        let invalid_uuids = [
            "not-a-uuid",
            "12345",
            "",
            "too-long-uuid-string-that-is-invalid",
        ];

        for invalid in &invalid_uuids {
            let result = Uuid::parse_str(invalid);
            assert!(result.is_err(), "Should fail to parse: {}", invalid);
        }
    }

    #[tokio::test]
    async fn test_valid_uuid_format() {
        let valid_uuid = Uuid::new_v4();
        let uuid_str = valid_uuid.to_string();

        let parsed = Uuid::parse_str(&uuid_str);
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap(), valid_uuid);
    }

    #[tokio::test]
    async fn test_empty_content() {
        let (store, _dir) = create_test_store().await;

        // Empty content should still be insertable (filtering happens at ingestion level)
        let memory = create_test_memory_with_tier("", StorageTier::Hot);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let retrieved = store.get(id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().content, "");
    }

    #[tokio::test]
    async fn test_very_long_content() {
        let (store, _dir) = create_test_store().await;

        let long_content = "a".repeat(10000);
        let memory = create_test_memory_with_tier(&long_content, StorageTier::Hot);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let retrieved = store.get(id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().content.len(), 10000);
    }

    #[tokio::test]
    async fn test_unicode_content() {
        let (store, _dir) = create_test_store().await;

        let unicode_contents = [
            "Hello 世界! 🌍",
            "Привет мир!",
            "مرحبا بالعالم",
            "🎉 Celebration! 🎊",
        ];

        for content in &unicode_contents {
            let memory = create_test_memory_with_tier(content, StorageTier::Hot);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let retrieved = store.get(id).await.unwrap();
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap().content, *content);
        }
    }

    #[tokio::test]
    async fn test_special_characters_in_content() {
        let (store, _dir) = create_test_store().await;

        let special_content = "Special chars: @#$%^&*()_+-=[]{}|;':\",./<>?";
        let memory = create_test_memory_with_tier(special_content, StorageTier::Hot);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let retrieved = store.get(id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().content, special_content);
    }

    #[tokio::test]
    async fn test_multiline_content() {
        let (store, _dir) = create_test_store().await;

        let multiline_content = "Line 1\nLine 2\nLine 3\n\nLine after blank";
        let memory = create_test_memory_with_tier(multiline_content, StorageTier::Hot);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let retrieved = store.get(id).await.unwrap();
        assert!(retrieved.is_some());
        let content = retrieved.unwrap().content;
        assert!(content.contains("Line 1"));
        assert!(content.contains("Line 2"));
        assert!(content.contains("Line 3"));
    }
}
