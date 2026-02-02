//! Injection tracking for memory deduplication
//!
//! Tracks which memories have been injected into the current session
//! to prevent duplicate injections. Uses an LRU cache with configurable
//! capacity for session-scoped deduplication.

use lru::LruCache;
use std::num::NonZeroUsize;
use uuid::Uuid;

/// Default capacity for the injection tracker cache
pub const DEFAULT_TRACKER_CAPACITY: usize = 1000;

/// Tracks memory IDs that have been injected in the current session
///
/// Uses an LRU cache to limit memory usage while preventing duplicate
/// injections of the same memory within a session.
#[derive(Debug)]
pub struct InjectionTracker {
    cache: LruCache<Uuid, ()>,
}

impl InjectionTracker {
    /// Creates a new injection tracker with the specified capacity
    ///
    /// # Arguments
    /// * `capacity` - Maximum number of memory IDs to track
    ///
    /// # Panics
    /// Panics if capacity is 0 (use at least 1)
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity)
            .unwrap_or_else(|| NonZeroUsize::new(DEFAULT_TRACKER_CAPACITY).unwrap());
        Self {
            cache: LruCache::new(cap),
        }
    }

    /// Marks a memory ID as having been injected
    ///
    /// If the cache is at capacity, the least recently used entry
    /// will be evicted to make room.
    pub fn mark_injected(&mut self, id: Uuid) {
        self.cache.put(id, ());
    }

    /// Checks if a memory ID has been injected
    ///
    /// Returns true if the ID is in the cache (was previously injected).
    /// Accessing an entry updates its recency in the LRU.
    pub fn was_injected(&mut self, id: &Uuid) -> bool {
        self.cache.get(id).is_some()
    }

    /// Clears all tracked injection records
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Returns the number of tracked memory IDs
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Returns true if no memory IDs are being tracked
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Returns the maximum capacity of the tracker
    pub fn capacity(&self) -> usize {
        self.cache.cap().get()
    }
}

impl Default for InjectionTracker {
    fn default() -> Self {
        Self::new(DEFAULT_TRACKER_CAPACITY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tracker_is_empty() {
        let tracker = InjectionTracker::new(100);
        assert!(tracker.is_empty());
        assert_eq!(tracker.len(), 0);
        assert_eq!(tracker.capacity(), 100);
    }

    #[test]
    fn test_default_capacity() {
        let tracker = InjectionTracker::default();
        assert_eq!(tracker.capacity(), DEFAULT_TRACKER_CAPACITY);
    }

    #[test]
    fn test_mark_and_check_injected() {
        let mut tracker = InjectionTracker::new(100);
        let id = Uuid::new_v4();

        // Initially not injected
        assert!(!tracker.was_injected(&id));

        // Mark as injected
        tracker.mark_injected(id);

        // Now should be marked as injected
        assert!(tracker.was_injected(&id));
        assert_eq!(tracker.len(), 1);
    }

    #[test]
    fn test_multiple_ids() {
        let mut tracker = InjectionTracker::new(100);
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();

        tracker.mark_injected(id1);
        tracker.mark_injected(id2);

        assert!(tracker.was_injected(&id1));
        assert!(tracker.was_injected(&id2));
        assert!(!tracker.was_injected(&id3));
        assert_eq!(tracker.len(), 2);
    }

    #[test]
    fn test_lru_eviction() {
        let mut tracker = InjectionTracker::new(3);
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        let id4 = Uuid::new_v4();

        // Fill to capacity
        tracker.mark_injected(id1);
        tracker.mark_injected(id2);
        tracker.mark_injected(id3);
        assert_eq!(tracker.len(), 3);

        // Access id1 to make it recently used
        assert!(tracker.was_injected(&id1));

        // Add id4, should evict id2 (least recently used)
        tracker.mark_injected(id4);
        assert_eq!(tracker.len(), 3);

        // id1 should still be present (was accessed recently)
        assert!(tracker.was_injected(&id1));
        // id2 should be evicted
        assert!(!tracker.was_injected(&id2));
        // id3 and id4 should be present
        assert!(tracker.was_injected(&id3));
        assert!(tracker.was_injected(&id4));
    }

    #[test]
    fn test_clear() {
        let mut tracker = InjectionTracker::new(100);
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        tracker.mark_injected(id1);
        tracker.mark_injected(id2);
        assert_eq!(tracker.len(), 2);

        tracker.clear();
        assert!(tracker.is_empty());
        assert_eq!(tracker.len(), 0);
        assert!(!tracker.was_injected(&id1));
        assert!(!tracker.was_injected(&id2));
    }

    #[test]
    fn test_remark_injected_updates_recency() {
        let mut tracker = InjectionTracker::new(2);
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();

        tracker.mark_injected(id1);
        tracker.mark_injected(id2);

        // Re-mark id1 to update its recency
        tracker.mark_injected(id1);

        // Add id3, should evict id2 (now least recently used)
        tracker.mark_injected(id3);

        assert!(tracker.was_injected(&id1));
        assert!(!tracker.was_injected(&id2));
        assert!(tracker.was_injected(&id3));
    }

    #[test]
    fn test_zero_capacity_uses_default() {
        let tracker = InjectionTracker::new(0);
        assert_eq!(tracker.capacity(), DEFAULT_TRACKER_CAPACITY);
    }
}
