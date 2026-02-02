//! Curator module for intelligent memory extraction
//!
//! The curator analyzes conversations using LLMs to extract meaningful
//! memories, determine importance, and identify what should be stored.

pub mod buffer;
pub mod provider;
pub mod types;

pub use buffer::{ConversationBuffer, ConversationTurn, Role};
pub use provider::CuratorProvider;
pub use types::{CuratedMemory, CurationResult, CuratorError};
