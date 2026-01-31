//! Integration tests for Phase 4 capacity management features
//!
//! Tests tier migration, compaction, eviction, and tombstone functionality.

use chrono::{Duration, Utc};
use nova_memory::memory::tombstone::{EvictionReason, Tombstone};
use nova_memory::memory::types::{
    CompressionLevel, Memory, MemorySource, MemoryType, StorageTier,
};
use nova_memory::storage::{
    CompactionConfig, Compactor, EvictionConfig, Evictor, LanceStore, TierConfig, TierManager,
};
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

/// Test helper: Create a test memory with specific tier
fn create_test_memory_with_tier(content: &str, tier: StorageTier) -> Memory {
    let mut memory = Memory::new(
        content.to_string(),
        vec![0.1; 384],
        MemoryType::Semantic,
        MemorySource::Manual,
    );
    memory.tier = tier;
    memory.weight = 0.5;
    memory
}

/// Test helper: Create a test memory with specific tier and access count
fn create_test_memory_with_access(
    content: &str,
    tier: StorageTier,
    access_count: u32,
) -> Memory {
    let mut memory = create_test_memory_with_tier(content, tier);
    memory.access_count = access_count;
    memory
}

/// Test helper: Create a test memory with specific age
fn create_test_memory_with_age(content: &str, tier: StorageTier, age_days: i64) -> Memory {
    let mut memory = create_test_memory_with_tier(content, tier);
    memory.created_at = Utc::now() - Duration::days(age_days);
    memory.weight = 0.5;
    memory
}

/// Test helper: Create a test memory with specific weight
fn create_test_memory_with_weight(content: &str, tier: StorageTier, weight: f32) -> Memory {
    let mut memory = create_test_memory_with_tier(content, tier);
    memory.weight = weight;
    memory
}

/// Test helper: Create a test memory with entities and old access time
fn create_test_memory_with_entities(
    content: &str,
    tier: StorageTier,
    entities: Vec<String>,
) -> Memory {
    let mut memory = create_test_memory_with_tier(content, tier);
    memory.entities = entities;
    memory.last_accessed = Utc::now() - Duration::hours(48);
    memory
}

mod tier_migration_tests {
    use super::*;

