//! Curator provider trait for memory extraction
//!
//! Defines the CuratorProvider trait that abstracts different curation
//! backends (local LLM, remote API, hybrid approaches).

use async_trait::async_trait;
use crate::curator::types::{CurationResult, CuratorError};

/// Trait for curator providers (local LLM, remote API, hybrid)
///
/// Implementations handle the actual curation logic, analyzing conversations
/// and deciding what memories to extract and store.
#[async_trait]
pub trait CuratorProvider: Send + Sync {
    /// Analyze a conversation and decide what to store
    ///
    /// Takes a conversation string (formatted as needed by the provider)
    /// and returns a CurationResult indicating what memories should be stored.
    async fn curate(&self, conversation: &str) -> Result<CurationResult, CuratorError>;

    /// Check if the provider is available (model loaded, API reachable, etc.)
    ///
    /// Returns true if the provider can handle curation requests.
    async fn is_available(&self) -> bool;

    /// Provider name for logging
    ///
    /// Returns a static string identifier for this provider type.
    fn name(&self) -> &'static str;
}
