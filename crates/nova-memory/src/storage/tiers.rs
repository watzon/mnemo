//! Storage tier management for Nova Memory
//!
//! Implements automatic migration of memories between storage tiers (Hot, Warm, Cold)
//! based on access patterns and storage thresholds.

use uuid::Uuid;

use crate::error::{NovaError, Result};
use crate::memory::types::StorageTier;
use crate::storage::LanceStore;

/// Configuration for tier thresholds and migration policies
#[derive(Debug, Clone)]
pub struct TierConfig {
    /// Maximum size of hot tier in gigabytes
    pub hot_threshold_gb: u64,
    /// Maximum size of warm tier in gigabytes
    pub warm_threshold_gb: u64,
    /// Number of accesses required to promote to a hotter tier
    pub access_promote_threshold: u32,
}

impl Default for TierConfig {
    fn default() -> Self {
        Self {
            hot_threshold_gb: 10,
            warm_threshold_gb: 50,
            access_promote_threshold: 5,
        }
    }
}

impl TierConfig {
    /// Create a new tier configuration with custom values
    pub fn new(hot_threshold_gb: u64, warm_threshold_gb: u64, access_promote_threshold: u32) -> Self {
        Self {
            hot_threshold_gb,
            warm_threshold_gb,
            access_promote_threshold,
        }
    }
}

/// Manages storage tier migrations for memories
///
/// The TierManager handles moving memories between Hot, Warm, and Cold storage tiers
/// based on access patterns and storage constraints.
///
/// For v1:
/// - Hot and Warm tiers are logically different but use the same LanceDB table
/// - Cold tier will eventually use a separate archive table
/// - Currently all tiers use the same table with different tier field values
pub struct TierManager<'a> {
    store: &'a LanceStore,
    config: TierConfig,
}

impl<'a> TierManager<'a> {
    /// Create a new TierManager with the given store and default configuration
    pub fn new(store: &'a LanceStore) -> Self {
        Self {
            store,
            config: TierConfig::default(),
        }
    }

    /// Create a new TierManager with custom configuration
    pub fn with_config(store: &'a LanceStore, config: TierConfig) -> Self {
        Self { store, config }
    }

    /// Get the current configuration
    pub fn config(&self) -> &TierConfig {
        &self.config
    }

    /// Migrate a memory from one tier to another
    ///
    /// # Arguments
    /// * `memory_id` - The UUID of the memory to migrate
    /// * `from` - The expected current tier of the memory
    /// * `to` - The target tier to migrate to
    ///
    /// # Returns
    /// * `Ok(())` if migration was successful
    /// * `Err` if memory not found, tier mismatch, or storage error
    ///
    /// # Example
    /// ```ignore
    /// let manager = TierManager::new(&store);
    /// manager.migrate(memory_id, StorageTier::Hot, StorageTier::Warm).await?;
    /// ```
    pub async fn migrate(
        &self,
        memory_id: Uuid,
        from: StorageTier,
        to: StorageTier,
    ) -> Result<()> {
        let memory = self
            .store
            .get(memory_id)
            .await?
            .ok_or_else(|| NovaError::Memory(format!("Memory not found: {}", memory_id)))?;

        if memory.tier != from {
            return Err(NovaError::Memory(format!(
                "Tier mismatch: expected {:?}, found {:?}",
                from, memory.tier
            )));
        }

        if from == to {
            return Ok(());
        }

        self.store.update_tier(memory_id, to).await
    }

    /// Promote a memory to a hotter tier
    ///
    /// - Cold → Warm
    /// - Warm → Hot
    /// - Hot → (no-op, already hottest)
    ///
    /// This is typically called when a memory is accessed frequently,
    /// indicating it should be moved to a faster-access tier.
    pub async fn promote(&self, memory_id: Uuid) -> Result<()> {
        let memory = self
            .store
            .get(memory_id)
            .await?
            .ok_or_else(|| NovaError::Memory(format!("Memory not found: {}", memory_id)))?;

        let new_tier = match memory.tier {
            StorageTier::Cold => StorageTier::Warm,
            StorageTier::Warm => StorageTier::Hot,
            StorageTier::Hot => return Ok(()),
        };

        self.store.update_tier(memory_id, new_tier).await
    }