    #[tokio::test]
    async fn test_migrate_hot_to_warm() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_tier("Hot memory", StorageTier::Hot);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);
        manager
            .migrate(id, StorageTier::Hot, StorageTier::Warm)
            .await
            .unwrap();

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.tier, StorageTier::Warm);
    }

    #[tokio::test]
    async fn test_migrate_warm_to_cold() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_tier("Warm memory", StorageTier::Warm);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);
        manager
            .migrate(id, StorageTier::Warm, StorageTier::Cold)
            .await
            .unwrap();

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.tier, StorageTier::Cold);
    }

    #[tokio::test]
    async fn test_migrate_cold_to_hot() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_tier("Cold memory", StorageTier::Cold);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);
        manager
            .migrate(id, StorageTier::Cold, StorageTier::Hot)
            .await
            .unwrap();

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.tier, StorageTier::Hot);
    }

    #[tokio::test]
    async fn test_migrate_same_tier_is_noop() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_tier("Hot memory", StorageTier::Hot);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);
        manager
            .migrate(id, StorageTier::Hot, StorageTier::Hot)
            .await
            .unwrap();

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.tier, StorageTier::Hot);
    }

    #[tokio::test]
    async fn test_migrate_tier_mismatch_error() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_tier("Hot memory", StorageTier::Hot);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);
        let result = manager
            .migrate(id, StorageTier::Warm, StorageTier::Cold)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_migrate_nonexistent_memory() {
        let (store, _dir) = create_test_store().await;
        let manager = TierManager::new(&store);
        let result = manager
            .migrate(Uuid::new_v4(), StorageTier::Hot, StorageTier::Warm)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_promote_cold_to_warm() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_tier("Cold memory", StorageTier::Cold);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);
        manager.promote(id).await.unwrap();

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.tier, StorageTier::Warm);
    }

    #[tokio::test]
    async fn test_promote_warm_to_hot() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_tier("Warm memory", StorageTier::Warm);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);
        manager.promote(id).await.unwrap();

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.tier, StorageTier::Hot);
    }

    #[tokio::test]
    async fn test_promote_hot_is_noop() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_tier("Hot memory", StorageTier::Hot);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);
        manager.promote(id).await.unwrap();

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.tier, StorageTier::Hot);
    }

    #[tokio::test]
    async fn test_demote_hot_to_warm() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_tier("Hot memory", StorageTier::Hot);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);
        manager.demote(id).await.unwrap();

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.tier, StorageTier::Warm);
    }

    #[tokio::test]
    async fn test_demote_warm_to_cold() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_tier("Warm memory", StorageTier::Warm);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);
        manager.demote(id).await.unwrap();

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.tier, StorageTier::Cold);
    }

    #[tokio::test]
    async fn test_demote_cold_is_noop() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_tier("Cold memory", StorageTier::Cold);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);
        manager.demote(id).await.unwrap();

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.tier, StorageTier::Cold);
    }

    #[tokio::test]
    async fn test_full_tier_lifecycle() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_tier("Lifecycle memory", StorageTier::Hot);
        let id = memory.id;
        let original_content = memory.content.clone();
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);

        // Hot -> Warm -> Cold
        manager.demote(id).await.unwrap();
        assert_eq!(manager.get_tier(id).await.unwrap(), StorageTier::Warm);

        manager.demote(id).await.unwrap();
        assert_eq!(manager.get_tier(id).await.unwrap(), StorageTier::Cold);

        // Cold -> Warm -> Hot
        manager.promote(id).await.unwrap();
        assert_eq!(manager.get_tier(id).await.unwrap(), StorageTier::Warm);

        manager.promote(id).await.unwrap();
        assert_eq!(manager.get_tier(id).await.unwrap(), StorageTier::Hot);

        // Verify content preserved
        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.content, original_content);
    }

    #[tokio::test]
    async fn test_should_promote_below_threshold() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_access("Low access", StorageTier::Warm, 3);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);
        let should_promote = manager.should_promote(id).await.unwrap();
        assert!(!should_promote);
    }

    #[tokio::test]
    async fn test_should_promote_at_threshold() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_access("At threshold", StorageTier::Warm, 5);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);
        let should_promote = manager.should_promote(id).await.unwrap();
        assert!(should_promote);
    }

    #[tokio::test]
    async fn test_should_promote_already_hot() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_access("Already hot", StorageTier::Hot, 10);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);
        let should_promote = manager.should_promote(id).await.unwrap();
        assert!(!should_promote);
    }

    #[tokio::test]
    async fn test_check_and_promote_triggers_promotion() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_access("Promote me", StorageTier::Cold, 10);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);
        let promoted = manager.check_and_promote(id).await.unwrap();
        assert!(promoted);

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.tier, StorageTier::Warm);
    }

    #[tokio::test]
    async fn test_check_and_promote_no_promotion_needed() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_access("Stay warm", StorageTier::Warm, 2);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let manager = TierManager::new(&store);
        let promoted = manager.check_and_promote(id).await.unwrap();
        assert!(!promoted);

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.tier, StorageTier::Warm);
    }

    #[tokio::test]
    async fn test_custom_tier_config() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_access("Custom config", StorageTier::Warm, 3);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let default_manager = TierManager::new(&store);
        assert!(!default_manager.should_promote(id).await.unwrap());

        let custom_config = TierConfig::new(10, 50, 2);
        let custom_manager = TierManager::with_config(&store, custom_config);
        assert!(custom_manager.should_promote(id).await.unwrap());
    }
}

mod compaction_tests {
    use super::*;

    #[tokio::test]
    async fn test_compact_full_to_summary() {
        let (store, _dir) = create_test_store().await;
        let long_content = "First sentence here. Second sentence here. Third sentence here. Fourth sentence here.";
        let memory = create_test_memory_with_age(long_content, StorageTier::Warm, 45);
        let id = memory.id;
        let original_len = memory.content.len();
        store.insert(&memory).await.unwrap();

        let compactor = Compactor::new(&store);
        let result = compactor.compact(StorageTier::Warm).await.unwrap();

        assert_eq!(result.compacted_count, 1);
        assert!(result.compacted_ids.contains(&id));

        let updated = store.get(id).await.unwrap().unwrap();
        assert!(updated.content.len() < original_len);
        assert_eq!(updated.compression, CompressionLevel::Summary);
    }

