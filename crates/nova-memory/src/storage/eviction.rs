//! Capacity-based eviction for Nova Memory storage tiers
//!
//! Implements automatic eviction of low-priority memories when storage
//! capacity thresholds are exceeded.

use chrono::{Duration, Utc};
use uuid::Uuid;

use crate::error::Result;
use crate::memory::tombstone::{EvictionReason, Tombstone};
use crate::memory::types::{Memory, StorageTier};
use crate::memory::weight::{WeightConfig, calculate_effective_weight};
use crate::storage::LanceStore;

/// Configuration for eviction behavior
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvictionConfig {
    /// Capacity threshold to trigger warning (default: 0.70 = 70%)
    pub warning_threshold: f32,
    /// Capacity threshold to start eviction (default: 0.80 = 80%)
    pub eviction_threshold: f32,
    /// Capacity threshold for aggressive eviction (default: 0.95 = 95%)
    pub aggressive_threshold: f32,
    /// Hours since last access to consider memory "recently accessed" (default: 24)
    pub recent_access_hours: u64,
    /// Minimum weight to protect memory from eviction (default: 0.7)
    pub min_weight_protected: f32,
    /// Maximum number of memories per tier (used for capacity calculation)
    pub max_memories_per_tier: usize,
}

impl Default for EvictionConfig {
    fn default() -> Self {
        Self {
            warning_threshold: 0.70,
            eviction_threshold: 0.80,
            aggressive_threshold: 0.95,
            recent_access_hours: 24,
            min_weight_protected: 0.7,
            max_memories_per_tier: 10000,
        }
    }
}

impl EvictionConfig {
    pub fn new(
        warning_threshold: f32,
        eviction_threshold: f32,
        aggressive_threshold: f32,
        recent_access_hours: u64,
        min_weight_protected: f32,
        max_memories_per_tier: usize,
    ) -> Self {
        Self {
            warning_threshold,
            eviction_threshold,
            aggressive_threshold,
            recent_access_hours,
            min_weight_protected,
            max_memories_per_tier,
        }
    }
}

/// Result of capacity check indicating current storage state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapacityStatus {
    /// Below warning threshold - no action needed
    Normal,
    /// Above warning threshold but below eviction threshold
    Warning,
    /// Above eviction threshold - eviction should occur
    EvictionNeeded,
    /// Above aggressive threshold - urgent eviction required
    AggressiveEvictionNeeded,
}

/// Manages eviction of low-priority memories from storage tiers
pub struct Evictor<'a> {
    store: &'a LanceStore,
    config: EvictionConfig,
    weight_config: WeightConfig,
}

impl<'a> Evictor<'a> {
    pub fn new(store: &'a LanceStore) -> Self {
        Self {
            store,
            config: EvictionConfig::default(),
            weight_config: WeightConfig::default(),
        }
    }

    pub fn with_config(store: &'a LanceStore, config: EvictionConfig) -> Self {
        Self {
            store,
            config,
            weight_config: WeightConfig::default(),
        }
    }

    pub fn with_configs(
        store: &'a LanceStore,
        config: EvictionConfig,
        weight_config: WeightConfig,
    ) -> Self {
        Self {
            store,
            config,
            weight_config,
        }
    }

    pub fn config(&self) -> &EvictionConfig {
        &self.config
    }

    /// Calculate eviction priority for a memory.
    /// Higher score = higher priority to KEEP (don't evict).
    /// Lower score = lower priority = evict first.
    ///
    /// Priority formula:
    /// - effective_weight: Base importance considering decay and access patterns
    /// - recency_bonus: Recently accessed memories get bonus (0.0 to 0.3)
    /// - association_bonus: Placeholder for future association strength (currently 0)
    pub fn eviction_priority(&self, memory: &Memory) -> f32 {
        let effective_weight = calculate_effective_weight(memory, &self.weight_config);

        let hours_since_access = (Utc::now() - memory.last_accessed).num_hours().max(0) as f32;
        // Recency bonus: 1.0 / (1.0 + hours/24) gives ~0.5 at 24h, ~0.33 at 48h
        // Multiply by 0.3 to cap at 0.3 bonus for very recent access
        let recency_bonus = 0.3 / (1.0 + hours_since_access / 24.0);

        // Placeholder for association bonus - will be implemented with association graph
        let association_bonus = 0.0;

        effective_weight + recency_bonus + association_bonus
    }

