//! Filter types for memory search operations
//!
//! Provides filtering capabilities for vector similarity searches,
//! allowing queries to be narrowed by memory type, weight, time, and conversation.

use chrono::{DateTime, Utc};

use crate::memory::types::MemoryType;

/// Filter criteria for memory search operations.
///
/// All fields are optional - when `None`, that filter is not applied.
/// Multiple filters are combined with AND logic.
#[derive(Debug, Clone, Default)]
pub struct MemoryFilter {
    /// Filter by specific memory types (OR logic within this filter)
    pub memory_types: Option<Vec<MemoryType>>,
    /// Minimum weight threshold (inclusive)
    pub min_weight: Option<f32>,
    /// Only return memories created after this time
    pub since: Option<DateTime<Utc>>,
    /// Filter to specific conversation
    pub conversation_id: Option<String>,
    /// Session filter: None = not set, Some(None) = global only, Some(Some(id)) = session or global
    pub session_filter: Option<Option<String>>,
}

impl MemoryFilter {
    /// Create a new empty filter (no filtering applied)
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by memory types
    pub fn with_memory_types(mut self, types: Vec<MemoryType>) -> Self {
        self.memory_types = Some(types);
        self
    }

    /// Filter by minimum weight
    pub fn with_min_weight(mut self, min_weight: f32) -> Self {
        self.min_weight = Some(min_weight);
        self
    }

    /// Filter by creation time
    pub fn since(mut self, since: DateTime<Utc>) -> Self {
        self.since = Some(since);
        self
    }

    /// Filter by conversation ID
    pub fn with_conversation_id(mut self, conversation_id: String) -> Self {
        self.conversation_id = Some(conversation_id);
        self
    }

    /// Filter by session ID with NULL awareness.
    /// When `Some(id)`: matches memories for that session OR global memories (conversation_id IS NULL)
    /// When `None`: matches only global memories (conversation_id IS NULL)
    pub fn with_session_filter(mut self, session_id: Option<String>) -> Self {
        self.session_filter = Some(session_id);
        self
    }

    /// Build a SQL WHERE clause from this filter.
    /// Returns `None` if no filters are set.
    pub fn to_sql_clause(&self) -> Option<String> {
        let mut conditions = Vec::new();

        // Memory types filter (OR within types)
        if let Some(ref types) = self.memory_types {
            if !types.is_empty() {
                let type_strs: Vec<&str> = types
                    .iter()
                    .map(|t| match t {
                        MemoryType::Episodic => "Episodic",
                        MemoryType::Semantic => "Semantic",
                        MemoryType::Procedural => "Procedural",
                    })
                    .collect();

                if type_strs.len() == 1 {
                    conditions.push(format!("memory_type = '{}'", type_strs[0]));
                } else {
                    let in_clause = type_strs
                        .iter()
                        .map(|s| format!("'{s}'"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    conditions.push(format!("memory_type IN ({in_clause})"));
                }
            }
        }

        // Min weight filter
        if let Some(min_weight) = self.min_weight {
            conditions.push(format!("weight >= {min_weight}"));
        }

        // Since filter (created_at is stored as microseconds since epoch)
        if let Some(ref since) = self.since {
            let micros = since.timestamp_micros();
            conditions.push(format!("created_at >= {micros}"));
        }

        // Conversation ID filter
        if let Some(ref conv_id) = self.conversation_id {
            conditions.push(format!("conversation_id = '{conv_id}'"));
        }

        // Session filter with NULL awareness
        if let Some(ref session_opt) = self.session_filter {
            match session_opt {
                Some(session_id) => {
                    // Session specified: match session memories OR global memories
                    conditions.push(format!(
                        "(conversation_id = '{session_id}' OR conversation_id IS NULL)"
                    ));
                }
                None => {
                    // Explicitly None: match only global memories
                    conditions.push("conversation_id IS NULL".to_string());
                }
            }
        }

        if conditions.is_empty() {
            None
        } else {
            Some(conditions.join(" AND "))
        }
    }

    /// Check if this filter is empty (no conditions set)
    pub fn is_empty(&self) -> bool {
        self.memory_types.is_none()
            && self.min_weight.is_none()
            && self.since.is_none()
            && self.conversation_id.is_none()
            && self.session_filter.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_filter() {
        let filter = MemoryFilter::new();
        assert!(filter.is_empty());
        assert!(filter.to_sql_clause().is_none());
    }

    #[test]
    fn test_single_memory_type_filter() {
        let filter = MemoryFilter::new().with_memory_types(vec![MemoryType::Semantic]);

        let sql = filter.to_sql_clause().unwrap();
        assert_eq!(sql, "memory_type = 'Semantic'");
    }

    #[test]
    fn test_multiple_memory_types_filter() {
        let filter =
            MemoryFilter::new().with_memory_types(vec![MemoryType::Episodic, MemoryType::Semantic]);

        let sql = filter.to_sql_clause().unwrap();
        assert_eq!(sql, "memory_type IN ('Episodic', 'Semantic')");
    }

    #[test]
    fn test_min_weight_filter() {
        let filter = MemoryFilter::new().with_min_weight(0.5);

        let sql = filter.to_sql_clause().unwrap();
        assert_eq!(sql, "weight >= 0.5");
    }

    #[test]
    fn test_conversation_id_filter() {
        let filter = MemoryFilter::new().with_conversation_id("conv-123".to_string());

        let sql = filter.to_sql_clause().unwrap();
        assert_eq!(sql, "conversation_id = 'conv-123'");
    }

    #[test]
    fn test_combined_filters() {
        let filter = MemoryFilter::new()
            .with_memory_types(vec![MemoryType::Semantic])
            .with_min_weight(0.7)
            .with_conversation_id("conv-456".to_string());

        let sql = filter.to_sql_clause().unwrap();
        assert!(sql.contains("memory_type = 'Semantic'"));
        assert!(sql.contains("weight >= 0.7"));
        assert!(sql.contains("conversation_id = 'conv-456'"));
        assert!(sql.contains(" AND "));
    }

    #[test]
    fn test_since_filter() {
        use chrono::TimeZone;
        let since = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let filter = MemoryFilter::new().since(since);

        let sql = filter.to_sql_clause().unwrap();
        assert!(sql.contains("created_at >= "));
    }

    #[test]
    fn test_session_filter_with_session() {
        let filter = MemoryFilter::new().with_session_filter(Some("session-abc".to_string()));

        let sql = filter.to_sql_clause().unwrap();
        assert_eq!(
            sql,
            "(conversation_id = 'session-abc' OR conversation_id IS NULL)"
        );
    }

    #[test]
    fn test_session_filter_global_only() {
        let filter = MemoryFilter::new().with_session_filter(None);

        let sql = filter.to_sql_clause().unwrap();
        assert_eq!(sql, "conversation_id IS NULL");
    }
}