    #[tokio::test]
    async fn test_compact_summary_to_keywords() {
        let (store, _dir) = create_test_store().await;
        let content = "This is a detailed memory with multiple sentences. It contains important information about the system. There are several key points to remember here.";
        let mut memory = create_test_memory_with_age(content, StorageTier::Warm, 100);
        memory.compression = CompressionLevel::Summary;
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let compactor = Compactor::new(&store);
        let result = compactor.compact(StorageTier::Warm).await.unwrap();

        assert_eq!(result.compacted_count, 1);

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.compression, CompressionLevel::Keywords);
    }

    #[tokio::test]
    async fn test_compact_keywords_to_hash() {
        let (store, _dir) = create_test_store().await;
        let content = "This is a very old memory that should be compressed to hash level.";
        let mut memory = create_test_memory_with_age(content, StorageTier::Warm, 100);
        memory.compression = CompressionLevel::Keywords;
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        // Use compact_single to explicitly target Hash level
        // Automatic compaction only progresses to Keywords based on age
        let compactor = Compactor::new(&store);
        let compacted = compactor
            .compact_single(id, CompressionLevel::Hash)
            .await
            .unwrap();

        assert!(compacted);

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.compression, CompressionLevel::Hash);
        assert_eq!(updated.content, "[content archived - searchable via embedding]");
    }

    #[tokio::test]
    async fn test_compact_preserves_embedding() {
        let (store, _dir) = create_test_store().await;
        let mut memory = create_test_memory_with_age("Test content", StorageTier::Warm, 45);
        memory.embedding = vec![0.5; 384];
        let id = memory.id;
        let original_embedding = memory.embedding.clone();
        store.insert(&memory).await.unwrap();

        let compactor = Compactor::new(&store);
        compactor.compact(StorageTier::Warm).await.unwrap();

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.embedding, original_embedding);
        assert_eq!(updated.embedding.len(), 384);
    }

    #[tokio::test]
    async fn test_compact_skips_high_weight_memories() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_weight("Important memory", StorageTier::Warm, 0.9);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let compactor = Compactor::new(&store);
        let result = compactor.compact(StorageTier::Warm).await.unwrap();

        assert_eq!(result.compacted_count, 0);
        assert_eq!(result.skipped_high_weight, 1);

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.compression, CompressionLevel::Full);
    }

    #[tokio::test]
    async fn test_compact_skips_recent_memories() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_age("Recent memory", StorageTier::Warm, 5);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let compactor = Compactor::new(&store);
        let result = compactor.compact(StorageTier::Warm).await.unwrap();

        assert_eq!(result.compacted_count, 0);

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.compression, CompressionLevel::Full);
    }

    #[tokio::test]
    async fn test_compact_skips_already_compressed() {
        let (store, _dir) = create_test_store().await;
        let mut memory = create_test_memory_with_age("Already compressed", StorageTier::Warm, 45);
        memory.compression = CompressionLevel::Summary;
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let compactor = Compactor::new(&store);
        let result = compactor.compact(StorageTier::Warm).await.unwrap();

        assert_eq!(result.compacted_count, 0);
        assert_eq!(result.already_compressed, 1);

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.compression, CompressionLevel::Summary);
    }

    #[tokio::test]
    async fn test_compact_single() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_age("Single compaction", StorageTier::Warm, 45);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let compactor = Compactor::new(&store);
        let compacted = compactor
            .compact_single(id, CompressionLevel::Keywords)
            .await
            .unwrap();

        assert!(compacted);

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.compression, CompressionLevel::Keywords);
    }

    #[tokio::test]
    async fn test_compact_single_respects_weight() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_weight("High weight", StorageTier::Warm, 0.85);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let compactor = Compactor::new(&store);
        let compacted = compactor
            .compact_single(id, CompressionLevel::Summary)
            .await
            .unwrap();

        assert!(!compacted);

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.compression, CompressionLevel::Full);
    }

    #[tokio::test]
    async fn test_compact_nonexistent_memory() {
        let (store, _dir) = create_test_store().await;
        let compactor = Compactor::new(&store);
        let result = compactor
            .compact_single(Uuid::new_v4(), CompressionLevel::Summary)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_compact_with_custom_config() {
        let (store, _dir) = create_test_store().await;
        let memory = create_test_memory_with_age("Custom config", StorageTier::Warm, 20);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        let config = CompactionConfig::new(15, 60);
        let compactor = Compactor::with_config(&store, config);
        let result = compactor.compact(StorageTier::Warm).await.unwrap();

        assert_eq!(result.compacted_count, 1);

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.compression, CompressionLevel::Summary);
    }

    #[tokio::test]
    async fn test_compact_all_tiers() {
        let (store, _dir) = create_test_store().await;

        // Create memories in different tiers
        let hot_memory = create_test_memory_with_age("Hot old", StorageTier::Hot, 45);
        let warm_memory = create_test_memory_with_age("Warm old", StorageTier::Warm, 45);
        let cold_memory = create_test_memory_with_age("Cold old", StorageTier::Cold, 45);

        store.insert(&hot_memory).await.unwrap();
        store.insert(&warm_memory).await.unwrap();
        store.insert(&cold_memory).await.unwrap();

        let compactor = Compactor::new(&store);

        // Compact each tier
        let hot_result = compactor.compact(StorageTier::Hot).await.unwrap();
        let warm_result = compactor.compact(StorageTier::Warm).await.unwrap();
        let cold_result = compactor.compact(StorageTier::Cold).await.unwrap();

        assert_eq!(hot_result.compacted_count, 1);
        assert_eq!(warm_result.compacted_count, 1);
        assert_eq!(cold_result.compacted_count, 1);
    }

    #[tokio::test]
    async fn test_progressive_compression_over_time() {
        let (store, _dir) = create_test_store().await;
        let content = "First sentence. Second sentence. Third sentence. Fourth sentence. Fifth sentence.";

        // Create memory at different ages
        let new_memory = create_test_memory_with_age(content, StorageTier::Warm, 5);
        let medium_memory = create_test_memory_with_age(content, StorageTier::Warm, 45);
        let old_memory = create_test_memory_with_age(content, StorageTier::Warm, 100);

        let new_id = new_memory.id;
        let medium_id = medium_memory.id;
        let old_id = old_memory.id;

        store.insert(&new_memory).await.unwrap();
        store.insert(&medium_memory).await.unwrap();
        store.insert(&old_memory).await.unwrap();

        let compactor = Compactor::new(&store);
        compactor.compact(StorageTier::Warm).await.unwrap();

        let new_updated = store.get(new_id).await.unwrap().unwrap();
        let medium_updated = store.get(medium_id).await.unwrap().unwrap();
        let old_updated = store.get(old_id).await.unwrap().unwrap();

        assert_eq!(new_updated.compression, CompressionLevel::Full);
        assert_eq!(medium_updated.compression, CompressionLevel::Summary);
        assert_eq!(old_updated.compression, CompressionLevel::Keywords);
    }
}