    /// Check if a memory is protected from eviction.
    /// Protected memories should not be evicted regardless of priority.
    pub fn is_protected(&self, memory: &Memory) -> bool {
        // Protect memories accessed within recent_access_hours
        let recent_threshold = Utc::now() - Duration::hours(self.config.recent_access_hours as i64);
        if memory.last_accessed > recent_threshold {
            return true;
        }

        // Protect memories with high base weight (owner importance)
        if memory.weight >= self.config.min_weight_protected {
            return true;
        }

        false
    }

    /// Check the current capacity status of a tier
    pub async fn check_capacity(&self, tier: StorageTier) -> Result<CapacityStatus> {
        let count = self.store.count_by_tier(tier).await?;
        let capacity_ratio = count as f32 / self.config.max_memories_per_tier as f32;

        if capacity_ratio >= self.config.aggressive_threshold {
            Ok(CapacityStatus::AggressiveEvictionNeeded)
        } else if capacity_ratio >= self.config.eviction_threshold {
            Ok(CapacityStatus::EvictionNeeded)
        } else if capacity_ratio >= self.config.warning_threshold {
            Ok(CapacityStatus::Warning)
        } else {
            Ok(CapacityStatus::Normal)
        }
    }

    /// Get the current capacity ratio for a tier (0.0 to 1.0+)
    pub async fn capacity_ratio(&self, tier: StorageTier) -> Result<f32> {
        let count = self.store.count_by_tier(tier).await?;
        Ok(count as f32 / self.config.max_memories_per_tier as f32)
    }

    /// Evict memories if needed based on current capacity.
    /// Returns the UUIDs of evicted memories.
    ///
    /// Eviction process:
    /// 1. Check if eviction is needed (capacity >= eviction_threshold)
    /// 2. Get all memories in the tier
    /// 3. Filter out protected memories
    /// 4. Sort by eviction priority (lowest first)
    /// 5. Create tombstones and delete lowest priority memories until below eviction threshold
    pub async fn evict_if_needed(&self, tier: StorageTier) -> Result<Vec<Uuid>> {
        let status = self.check_capacity(tier).await?;

        match status {
            CapacityStatus::Normal | CapacityStatus::Warning => {
                return Ok(Vec::new());
            }
            CapacityStatus::EvictionNeeded | CapacityStatus::AggressiveEvictionNeeded => {
                // Continue with eviction
            }
        }

        let target_ratio = if status == CapacityStatus::AggressiveEvictionNeeded {
            // Aggressive: evict down to warning threshold
            self.config.warning_threshold
        } else {
            // Normal: evict down to just below eviction threshold
            self.config.eviction_threshold - 0.05
        };

        let current_count = self.store.count_by_tier(tier).await?;
        let target_count = (self.config.max_memories_per_tier as f32 * target_ratio) as usize;

        if current_count <= target_count {
            return Ok(Vec::new());
        }

        let to_evict_count = current_count - target_count;

        let memories = self.store.list_by_tier(tier).await?;

        // Calculate priority and filter protected
        let mut candidates: Vec<(Memory, f32)> = memories
            .into_iter()
            .filter(|m| !self.is_protected(m))
            .map(|m| {
                let priority = self.eviction_priority(&m);
                (m, priority)
            })
            .collect();

        // Sort by priority ascending (lowest priority = evict first)
        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Determine eviction reason based on status
        let reason = if status == CapacityStatus::AggressiveEvictionNeeded {
            EvictionReason::StoragePressure
        } else {
            EvictionReason::LowWeight
        };

        let mut evicted = Vec::new();
        for (memory, _priority) in candidates.into_iter().take(to_evict_count) {
            // Create tombstone and then delete
            if self.evict_with_tombstone(&memory, reason.clone()).await? {
                evicted.push(memory.id);
            }
        }

        Ok(evicted)
    }

