//! Memory types and operations
//!
//! Defines core memory structures and operations for storing
//! and retrieving semantic memories across different tiers.

pub mod ingestion;
pub mod injection_tracker;
pub mod retrieval;
pub mod tombstone;
pub mod types;
pub mod weight;

pub use ingestion::IngestionPipeline;
pub use injection_tracker::{InjectionTracker, DEFAULT_TRACKER_CAPACITY};
pub use retrieval::{RetrievalConfig, RetrievalPipeline, RetrievedMemory};
pub use tombstone::{EvictionReason, Tombstone};
pub use types::{CompressionLevel, Memory, MemorySource, MemoryType, StorageTier};
pub use weight::{WeightConfig, calculate_effective_weight, calculate_initial_weight};