mod eviction_tests {
    use super::*;

    #[tokio::test]
    async fn test_eviction_priority_recent_access() {
        let (store, _dir) = create_test_store().await;
        let evictor = Evictor::new(&store);

        let recent = create_memory_with_access_time("Recent", 0.5, StorageTier::Hot, 1);
        let old = create_memory_with_access_time("Old", 0.5, StorageTier::Hot, 100);

        let recent_priority = evictor.eviction_priority(&recent);
        let old_priority = evictor.eviction_priority(&old);

        assert!(
            recent_priority > old_priority,
            "Recently accessed memory should have higher priority"
        );
    }

    #[tokio::test]
    async fn test_eviction_priority_higher_weight() {
        let (store, _dir) = create_test_store().await;
        let evictor = Evictor::new(&store);

        let high_weight = create_memory_with_access_time("High", 0.9, StorageTier::Hot, 50);
        let low_weight = create_memory_with_access_time("Low", 0.2, StorageTier::Hot, 50);

        let high_priority = evictor.eviction_priority(&high_weight);
        let low_priority = evictor.eviction_priority(&low_weight);

        assert!(
            high_priority > low_priority,
            "Higher weight memory should have higher priority"
        );
    }

    #[tokio::test]
    async fn test_is_protected_recent_access() {
        let (store, _dir) = create_test_store().await;
        let evictor = Evictor::new(&store);

        let recent = create_memory_with_access_time("Recent", 0.3, StorageTier::Hot, 1);
        let old = create_memory_with_access_time("Old", 0.3, StorageTier::Hot, 48);

        assert!(evictor.is_protected(&recent));
        assert!(!evictor.is_protected(&old));
    }

    #[tokio::test]
    async fn test_is_protected_high_weight() {
        let (store, _dir) = create_test_store().await;
        let evictor = Evictor::new(&store);

        let high_weight = create_memory_with_access_time("High", 0.8, StorageTier::Hot, 100);
        let low_weight = create_memory_with_access_time("Low", 0.3, StorageTier::Hot, 100);

        assert!(evictor.is_protected(&high_weight));
        assert!(!evictor.is_protected(&low_weight));
    }

    #[tokio::test]
    async fn test_capacity_status_normal() {
        let (store, _dir) = create_test_store().await;
        let config = EvictionConfig {
            max_memories_per_tier: 100,
            ..EvictionConfig::default()
        };
        let evictor = Evictor::with_config(&store, config);

        // Insert 50 memories = 50% capacity
        let memories: Vec<Memory> = (0..50)
            .map(|i| create_test_memory_with_tier(&format!("Memory {}", i), StorageTier::Hot))
            .collect();
        store.insert_batch(&memories).await.unwrap();

        let status = evictor.check_capacity(StorageTier::Hot).await.unwrap();
        assert_eq!(status, nova_memory::storage::CapacityStatus::Normal);
    }

    #[tokio::test]
    async fn test_capacity_status_warning() {
        let (store, _dir) = create_test_store().await;
        let config = EvictionConfig {
            max_memories_per_tier: 100,
            ..EvictionConfig::default()
        };
        let evictor = Evictor::with_config(&store, config);

        // Insert 75 memories = 75% capacity
        let memories: Vec<Memory> = (0..75)
            .map(|i| create_test_memory_with_tier(&format!("Memory {}", i), StorageTier::Hot))
            .collect();
        store.insert_batch(&memories).await.unwrap();

        let status = evictor.check_capacity(StorageTier::Hot).await.unwrap();
        assert_eq!(status, nova_memory::storage::CapacityStatus::Warning);
    }