    /// Get eviction candidates sorted by priority (lowest first).
    /// Does not actually evict - useful for previewing what would be evicted.
    pub async fn get_eviction_candidates(
        &self,
        tier: StorageTier,
        limit: usize,
    ) -> Result<Vec<(Memory, f32)>> {
        let memories = self.store.list_by_tier(tier).await?;

        let mut candidates: Vec<(Memory, f32)> = memories
            .into_iter()
            .filter(|m| !self.is_protected(m))
            .map(|m| {
                let priority = self.eviction_priority(&m);
                (m, priority)
            })
            .collect();

        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        Ok(candidates.into_iter().take(limit).collect())
    }

    /// Create a tombstone for a memory before eviction.
    /// Extracts topics from entities and creates a tombstone record.
    async fn create_tombstone(&self, memory: &Memory, reason: EvictionReason) -> Result<Tombstone> {
        // Extract topics from entities (all entities are treated as topics for now)
        let topics = memory.entities.clone();

        // Participants are not currently stored separately in Memory,
        // so we leave it empty. In the future, if Memory stores labeled entities,
        // we can filter for Person entities here.
        let participants = Vec::new();

        let tombstone = Tombstone {
            original_id: memory.id,
            evicted_at: Utc::now(),
            topics,
            participants,
            approximate_date: memory.created_at,
            reason,
        };

        self.store.insert_tombstone(&tombstone).await?;
        Ok(tombstone)
    }