    /// Demote a memory to a cooler tier
    ///
    /// - Hot → Warm
    /// - Warm → Cold
    /// - Cold → (no-op, already coldest)
    ///
    /// This is typically called when storage space is needed or
    /// when a memory hasn't been accessed in a long time.
    pub async fn demote(&self, memory_id: Uuid) -> Result<()> {
        let memory = self
            .store
            .get(memory_id)
            .await?
            .ok_or_else(|| NovaError::Memory(format!("Memory not found: {}", memory_id)))?;

        let new_tier = match memory.tier {
            StorageTier::Hot => StorageTier::Warm,
            StorageTier::Warm => StorageTier::Cold,
            StorageTier::Cold => return Ok(()),
        };

        self.store.update_tier(memory_id, new_tier).await
    }

    /// Check if a memory should be promoted based on access count
    ///
    /// Returns true if the memory's access count exceeds the promotion threshold
    pub async fn should_promote(&self, memory_id: Uuid) -> Result<bool> {
        let memory = self
            .store
            .get(memory_id)
            .await?
            .ok_or_else(|| NovaError::Memory(format!("Memory not found: {}", memory_id)))?;

        if memory.tier == StorageTier::Hot {
            return Ok(false);
        }

        Ok(memory.access_count >= self.config.access_promote_threshold)
    }

    /// Get the current tier of a memory
    pub async fn get_tier(&self, memory_id: Uuid) -> Result<StorageTier> {
        let memory = self
            .store
            .get(memory_id)
            .await?
            .ok_or_else(|| NovaError::Memory(format!("Memory not found: {}", memory_id)))?;

        Ok(memory.tier)
    }