    #[tokio::test]
    async fn test_capacity_status_eviction_needed() {
        let (store, _dir) = create_test_store().await;
        let config = EvictionConfig {
            max_memories_per_tier: 100,
            ..EvictionConfig::default()
        };
        let evictor = Evictor::with_config(&store, config);

        // Insert 85 memories = 85% capacity
        let memories: Vec<Memory> = (0..85)
            .map(|i| create_test_memory_with_tier(&format!("Memory {}", i), StorageTier::Hot))
            .collect();
        store.insert_batch(&memories).await.unwrap();

        let status = evictor.check_capacity(StorageTier::Hot).await.unwrap();
        assert_eq!(status, nova_memory::storage::CapacityStatus::EvictionNeeded);
    }

    #[tokio::test]
    async fn test_capacity_status_aggressive() {
        let (store, _dir) = create_test_store().await;
        let config = EvictionConfig {
            max_memories_per_tier: 100,
            ..EvictionConfig::default()
        };
        let evictor = Evictor::with_config(&store, config);

        // Insert 96 memories = 96% capacity
        let memories: Vec<Memory> = (0..96)
            .map(|i| create_test_memory_with_tier(&format!("Memory {}", i), StorageTier::Hot))
            .collect();
        store.insert_batch(&memories).await.unwrap();

        let status = evictor.check_capacity(StorageTier::Hot).await.unwrap();
        assert_eq!(
            status,
            nova_memory::storage::CapacityStatus::AggressiveEvictionNeeded
        );
    }

    #[tokio::test]
    async fn test_evict_if_needed_below_threshold() {
        let (store, _dir) = create_test_store().await;
        let config = EvictionConfig {
            max_memories_per_tier: 100,
            ..EvictionConfig::default()
        };
        let evictor = Evictor::with_config(&store, config);

        // Insert 50 memories = 50% capacity (below 80% threshold)
        let memories: Vec<Memory> = (0..50)
            .map(|i| {
                create_memory_with_access_time(
                    &format!("Memory {}", i),
                    0.3,
                    StorageTier::Hot,
                    100,
                )
            })
            .collect();
        store.insert_batch(&memories).await.unwrap();

        let evicted = evictor.evict_if_needed(StorageTier::Hot).await.unwrap();
        assert!(evicted.is_empty());
    }

    #[tokio::test]
    async fn test_evict_lowest_priority_first() {
        let (store, _dir) = create_test_store().await;
        let config = EvictionConfig {
            max_memories_per_tier: 10,
            recent_access_hours: 1,
            min_weight_protected: 0.9,
            ..EvictionConfig::default()
        };
        let evictor = Evictor::with_config(&store, config);

        // Insert memories with different weights (all old access, low weight)
        let mut memories = Vec::new();
        for i in 0..9 {
            let weight = 0.1 + (i as f32) * 0.05;
            let mem = create_memory_with_access_time(
                &format!("Mem-{}", i),
                weight,
                StorageTier::Hot,
                48,
            );
            memories.push(mem);
        }
        store.insert_batch(&memories).await.unwrap();

        let initial_count = store.count_by_tier(StorageTier::Hot).await.unwrap();
        assert_eq!(initial_count, 9);

        // At 90% capacity, eviction should occur
        let evicted = evictor.evict_if_needed(StorageTier::Hot).await.unwrap();

        assert!(!evicted.is_empty());

        let final_count = store.count_by_tier(StorageTier::Hot).await.unwrap();
        assert!(final_count < initial_count);
    }

    #[tokio::test]
    async fn test_protected_memories_not_evicted() {
        let (store, _dir) = create_test_store().await;
        let config = EvictionConfig {
            max_memories_per_tier: 10,
            recent_access_hours: 24,
            min_weight_protected: 0.7,
            ..EvictionConfig::default()
        };
        let evictor = Evictor::with_config(&store, config);

        // Insert 9 memories at 90% capacity
        let mut memories = Vec::new();

        // Protected by recency
        for i in 0..3 {
            memories.push(create_memory_with_access_time(
                &format!("Recent-{}", i),
                0.3,
                StorageTier::Hot,
                1,
            ));
        }

        // Protected by weight
        for i in 0..3 {
            memories.push(create_memory_with_access_time(
                &format!("Heavy-{}", i),
                0.8,
                StorageTier::Hot,
                100,
            ));
        }

        // Unprotected
        for i in 0..3 {
            memories.push(create_memory_with_access_time(
                &format!("Evictable-{}", i),
                0.2,
                StorageTier::Hot,
                100,
            ));
        }

        let protected_ids: Vec<Uuid> = memories[0..6].iter().map(|m| m.id).collect();
        store.insert_batch(&memories).await.unwrap();

        let evicted = evictor.evict_if_needed(StorageTier::Hot).await.unwrap();

        // Verify no protected memories were evicted
        for evicted_id in &evicted {
            assert!(
                !protected_ids.contains(evicted_id),
                "Protected memory should not be evicted"
            );
        }
    }