    /// Create a tombstone and evict a memory.
    /// This is the preferred method for eviction as it preserves metadata.
    async fn evict_with_tombstone(&self, memory: &Memory, reason: EvictionReason) -> Result<bool> {
        // Create tombstone first (before deleting)
        self.create_tombstone(memory, reason).await?;

        // Then delete the memory
        self.store.delete(memory.id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::types::{MemorySource, MemoryType};
    use chrono::Duration;

    fn create_test_memory(content: &str, weight: f32, tier: StorageTier) -> Memory {
        let mut memory = Memory::new(
            content.to_string(),
            vec![0.1; 384],
            MemoryType::Semantic,
            MemorySource::Manual,
        );
        memory.weight = weight;
        memory.tier = tier;
        memory
    }

    fn create_memory_with_access(
        content: &str,
        weight: f32,
        tier: StorageTier,
        hours_ago: i64,
        access_count: u32,
    ) -> Memory {
        let mut memory = create_test_memory(content, weight, tier);
        memory.last_accessed = Utc::now() - Duration::hours(hours_ago);
        memory.access_count = access_count;
        memory
    }

    mod eviction_config {
        use super::*;

        #[test]
        fn test_default_config() {
            let config = EvictionConfig::default();
            assert_eq!(config.warning_threshold, 0.70);
            assert_eq!(config.eviction_threshold, 0.80);
            assert_eq!(config.aggressive_threshold, 0.95);
            assert_eq!(config.recent_access_hours, 24);
            assert_eq!(config.min_weight_protected, 0.7);
            assert_eq!(config.max_memories_per_tier, 10000);
        }

        #[test]
        fn test_custom_config() {
            let config = EvictionConfig::new(0.60, 0.75, 0.90, 12, 0.8, 5000);
            assert_eq!(config.warning_threshold, 0.60);
            assert_eq!(config.eviction_threshold, 0.75);
            assert_eq!(config.aggressive_threshold, 0.90);
            assert_eq!(config.recent_access_hours, 12);
            assert_eq!(config.min_weight_protected, 0.8);
            assert_eq!(config.max_memories_per_tier, 5000);
        }
    }

    mod eviction_priority {
        use super::*;

        #[tokio::test]
        async fn test_priority_higher_for_recent_access() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let evictor = Evictor::new(&store);

            let recent = create_memory_with_access("Recent", 0.5, StorageTier::Hot, 1, 0);
            let old = create_memory_with_access("Old", 0.5, StorageTier::Hot, 100, 0);

            let recent_priority = evictor.eviction_priority(&recent);
            let old_priority = evictor.eviction_priority(&old);

            assert!(
                recent_priority > old_priority,
                "Recently accessed memory should have higher priority (keep), recent={recent_priority}, old={old_priority}"
            );
        }

        #[tokio::test]
        async fn test_priority_higher_for_higher_weight() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let evictor = Evictor::new(&store);

            let high_weight = create_memory_with_access("High", 0.9, StorageTier::Hot, 50, 0);
            let low_weight = create_memory_with_access("Low", 0.2, StorageTier::Hot, 50, 0);

            let high_priority = evictor.eviction_priority(&high_weight);
            let low_priority = evictor.eviction_priority(&low_weight);

            assert!(
                high_priority > low_priority,
                "Higher weight memory should have higher priority (keep)"
            );
        }

        #[tokio::test]
        async fn test_priority_higher_for_more_access() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let evictor = Evictor::new(&store);

            let frequently_accessed =
                create_memory_with_access("Frequent", 0.5, StorageTier::Hot, 50, 100);
            let rarely_accessed = create_memory_with_access("Rare", 0.5, StorageTier::Hot, 50, 1);

            let frequent_priority = evictor.eviction_priority(&frequently_accessed);
            let rare_priority = evictor.eviction_priority(&rarely_accessed);

            assert!(
                frequent_priority > rare_priority,
                "Frequently accessed memory should have higher priority (keep)"
            );
        }
    }

    mod protection {
        use super::*;

        #[tokio::test]
        async fn test_recently_accessed_is_protected() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let evictor = Evictor::new(&store);

            // Accessed 1 hour ago (within 24h default)
            let recent = create_memory_with_access("Recent", 0.3, StorageTier::Hot, 1, 0);
            assert!(
                evictor.is_protected(&recent),
                "Memory accessed within 24h should be protected"
            );

            // Accessed 48 hours ago (outside 24h default)
            let old = create_memory_with_access("Old", 0.3, StorageTier::Hot, 48, 0);
            assert!(
                !evictor.is_protected(&old),
                "Memory accessed 48h ago should not be protected by recency"
            );
        }

        #[tokio::test]
        async fn test_high_weight_is_protected() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let evictor = Evictor::new(&store);

            // High weight (above 0.7 default threshold), old access
            let high_weight = create_memory_with_access("High", 0.8, StorageTier::Hot, 100, 0);
            assert!(
                evictor.is_protected(&high_weight),
                "High weight memory should be protected"
            );

            // Low weight, old access
            let low_weight = create_memory_with_access("Low", 0.3, StorageTier::Hot, 100, 0);
            assert!(
                !evictor.is_protected(&low_weight),
                "Low weight old memory should not be protected"
            );
        }

        #[tokio::test]
        async fn test_custom_protection_thresholds() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let config = EvictionConfig {
                recent_access_hours: 12,
                min_weight_protected: 0.5,
                ..EvictionConfig::default()
            };
            let evictor = Evictor::with_config(&store, config);

            // Accessed 6 hours ago (within 12h threshold)
            let recent = create_memory_with_access("Recent", 0.3, StorageTier::Hot, 6, 0);
            assert!(evictor.is_protected(&recent));

            // Accessed 20 hours ago (outside 12h threshold)
            let old = create_memory_with_access("Old", 0.3, StorageTier::Hot, 20, 0);
            assert!(!evictor.is_protected(&old));

            // Weight 0.6 (above 0.5 threshold)
            let medium_weight = create_memory_with_access("Medium", 0.6, StorageTier::Hot, 100, 0);
            assert!(evictor.is_protected(&medium_weight));
        }
    }

    mod capacity {
        use super::*;

        #[tokio::test]
        async fn test_capacity_status_normal() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            // Insert 50 memories into a tier with max 100 = 50% capacity
            let config = EvictionConfig {
                max_memories_per_tier: 100,
                ..EvictionConfig::default()
            };
            let evictor = Evictor::with_config(&store, config);

            let memories: Vec<Memory> = (0..50)
                .map(|i| create_test_memory(&format!("Memory {i}"), 0.5, StorageTier::Hot))
                .collect();
            store.insert_batch(&memories).await.unwrap();

            let status = evictor.check_capacity(StorageTier::Hot).await.unwrap();
            assert_eq!(status, CapacityStatus::Normal);
        }

        #[tokio::test]
        async fn test_capacity_status_warning() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            // Insert 75 memories into a tier with max 100 = 75% capacity
            let config = EvictionConfig {
                max_memories_per_tier: 100,
                ..EvictionConfig::default()
            };
            let evictor = Evictor::with_config(&store, config);

            let memories: Vec<Memory> = (0..75)
                .map(|i| create_test_memory(&format!("Memory {i}"), 0.5, StorageTier::Hot))
                .collect();
            store.insert_batch(&memories).await.unwrap();

            let status = evictor.check_capacity(StorageTier::Hot).await.unwrap();
            assert_eq!(status, CapacityStatus::Warning);
        }

        #[tokio::test]
        async fn test_capacity_status_eviction_needed() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            // Insert 85 memories into a tier with max 100 = 85% capacity
            let config = EvictionConfig {
                max_memories_per_tier: 100,
                ..EvictionConfig::default()
            };
            let evictor = Evictor::with_config(&store, config);

            let memories: Vec<Memory> = (0..85)
                .map(|i| create_test_memory(&format!("Memory {i}"), 0.5, StorageTier::Hot))
                .collect();
            store.insert_batch(&memories).await.unwrap();

            let status = evictor.check_capacity(StorageTier::Hot).await.unwrap();
            assert_eq!(status, CapacityStatus::EvictionNeeded);
        }

        #[tokio::test]
        async fn test_capacity_status_aggressive() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            // Insert 96 memories into a tier with max 100 = 96% capacity
            let config = EvictionConfig {
                max_memories_per_tier: 100,
                ..EvictionConfig::default()
            };
            let evictor = Evictor::with_config(&store, config);

            let memories: Vec<Memory> = (0..96)
                .map(|i| create_test_memory(&format!("Memory {i}"), 0.5, StorageTier::Hot))
                .collect();
            store.insert_batch(&memories).await.unwrap();

            let status = evictor.check_capacity(StorageTier::Hot).await.unwrap();
            assert_eq!(status, CapacityStatus::AggressiveEvictionNeeded);
        }
    }

    mod eviction {
        use super::*;

        #[tokio::test]
        async fn test_evict_if_needed_below_threshold() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let config = EvictionConfig {
                max_memories_per_tier: 100,
                ..EvictionConfig::default()
            };
            let evictor = Evictor::with_config(&store, config);

            // Insert 50 memories = 50% capacity (below 80% threshold)
            let memories: Vec<Memory> = (0..50)
                .map(|i| {
                    create_memory_with_access(&format!("Memory {i}"), 0.3, StorageTier::Hot, 100, 0)
                })
                .collect();
            store.insert_batch(&memories).await.unwrap();

            let evicted = evictor.evict_if_needed(StorageTier::Hot).await.unwrap();
            assert!(
                evicted.is_empty(),
                "No eviction should occur below threshold"
            );
        }

        #[tokio::test]
        async fn test_evict_lowest_priority_first() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();
            store.create_tombstones_table().await.unwrap();

            let config = EvictionConfig {
                max_memories_per_tier: 10,
                recent_access_hours: 1, // Very short to not protect by recency
                min_weight_protected: 0.9, // High threshold to not protect by weight
                ..EvictionConfig::default()
            };
            let evictor = Evictor::with_config(&store, config);

            // Insert memories with different weights (all old access, low weight)
            let mut memories = Vec::new();
            for i in 0..9 {
                let weight = 0.1 + (i as f32) * 0.05; // 0.1, 0.15, 0.20, ...
                let mem =
                    create_memory_with_access(&format!("Mem-{i}"), weight, StorageTier::Hot, 48, 0);
                memories.push(mem);
            }
            store.insert_batch(&memories).await.unwrap();

            // Get initial count
            let initial_count = store.count_by_tier(StorageTier::Hot).await.unwrap();
            assert_eq!(initial_count, 9);

            // At 90% capacity, eviction should occur
            let evicted = evictor.evict_if_needed(StorageTier::Hot).await.unwrap();

            // Should have evicted some memories
            assert!(!evicted.is_empty(), "Should have evicted some memories");

            let final_count = store.count_by_tier(StorageTier::Hot).await.unwrap();
            assert!(
                final_count < initial_count,
                "Count should decrease after eviction"
            );
        }

        #[tokio::test]
        async fn test_protected_memories_not_evicted() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();
            store.create_tombstones_table().await.unwrap();

            let config = EvictionConfig {
                max_memories_per_tier: 10,
                recent_access_hours: 24,
                min_weight_protected: 0.7,
                ..EvictionConfig::default()
            };
            let evictor = Evictor::with_config(&store, config);

            // Insert 9 memories at 90% capacity
            // 3 protected by recency (accessed 1h ago)
            // 3 protected by weight (weight 0.8)
            // 3 unprotected (old access, low weight)
            let mut memories = Vec::new();

            // Protected by recency
            for i in 0..3 {
                memories.push(create_memory_with_access(
                    &format!("Recent-{i}"),
                    0.3,
                    StorageTier::Hot,
                    1, // Recently accessed
                    0,
                ));
            }

            // Protected by weight
            for i in 0..3 {
                memories.push(create_memory_with_access(
                    &format!("Heavy-{i}"),
                    0.8, // High weight
                    StorageTier::Hot,
                    100,
                    0,
                ));
            }

            // Unprotected
            for i in 0..3 {
                memories.push(create_memory_with_access(
                    &format!("Evictable-{i}"),
                    0.2, // Low weight
                    StorageTier::Hot,
                    100, // Old access
                    0,
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
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

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
                let mem = create_memory_with_access(
                    &format!("Candidate-{i}"),
                    weight,
                    StorageTier::Hot,
                    48,
                    0,
                );
                memories.push(mem);
            }
            store.insert_batch(&memories).await.unwrap();

            let candidates = evictor
                .get_eviction_candidates(StorageTier::Hot, 3)
                .await
                .unwrap();

            assert_eq!(
                candidates.len(),
                3,
                "Should return requested number of candidates"
            );

            // Verify candidates are sorted by priority (lowest first)
            for i in 0..candidates.len() - 1 {
                assert!(
                    candidates[i].1 <= candidates[i + 1].1,
                    "Candidates should be sorted by priority ascending"
                );
            }
        }
    }

    mod tombstone_creation {
        use super::*;
        use crate::memory::tombstone::EvictionReason;

        fn create_memory_with_entities(
            content: &str,
            weight: f32,
            tier: StorageTier,
            hours_ago: i64,
            entities: Vec<String>,
        ) -> Memory {
            let mut memory = create_memory_with_access(content, weight, tier, hours_ago, 0);
            memory.entities = entities;
            memory
        }

        #[tokio::test]
        async fn test_eviction_creates_tombstone() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();
            store.create_tombstones_table().await.unwrap();

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
                let mem = create_memory_with_entities(
                    &format!("Evictable-{i}"),
                    0.1 + (i as f32) * 0.05,
                    StorageTier::Hot,
                    48,
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
            assert!(!evicted.is_empty(), "Should have evicted some memories");

            // Verify tombstones were created
            let tombstones_after = store.list_all_tombstones().await.unwrap();
            assert_eq!(
                tombstones_after.len(),
                evicted.len(),
                "Should have created a tombstone for each evicted memory"
            );

            // Verify tombstone content
            for tombstone in &tombstones_after {
                assert!(!tombstone.topics.is_empty(), "Tombstone should have topics");
                assert!(
                    tombstone.topics.contains(&"shared-topic".to_string()),
                    "Tombstone should contain shared topic"
                );
                assert!(
                    matches!(
                        tombstone.reason,
                        EvictionReason::StoragePressure | EvictionReason::LowWeight
                    ),
                    "Tombstone should have correct eviction reason"
                );
            }
        }

        #[tokio::test]
        async fn test_tombstone_contains_correct_topics() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();
            store.create_tombstones_table().await.unwrap();

            let config = EvictionConfig {
                max_memories_per_tier: 10,
                recent_access_hours: 1,
                min_weight_protected: 0.9,
                ..EvictionConfig::default()
            };
            let evictor = Evictor::with_config(&store, config);

            // Create memory with specific entities
            let mut memory = create_memory_with_entities(
                "Test content",
                0.1,
                StorageTier::Hot,
                48,
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
                let mem = create_memory_with_access(
                    &format!("Filler-{i}"),
                    0.1 + (i as f32) * 0.01,
                    StorageTier::Hot,
                    48,
                    0,
                );
                store.insert(&mem).await.unwrap();
            }

            // Trigger eviction
            let evicted = evictor.evict_if_needed(StorageTier::Hot).await.unwrap();
            assert!(!evicted.is_empty());

            // Find the tombstone for our specific memory
            let tombstone = store.get_tombstone(memory_id).await.unwrap();
            assert!(
                tombstone.is_some(),
                "Should have created a tombstone for the memory"
            );

            let tombstone = tombstone.unwrap();
            assert_eq!(tombstone.topics.len(), 3);
            assert!(tombstone.topics.contains(&"rust".to_string()));
            assert!(tombstone.topics.contains(&"programming".to_string()));
            assert!(tombstone.topics.contains(&"async".to_string()));
        }

        #[tokio::test]
        async fn test_tombstone_searchable_by_topic() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();
            store.create_tombstones_table().await.unwrap();

            let config = EvictionConfig {
                max_memories_per_tier: 10,
                recent_access_hours: 1,
                min_weight_protected: 0.9,
                ..EvictionConfig::default()
            };
            let evictor = Evictor::with_config(&store, config);

            // Create memories with different topics (low weights to ensure eviction)
            let mut memory1 = create_memory_with_entities(
                "Memory about machine learning",
                0.05, // Very low weight to ensure eviction
                StorageTier::Hot,
                48,
                vec!["machine-learning".to_string(), "python".to_string()],
            );
            memory1.id = Uuid::new_v4();

            let mut memory2 = create_memory_with_entities(
                "Memory about web development",
                0.06, // Very low weight to ensure eviction
                StorageTier::Hot,
                48,
                vec!["web".to_string(), "javascript".to_string()],
            );
            memory2.id = Uuid::new_v4();

            store.insert(&memory1).await.unwrap();
            store.insert(&memory2).await.unwrap();

            // Add more memories to trigger aggressive eviction (need > 95% for multiple evictions)
            for i in 0..8 {
                let mem = create_memory_with_access(
                    &format!("Filler-{i}"),
                    0.5 + (i as f32) * 0.01, // Higher weights to avoid eviction
                    StorageTier::Hot,
                    48,
                    0,
                );
                store.insert(&mem).await.unwrap();
            }

            // Trigger eviction - at 10/10 = 100% capacity, should evict aggressively
            let evicted = evictor.evict_if_needed(StorageTier::Hot).await.unwrap();
            assert!(
                evicted.len() >= 2,
                "Should evict at least 2 memories, evicted: {:?}",
                evicted.len()
            );

            // Search tombstones by topic
            let ml_tombstones = store
                .search_tombstones_by_topic("machine-learning")
                .await
                .unwrap();
            assert!(
                !ml_tombstones.is_empty(),
                "Should find tombstone by 'machine-learning' topic"
            );

            let python_tombstones = store.search_tombstones_by_topic("python").await.unwrap();
            assert!(
                !python_tombstones.is_empty(),
                "Should find tombstone by 'python' topic"
            );

            let web_tombstones = store.search_tombstones_by_topic("web").await.unwrap();
            assert!(
                !web_tombstones.is_empty(),
                "Should find tombstone by 'web' topic"
            );

            let nonexistent = store
                .search_tombstones_by_topic("nonexistent")
                .await
                .unwrap();
            assert!(
                nonexistent.is_empty(),
                "Should not find tombstones for nonexistent topic"
            );
        }

        #[tokio::test]
        async fn test_tombstone_has_approximate_date() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();
            store.create_tombstones_table().await.unwrap();

            let config = EvictionConfig {
                max_memories_per_tier: 10,
                recent_access_hours: 1,
                min_weight_protected: 0.9,
                ..EvictionConfig::default()
            };
            let evictor = Evictor::with_config(&store, config);

            // Create memory with specific creation date
            let mut memory = create_memory_with_entities(
                "Test content",
                0.1,
                StorageTier::Hot,
                48,
                vec!["test-topic".to_string()],
            );
            memory.id = Uuid::new_v4();
            let created_at = memory.created_at;
            let memory_id = memory.id;

            store.insert(&memory).await.unwrap();

            // Add more memories to trigger eviction
            for i in 0..8 {
                let mem = create_memory_with_access(
                    &format!("Filler-{i}"),
                    0.1 + (i as f32) * 0.01,
                    StorageTier::Hot,
                    48,
                    0,
                );
                store.insert(&mem).await.unwrap();
            }

            // Trigger eviction
            let evicted = evictor.evict_if_needed(StorageTier::Hot).await.unwrap();
            assert!(!evicted.is_empty());

            // Verify tombstone has correct approximate date
            let tombstone = store.get_tombstone(memory_id).await.unwrap().unwrap();
            assert_eq!(tombstone.approximate_date, created_at);
        }
    }
}