    /// Check and potentially promote a memory after access
    ///
    /// This is a convenience method that checks if a memory should be promoted
    /// based on access count and performs the promotion if needed.
    ///
    /// Call this after updating access stats on a memory.
    pub async fn check_and_promote(&self, memory_id: Uuid) -> Result<bool> {
        if self.should_promote(memory_id).await? {
            self.promote(memory_id).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::types::{Memory, MemorySource, MemoryType};

    fn create_test_memory(tier: StorageTier) -> Memory {
        let mut memory = Memory::new(
            "Test memory content".to_string(),
            vec![0.1; 384],
            MemoryType::Semantic,
            MemorySource::Manual,
        );
        memory.tier = tier;
        memory
    }

    fn create_test_memory_with_access_count(tier: StorageTier, access_count: u32) -> Memory {
        let mut memory = create_test_memory(tier);
        memory.access_count = access_count;
        memory
    }

    mod tier_config {
        use super::*;

        #[test]
        fn test_default_config() {
            let config = TierConfig::default();
            assert_eq!(config.hot_threshold_gb, 10);
            assert_eq!(config.warm_threshold_gb, 50);
            assert_eq!(config.access_promote_threshold, 5);
        }

        #[test]
        fn test_custom_config() {
            let config = TierConfig::new(20, 100, 10);
            assert_eq!(config.hot_threshold_gb, 20);
            assert_eq!(config.warm_threshold_gb, 100);
            assert_eq!(config.access_promote_threshold, 10);
        }
    }

    mod tier_manager {
        use super::*;

        #[tokio::test]
        async fn test_migrate_hot_to_warm() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory(StorageTier::Hot);
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
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory(StorageTier::Warm);
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
        async fn test_migrate_same_tier_is_noop() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory(StorageTier::Hot);
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
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory(StorageTier::Hot);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let manager = TierManager::new(&store);
            let result = manager
                .migrate(id, StorageTier::Warm, StorageTier::Cold)
                .await;

            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(matches!(err, NovaError::Memory(_)));
        }

        #[tokio::test]
        async fn test_migrate_nonexistent_memory() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let manager = TierManager::new(&store);
            let result = manager
                .migrate(Uuid::new_v4(), StorageTier::Hot, StorageTier::Warm)
                .await;

            assert!(result.is_err());
        }

        #[tokio::test]
        async fn test_promote_cold_to_warm() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory(StorageTier::Cold);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let manager = TierManager::new(&store);
            manager.promote(id).await.unwrap();

            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.tier, StorageTier::Warm);
        }

        #[tokio::test]
        async fn test_promote_warm_to_hot() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory(StorageTier::Warm);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let manager = TierManager::new(&store);
            manager.promote(id).await.unwrap();

            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.tier, StorageTier::Hot);
        }

        #[tokio::test]
        async fn test_promote_hot_is_noop() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory(StorageTier::Hot);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let manager = TierManager::new(&store);
            manager.promote(id).await.unwrap();

            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.tier, StorageTier::Hot);
        }

        #[tokio::test]
        async fn test_demote_hot_to_warm() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory(StorageTier::Hot);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let manager = TierManager::new(&store);
            manager.demote(id).await.unwrap();

            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.tier, StorageTier::Warm);
        }

        #[tokio::test]
        async fn test_demote_warm_to_cold() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory(StorageTier::Warm);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let manager = TierManager::new(&store);
            manager.demote(id).await.unwrap();

            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.tier, StorageTier::Cold);
        }

        #[tokio::test]
        async fn test_demote_cold_is_noop() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory(StorageTier::Cold);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let manager = TierManager::new(&store);
            manager.demote(id).await.unwrap();

            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.tier, StorageTier::Cold);
        }

        #[tokio::test]
        async fn test_get_tier() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory(StorageTier::Warm);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let manager = TierManager::new(&store);
            let tier = manager.get_tier(id).await.unwrap();
            assert_eq!(tier, StorageTier::Warm);
        }

        #[tokio::test]
        async fn test_should_promote_below_threshold() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory_with_access_count(StorageTier::Warm, 3);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let manager = TierManager::new(&store);
            let should_promote = manager.should_promote(id).await.unwrap();
            assert!(!should_promote);
        }

        #[tokio::test]
        async fn test_should_promote_at_threshold() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory_with_access_count(StorageTier::Warm, 5);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let manager = TierManager::new(&store);
            let should_promote = manager.should_promote(id).await.unwrap();
            assert!(should_promote);
        }

        #[tokio::test]
        async fn test_should_promote_already_hot() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory_with_access_count(StorageTier::Hot, 10);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let manager = TierManager::new(&store);
            let should_promote = manager.should_promote(id).await.unwrap();
            assert!(!should_promote);
        }

        #[tokio::test]
        async fn test_check_and_promote_triggers_promotion() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory_with_access_count(StorageTier::Cold, 10);
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
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory_with_access_count(StorageTier::Warm, 2);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let manager = TierManager::new(&store);
            let promoted = manager.check_and_promote(id).await.unwrap();
            assert!(!promoted);

            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.tier, StorageTier::Warm);
        }

        #[tokio::test]
        async fn test_memory_retrievable_after_migration() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory(StorageTier::Hot);
            let id = memory.id;
            let original_content = memory.content.clone();
            store.insert(&memory).await.unwrap();

            let manager = TierManager::new(&store);
            
            manager.demote(id).await.unwrap();
            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.tier, StorageTier::Warm);
            assert_eq!(updated.content, original_content);

            manager.demote(id).await.unwrap();
            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.tier, StorageTier::Cold);
            assert_eq!(updated.content, original_content);

            manager.promote(id).await.unwrap();
            manager.promote(id).await.unwrap();
            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.tier, StorageTier::Hot);
            assert_eq!(updated.content, original_content);
        }

        #[tokio::test]
        async fn test_tier_changes_tracked() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory(StorageTier::Hot);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let manager = TierManager::new(&store);
            
            assert_eq!(manager.get_tier(id).await.unwrap(), StorageTier::Hot);
            
            manager.demote(id).await.unwrap();
            assert_eq!(manager.get_tier(id).await.unwrap(), StorageTier::Warm);
            
            manager.demote(id).await.unwrap();
            assert_eq!(manager.get_tier(id).await.unwrap(), StorageTier::Cold);
            
            manager.promote(id).await.unwrap();
            assert_eq!(manager.get_tier(id).await.unwrap(), StorageTier::Warm);
            
            manager.promote(id).await.unwrap();
            assert_eq!(manager.get_tier(id).await.unwrap(), StorageTier::Hot);
        }

        #[tokio::test]
        async fn test_with_custom_config() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory_with_access_count(StorageTier::Warm, 3);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let manager = TierManager::new(&store);
            assert!(!manager.should_promote(id).await.unwrap());

            let custom_config = TierConfig::new(10, 50, 2);
            let manager = TierManager::with_config(&store, custom_config);
            assert!(manager.should_promote(id).await.unwrap());
        }
    }
}