    #[tokio::test]
    async fn test_get_eviction_candidates() {
        let (store, _dir) = create_test_store().await;
        let config = EvictionConfig {
            max_memories_per_tier: 100,
            recent_access_hours: 1,
            min_weight_protected: 0.9,
            ..EvictionConfig::default()
        };
        let evictor = Evictor::with_config(&store, config);

        // Insert memories with varying priorities
        let mut memories = Vec::new();
        for i in 0..5 {
            let weight = 0.1 + (i as f32) * 0.1;
            let mem = create_memory_with_access_time(
                &format!("Candidate-{}", i),
                weight,
                StorageTier::Hot,
                48,
            );
            memories.push(mem);
        }
        store.insert_batch(&memories).await.unwrap();

        let candidates = evictor
            .get_eviction_candidates(StorageTier::Hot, 3)
            .await
            .unwrap();

        assert_eq!(candidates.len(), 3);

        // Verify candidates are sorted by priority (lowest first)
        for i in 0..candidates.len() - 1 {
            assert!(
                candidates[i].1 <= candidates[i + 1].1,
                "Candidates should be sorted by priority ascending"
            );
        }
    }

    #[tokio::test]
    async fn test_capacity_ratio_calculation() {
        let (store, _dir) = create_test_store().await;
        let config = EvictionConfig {
            max_memories_per_tier: 100,
            ..EvictionConfig::default()
        };
        let evictor = Evictor::with_config(&store, config);

        // Insert 50 memories
        let memories: Vec<Memory> = (0..50)
            .map(|i| create_test_memory_with_tier(&format!("Memory {}", i), StorageTier::Hot))
            .collect();
        store.insert_batch(&memories).await.unwrap();

        let ratio = evictor.capacity_ratio(StorageTier::Hot).await.unwrap();
        assert!((ratio - 0.5).abs() < 0.01, "Capacity ratio should be 0.5");
    }

    fn create_memory_with_access_time(
        content: &str,
        weight: f32,
        tier: StorageTier,
        hours_ago: i64,
    ) -> Memory {
        let mut memory = create_test_memory_with_tier(content, tier);
        memory.weight = weight;
        memory.last_accessed = Utc::now() - Duration::hours(hours_ago);
        memory
    }
}

mod tombstone_tests {
    use super::*;

    #[tokio::test]
    async fn test_tombstone_creation_on_eviction() {
        let (store, _dir) = create_test_store().await;
        let config = EvictionConfig {
            max_memories_per_tier: 10,
            recent_access_hours: 1,
            min_weight_protected: 0.9,
            ..EvictionConfig::default()
        };
        let evictor = Evictor::with_config(&store, config);

        // Insert 9 memories at 90% capacity with entities
        let mut memories = Vec::new();
        for i in 0..9 {
            let mem = create_test_memory_with_entities(
                &format!("Evictable-{}", i),
                StorageTier::Hot,
                vec![format!("topic-{}", i), "shared-topic".to_string()],
            );
            memories.push(mem);
        }
        store.insert_batch(&memories).await.unwrap();

        // Verify no tombstones exist before eviction
        let tombstones_before = store.list_all_tombstones().await.unwrap();
        assert!(tombstones_before.is_empty());

        // Trigger eviction
        let evicted = evictor.evict_if_needed(StorageTier::Hot).await.unwrap();

        // Verify memories were evicted
        assert!(!evicted.is_empty());

        // Verify tombstones were created
        let tombstones_after = store.list_all_tombstones().await.unwrap();
        assert_eq!(tombstones_after.len(), evicted.len());
    }

    #[tokio::test]
    async fn test_tombstone_contains_correct_topics() {
        let (store, _dir) = create_test_store().await;
        let config = EvictionConfig {
            max_memories_per_tier: 10,
            recent_access_hours: 1,
            min_weight_protected: 0.9,
            ..EvictionConfig::default()
        };
        let evictor = Evictor::with_config(&store, config);

        // Create memory with specific entities
        let mut memory = create_test_memory_with_entities(
            "Test content",
            StorageTier::Hot,
            vec![
                "rust".to_string(),
                "programming".to_string(),
                "async".to_string(),
            ],
        );
        memory.id = Uuid::new_v4();
        let memory_id = memory.id;

        store.insert(&memory).await.unwrap();

        // Add more memories to trigger eviction
        for i in 0..8 {
            let mem = create_test_memory_with_tier(&format!("Filler-{}", i), StorageTier::Hot);
            store.insert(&mem).await.unwrap();
        }

        // Trigger eviction
        let evicted = evictor.evict_if_needed(StorageTier::Hot).await.unwrap();
        assert!(!evicted.is_empty());

        // Find the tombstone for our specific memory
        let tombstone = store.get_tombstone(memory_id).await.unwrap();
        assert!(tombstone.is_some());

        let tombstone = tombstone.unwrap();
        assert_eq!(tombstone.topics.len(), 3);
        assert!(tombstone.topics.contains(&"rust".to_string()));
        assert!(tombstone.topics.contains(&"programming".to_string()));
        assert!(tombstone.topics.contains(&"async".to_string()));
    }

