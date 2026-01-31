//! Memory types and operations
//!
//! Defines core memory structures and operations for storing
//! and retrieving semantic memories across different tiers.

pub mod ingestion;
pub mod tombstone;
pub mod types;
pub mod weight;

pub use ingestion::IngestionPipeline;
pub use tombstone::{EvictionReason, Tombstone};
pub use types::{CompressionLevel, Memory, MemorySource, MemoryType, StorageTier};
pub use weight::{calculate_effective_weight, calculate_initial_weight, WeightConfig};
