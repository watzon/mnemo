//! Admin module for TUI dashboard and monitoring
//!
//! Provides shared types for real-time event streaming and statistics
//! between the daemon and admin clients.

pub mod handlers;

use crate::memory::types::Memory;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Events emitted by the proxy for real-time monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProxyEvent {
    /// A new request has started processing
    RequestStarted {
        request_id: String,
        method: String,
        path: String,
        provider: String,
        timestamp: DateTime<Utc>,
    },
    /// Memories were injected into a request
    MemoriesInjected {
        request_id: String,
        memory_ids: Vec<String>,
        count: usize,
    },
    /// A request has completed
    RequestCompleted {
        request_id: String,
        status: u16,
        latency_ms: u64,
        bytes: Option<u64>,
    },
    /// A new memory was ingested
    MemoryIngested {
        memory_id: String,
        memory_type: String,
        content_preview: String,
    },
    /// Periodic heartbeat with current stats
    Heartbeat {
        timestamp: DateTime<Utc>,
        stats: DaemonStats,
    },
}

/// Statistics about the daemon's current state
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DaemonStats {
    /// Total number of memories stored
    pub total_memories: u64,
    /// Number of memories in hot tier
    pub hot_count: u64,
    /// Number of memories in warm tier
    pub warm_count: u64,
    /// Number of memories in cold tier
    pub cold_count: u64,
    /// Total number of requests processed
    pub total_requests: u64,
    /// Number of active sessions
    pub active_sessions: u64,
}

/// Subset of Memory fields for admin API responses
///
/// Excludes the embedding vector (384 floats) to reduce payload size
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminMemory {
    /// Unique identifier for this memory
    pub id: String,
    /// The actual content of the memory
    pub content: String,
    /// Classification of what kind of memory this is
    pub memory_type: String,
    /// Which storage tier this memory is in
    pub tier: String,
    /// Current weight (importance score)
    pub weight: f32,
    /// How many times this memory has been accessed
    pub access_count: u32,
    /// When this memory was created
    pub created_at: DateTime<Utc>,
    /// When this memory was last accessed
    pub last_accessed: DateTime<Utc>,
    /// Extracted entities (names, places, etc.)
    pub entities: Vec<String>,
    /// Optional conversation ID this memory belongs to
    pub conversation_id: Option<String>,
}

impl From<&Memory> for AdminMemory {
    fn from(memory: &Memory) -> Self {
        Self {
            id: memory.id.to_string(),
            content: memory.content.clone(),
            memory_type: format!("{:?}", memory.memory_type),
            tier: format!("{:?}", memory.tier),
            weight: memory.weight,
            access_count: memory.access_count,
            created_at: memory.created_at,
            last_accessed: memory.last_accessed,
            entities: memory.entities.clone(),
            conversation_id: memory.conversation_id.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::types::{MemorySource, MemoryType};

    #[test]
    fn test_proxy_event_serialization() {
        let event = ProxyEvent::RequestStarted {
            request_id: "req-123".to_string(),
            method: "POST".to_string(),
            path: "/v1/chat/completions".to_string(),
            provider: "openai".to_string(),
            timestamp: Utc::now(),
        };

        let json = serde_json::to_string(&event).expect("Failed to serialize event");
        assert!(json.contains("request_started"));
        assert!(json.contains("req-123"));
    }

    #[test]
    fn test_daemon_stats_default() {
        let stats = DaemonStats::default();
        assert_eq!(stats.total_memories, 0);
        assert_eq!(stats.hot_count, 0);
        assert_eq!(stats.warm_count, 0);
        assert_eq!(stats.cold_count, 0);
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.active_sessions, 0);
    }

    #[test]
    fn test_admin_memory_from_memory() {
        let memory = Memory::new(
            "Test content".to_string(),
            vec![0.1; 384],
            MemoryType::Semantic,
            MemorySource::Manual,
        );

        let admin_memory = AdminMemory::from(&memory);

        assert_eq!(admin_memory.id, memory.id.to_string());
        assert_eq!(admin_memory.content, memory.content);
        assert_eq!(admin_memory.memory_type, "Semantic");
        assert_eq!(admin_memory.tier, "Hot");
        assert_eq!(admin_memory.weight, memory.weight);
        assert_eq!(admin_memory.access_count, memory.access_count);
        assert!(admin_memory.entities.is_empty());
        assert!(admin_memory.conversation_id.is_none());
    }

    #[test]
    fn test_admin_memory_serialization() {
        let admin_memory = AdminMemory {
            id: "test-id".to_string(),
            content: "Test content".to_string(),
            memory_type: "Semantic".to_string(),
            tier: "Hot".to_string(),
            weight: 0.75,
            access_count: 5,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            entities: vec!["Rust".to_string(), "programming".to_string()],
            conversation_id: Some("conv-123".to_string()),
        };

        let json = serde_json::to_string(&admin_memory).expect("Failed to serialize");
        let deserialized: AdminMemory = serde_json::from_str(&json).expect("Failed to deserialize");

        assert_eq!(admin_memory.id, deserialized.id);
        assert_eq!(admin_memory.content, deserialized.content);
        assert_eq!(admin_memory.memory_type, deserialized.memory_type);
        assert_eq!(admin_memory.weight, deserialized.weight);
    }
}