    #[tokio::test]
    async fn test_tombstone_searchable_by_topic() {
        let (store, _dir) = create_test_store().await;
        let config = EvictionConfig {
            max_memories_per_tier: 10,
            recent_access_hours: 1,
            min_weight_protected: 0.9,
            ..EvictionConfig::default()
        };
        let evictor = Evictor::with_config(&store, config);

        // Create memories with different topics
        let mut memory1 = create_test_memory_with_entities(
            "Memory about machine learning",
            StorageTier::Hot,
            vec!["machine-learning".to_string(), "python".to_string()],
        );
        memory1.id = Uuid::new_v4();

        let mut memory2 = create_test_memory_with_entities(
            "Memory about web development",
            StorageTier::Hot,
            vec!["web".to_string(), "javascript".to_string()],
        );
        memory2.id = Uuid::new_v4();

        store.insert(&memory1).await.unwrap();
        store.insert(&memory2).await.unwrap();

        // Add more memories to trigger eviction
        for i in 0..8 {
            let mem = create_test_memory_with_tier(&format!("Filler-{}", i), StorageTier::Hot);
            store.insert(&mem).await.unwrap();
        }

        // Trigger eviction
        let evicted = evictor.evict_if_needed(StorageTier::Hot).await.unwrap();
        assert!(evicted.len() >= 2);

        // Search tombstones by topic
        let ml_tombstones = store
            .search_tombstones_by_topic("machine-learning")
            .await
            .unwrap();
        assert!(!ml_tombstones.is_empty());

        let python_tombstones = store.search_tombstones_by_topic("python").await.unwrap();
        assert!(!python_tombstones.is_empty());

        let web_tombstones = store.search_tombstones_by_topic("web").await.unwrap();
        assert!(!web_tombstones.is_empty());

        let nonexistent = store
            .search_tombstones_by_topic("nonexistent")
            .await
            .unwrap();
        assert!(nonexistent.is_empty());
    }

    #[tokio::test]
    async fn test_tombstone_has_approximate_date() {
        let (store, _dir) = create_test_store().await;
        let config = EvictionConfig {
            max_memories_per_tier: 10,
            recent_access_hours: 1,
            min_weight_protected: 0.9,
            ..EvictionConfig::default()
        };
        let evictor = Evictor::with_config(&store, config);

        // Create memory with specific creation date
        let mut memory = create_test_memory_with_entities(
            "Test content",
            StorageTier::Hot,
            vec!["test-topic".to_string()],
        );
        memory.id = Uuid::new_v4();
        let created_at = memory.created_at;
        let memory_id = memory.id;

        store.insert(&memory).await.unwrap();

        // Add more memories to trigger eviction
        for i in 0..8 {
            let mem = create_test_memory_with_tier(&format!("Filler-{}", i), StorageTier::Hot);
            store.insert(&mem).await.unwrap();
        }

        // Trigger eviction
        let evicted = evictor.evict_if_needed(StorageTier::Hot).await.unwrap();
        assert!(!evicted.is_empty());

        // Verify tombstone has correct approximate date
        let tombstone = store.get_tombstone(memory_id).await.unwrap().unwrap();
        assert_eq!(tombstone.approximate_date, created_at);
    }

    #[tokio::test]
    async fn test_tombstone_has_eviction_reason() {
        let (store, _dir) = create_test_store().await;
        let config = EvictionConfig {
            max_memories_per_tier: 10,
            recent_access_hours: 1,
            min_weight_protected: 0.9,
            ..EvictionConfig::default()
        };
        let evictor = Evictor::with_config(&store, config);

        // Create memory
        let mut memory = create_test_memory_with_entities(
            "Test content",
            StorageTier::Hot,
            vec!["test-topic".to_string()],
        );
        memory.id = Uuid::new_v4();
        let memory_id = memory.id;

        store.insert(&memory).await.unwrap();

        // Add more memories to trigger eviction
        for i in 0..8 {
            let mem = create_test_memory_with_tier(&format!("Filler-{}", i), StorageTier::Hot);
            store.insert(&mem).await.unwrap();
        }

        // Trigger eviction
        let evicted = evictor.evict_if_needed(StorageTier::Hot).await.unwrap();
        assert!(!evicted.is_empty());

        // Verify tombstone has eviction reason
        let tombstone = store.get_tombstone(memory_id).await.unwrap().unwrap();
        assert!(
            matches!(
                tombstone.reason,
                EvictionReason::StoragePressure | EvictionReason::LowWeight
            ),
            "Tombstone should have valid eviction reason"
        );
    }

