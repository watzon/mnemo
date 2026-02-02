//! Curator module for intelligent memory extraction
//!
//! The curator analyzes conversations using LLMs to extract meaningful
//! memories, determine importance, and identify what should be stored.

pub mod buffer;
#[cfg(feature = "curator-local")]
pub mod hybrid;
#[cfg(feature = "curator-local")]
pub mod local;
pub mod prompts;
pub mod provider;
pub mod remote;
pub mod types;

pub use buffer::{ConversationBuffer, ConversationTurn, Role};
#[cfg(feature = "curator-local")]
pub use hybrid::HybridCurator;
#[cfg(feature = "curator-local")]
pub use local::LocalCurator;
pub use provider::CuratorProvider;
pub use remote::RemoteCurator;
pub use types::{CuratedMemory, CurationResult, CuratorError};