    #[tokio::test]
    async fn test_manual_tombstone_insertion() {
        let (store, _dir) = create_test_store().await;

        let tombstone = Tombstone {
            original_id: Uuid::new_v4(),
            evicted_at: Utc::now(),
            topics: vec!["manual-topic".to_string()],
            participants: vec![],
            approximate_date: Utc::now(),
            reason: EvictionReason::ManualDeletion,
        };

        store.insert_tombstone(&tombstone).await.unwrap();

        let retrieved = store.get_tombstone(tombstone.original_id).await.unwrap();
        assert!(retrieved.is_some());

        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.original_id, tombstone.original_id);
        assert_eq!(retrieved.topics, vec!["manual-topic".to_string()]);
        assert!(matches!(retrieved.reason, EvictionReason::ManualDeletion));
    }

    #[tokio::test]
    async fn test_list_all_tombstones() {
        let (store, _dir) = create_test_store().await;

        // Create multiple tombstones
        for i in 0..5 {
            let tombstone = Tombstone {
                original_id: Uuid::new_v4(),
                evicted_at: Utc::now(),
                topics: vec![format!("topic-{}", i)],
                participants: vec![],
                approximate_date: Utc::now(),
                reason: EvictionReason::LowWeight,
            };
            store.insert_tombstone(&tombstone).await.unwrap();
        }

        let tombstones = store.list_all_tombstones().await.unwrap();
        assert_eq!(tombstones.len(), 5);
    }

    #[tokio::test]
    async fn test_get_nonexistent_tombstone() {
        let (store, _dir) = create_test_store().await;

        let result = store.get_tombstone(Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }
}

mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_tier_migration_then_compaction() {
        let (store, _dir) = create_test_store().await;

        // Create memory in hot tier
        let content = "First sentence. Second sentence. Third sentence. Fourth sentence.";
        let memory = create_test_memory_with_age(content, StorageTier::Hot, 45);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        // Migrate to warm
        let tier_manager = TierManager::new(&store);
        tier_manager
            .migrate(id, StorageTier::Hot, StorageTier::Warm)
            .await
            .unwrap();

        // Compact
        let compactor = Compactor::new(&store);
        compactor.compact(StorageTier::Warm).await.unwrap();

        let updated = store.get(id).await.unwrap().unwrap();
        assert_eq!(updated.tier, StorageTier::Warm);
        assert_eq!(updated.compression, CompressionLevel::Summary);
    }

    #[tokio::test]
    async fn test_full_capacity_workflow() {
        let (store, _dir) = create_test_store().await;

        // Create memories that will trigger eviction
        let config = EvictionConfig {
            max_memories_per_tier: 10,
            recent_access_hours: 1,
            min_weight_protected: 0.9,
            ..EvictionConfig::default()
        };
        let evictor = Evictor::with_config(&store, config);

        // Insert old, low-weight memories
        let mut memories = Vec::new();
        for i in 0..9 {
            let mut mem = create_test_memory_with_age(
                &format!("Old memory {} with some content here", i),
                StorageTier::Hot,
                45,
            );
            mem.weight = 0.1 + (i as f32) * 0.05;
            mem.last_accessed = Utc::now() - Duration::hours(48);
            memories.push(mem);
        }
        store.insert_batch(&memories).await.unwrap();

        // Compact first
        let compactor = Compactor::new(&store);
        let compact_result = compactor.compact(StorageTier::Hot).await.unwrap();
        assert!(compact_result.compacted_count > 0);

        // Then trigger eviction
        let evicted = evictor.evict_if_needed(StorageTier::Hot).await.unwrap();

        // Verify some memories were evicted and tombstones created
        let tombstones = store.list_all_tombstones().await.unwrap();
        assert_eq!(tombstones.len(), evicted.len());
    }

    #[tokio::test]
    async fn test_memory_retrievable_after_all_operations() {
        let (store, _dir) = create_test_store().await;

        // Create and insert memory
        let content = "Test content for retrieval after operations.";
        let memory = create_test_memory_with_tier(content, StorageTier::Hot);
        let id = memory.id;
        store.insert(&memory).await.unwrap();

        // Perform various operations
        let tier_manager = TierManager::new(&store);
        tier_manager
            .migrate(id, StorageTier::Hot, StorageTier::Warm)
            .await
            .unwrap();

        // Access the memory
        store.update_access(id).await.unwrap();

        // Verify still retrievable
        let retrieved = store.get(id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().content, content);
    }
}
