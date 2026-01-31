//! Tombstone records for evicted memories
//!
//! Defines structures for tracking memories that have been evicted from storage,
//! preserving metadata about what was lost and why.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// A tombstone record for an evicted memory.
///
/// Tombstones preserve metadata about memories that have been removed from storage,
/// allowing the system to acknowledge gaps in knowledge when queried about evicted content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tombstone {
    /// Unique identifier of the original memory that was evicted
    pub original_id: Uuid,
    /// When this memory was evicted from storage
    pub evicted_at: DateTime<Utc>,
    /// Topics or subjects associated with the evicted memory
    pub topics: Vec<String>,
    /// Participants (people, entities) mentioned in the evicted memory
    pub participants: Vec<String>,
    /// Approximate date when the original memory was created
    pub approximate_date: DateTime<Utc>,
    /// Reason why this memory was evicted
    pub reason: EvictionReason,
}

/// Reasons why a memory may be evicted from storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvictionReason {
    /// Evicted due to storage pressure (capacity limits)
    StoragePressure,
    /// Evicted due to low importance weight
    LowWeight,
    /// Evicted because it was superseded by a newer memory
    Superseded { by: Uuid },
    /// Manually deleted by user or administrator
    ManualDeletion,
}

impl Tombstone {
    /// Create a new tombstone record for an evicted memory.
    pub fn new(
        original_id: Uuid,
        topics: Vec<String>,
        participants: Vec<String>,
        approximate_date: DateTime<Utc>,
        reason: EvictionReason,
    ) -> Self {
        Self {
            original_id,
            evicted_at: Utc::now(),
            topics,
            participants,
            approximate_date,
            reason,
        }
    }
}

impl Tombstone {
    /// Format the tombstone as XML for memory injection.
    /// Returns a string in the format:
    /// ```xml
    /// <nova-tombstone timestamp="2024-01-15" topics="project-x, alice">
    ///   I previously knew details about this topic but no longer have them.
    /// </nova-tombstone>
    /// ```
    pub fn to_xml(&self) -> String {
        let timestamp = self.approximate_date.format("%Y-%m-%d").to_string();
        let topics_str = if self.topics.is_empty() {
            "unknown".to_string()
        } else {
            self.topics.join(", ")
        };

        let content = format!("{self}");

        format!(
            r#"<nova-tombstone timestamp="{timestamp}" topics="{topics_str}">
  {content}
</nova-tombstone>"#
        )
    }
}

impl fmt::Display for Tombstone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Format topics list
        let topics_str = if self.topics.is_empty() {
            "various topics".to_string()
        } else if self.topics.len() == 1 {
            self.topics[0].clone()
        } else {
            let mut topics = self.topics.clone();
            let last = topics.pop().unwrap();
            format!("{} and {}", topics.join(", "), last)
        };

        // Format participants list
        let participants_str = if self.participants.is_empty() {
            "unknown participants".to_string()
        } else if self.participants.len() == 1 {
            self.participants[0].clone()
        } else {
            let mut participants = self.participants.clone();
            let last = participants.pop().unwrap();
            format!("{} and {}", participants.join(", "), last)
        };

        // Format approximate date
        let date_str = self.approximate_date.format("%B %Y").to_string();

        // Format reason
        let reason_str = match &self.reason {
            EvictionReason::StoragePressure => "storage pressure".to_string(),
            EvictionReason::LowWeight => "low importance".to_string(),
            EvictionReason::Superseded { by } => {
                format!("being superseded by memory {by}")
            }
            EvictionReason::ManualDeletion => "manual deletion".to_string(),
        };

        write!(
            f,
            "I previously knew about {topics_str} with {participants_str} around {date_str}, but this memory was evicted due to {reason_str}."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tombstone_new() {
        let original_id = Uuid::new_v4();
        let approximate_date = Utc::now();
        let tombstone = Tombstone::new(
            original_id,
            vec!["programming".to_string(), "rust".to_string()],
            vec!["Alice".to_string()],
            approximate_date,
            EvictionReason::StoragePressure,
        );

        assert_eq!(tombstone.original_id, original_id);
        assert_eq!(tombstone.topics.len(), 2);
        assert_eq!(tombstone.participants.len(), 1);
        assert_eq!(tombstone.approximate_date, approximate_date);
        assert!(matches!(tombstone.reason, EvictionReason::StoragePressure));
    }

    #[test]
    fn test_tombstone_serialization() {
        let tombstone = Tombstone::new(
            Uuid::new_v4(),
            vec!["topic1".to_string()],
            vec!["participant1".to_string()],
            Utc::now(),
            EvictionReason::LowWeight,
        );

        let json = serde_json::to_string(&tombstone).expect("Failed to serialize tombstone");
        let deserialized: Tombstone =
            serde_json::from_str(&json).expect("Failed to deserialize tombstone");

        assert_eq!(tombstone.original_id, deserialized.original_id);
        assert_eq!(tombstone.topics, deserialized.topics);
        assert_eq!(tombstone.participants, deserialized.participants);
        assert_eq!(tombstone.reason, deserialized.reason);
    }

    #[test]
    fn test_eviction_reason_serialization() {
        let reasons = vec![
            EvictionReason::StoragePressure,
            EvictionReason::LowWeight,
            EvictionReason::Superseded { by: Uuid::new_v4() },
            EvictionReason::ManualDeletion,
        ];

        for reason in reasons {
            let json = serde_json::to_string(&reason).expect("Failed to serialize");
            let deserialized: EvictionReason =
                serde_json::from_str(&json).expect("Failed to deserialize");
            assert_eq!(reason, deserialized);
        }
    }

    #[test]
    fn test_tombstone_display_single_topic_single_participant() {
        let tombstone = Tombstone::new(
            Uuid::new_v4(),
            vec!["machine learning".to_string()],
            vec!["Bob".to_string()],
            Utc::now(),
            EvictionReason::StoragePressure,
        );

        let display_str = format!("{tombstone}");
        assert!(display_str.contains("machine learning"));
        assert!(display_str.contains("Bob"));
        assert!(display_str.contains("storage pressure"));
        assert!(display_str.starts_with("I previously knew about"));
    }

    #[test]
    fn test_tombstone_display_multiple_topics_multiple_participants() {
        let tombstone = Tombstone::new(
            Uuid::new_v4(),
            vec![
                "rust".to_string(),
                "programming".to_string(),
                "async".to_string(),
            ],
            vec!["Alice".to_string(), "Charlie".to_string()],
            Utc::now(),
            EvictionReason::LowWeight,
        );

        let display_str = format!("{tombstone}");
        assert!(display_str.contains("rust, programming and async"));
        assert!(display_str.contains("Alice and Charlie"));
        assert!(display_str.contains("low importance"));
    }

    #[test]
    fn test_tombstone_display_empty_topics() {
        let tombstone = Tombstone::new(
            Uuid::new_v4(),
            vec![],
            vec!["Alice".to_string()],
            Utc::now(),
            EvictionReason::ManualDeletion,
        );

        let display_str = format!("{tombstone}");
        assert!(display_str.contains("various topics"));
        assert!(display_str.contains("manual deletion"));
    }

    #[test]
    fn test_tombstone_display_empty_participants() {
        let tombstone = Tombstone::new(
            Uuid::new_v4(),
            vec!["topic".to_string()],
            vec![],
            Utc::now(),
            EvictionReason::ManualDeletion,
        );

        let display_str = format!("{tombstone}");
        assert!(display_str.contains("unknown participants"));
    }

    #[test]
    fn test_tombstone_display_superseded_reason() {
        let new_memory_id = Uuid::new_v4();
        let tombstone = Tombstone::new(
            Uuid::new_v4(),
            vec!["topic".to_string()],
            vec!["participant".to_string()],
            Utc::now(),
            EvictionReason::Superseded { by: new_memory_id },
        );

        let display_str = format!("{tombstone}");
        assert!(display_str.contains(&format!("being superseded by memory {new_memory_id}")));
    }

    #[test]
    fn test_tombstone_display_includes_date() {
        use chrono::TimeZone;
        let specific_date = Utc.with_ymd_and_hms(2024, 6, 15, 10, 30, 0).unwrap();

        let tombstone = Tombstone::new(
            Uuid::new_v4(),
            vec!["topic".to_string()],
            vec!["participant".to_string()],
            specific_date,
            EvictionReason::StoragePressure,
        );

        let display_str = format!("{tombstone}");
        assert!(display_str.contains("June 2024"));
    }

    #[test]
    fn test_tombstone_to_xml_format() {
        use chrono::TimeZone;
        let specific_date = Utc.with_ymd_and_hms(2024, 1, 15, 10, 30, 0).unwrap();

        let tombstone = Tombstone::new(
            Uuid::new_v4(),
            vec!["project-x".to_string(), "alice".to_string()],
            vec!["bob".to_string()],
            specific_date,
            EvictionReason::StoragePressure,
        );

        let xml = tombstone.to_xml();

        // Check XML structure
        assert!(xml.starts_with("<nova-tombstone"));
        assert!(xml.contains("timestamp=\"2024-01-15\""));
        assert!(xml.contains("topics=\"project-x, alice\""));
        assert!(xml.contains("</nova-tombstone>"));

        // Check that display content is included
        assert!(xml.contains("I previously knew about"));
        assert!(xml.contains("storage pressure"));
    }

    #[test]
    fn test_tombstone_to_xml_empty_topics() {
        let tombstone = Tombstone::new(
            Uuid::new_v4(),
            vec![],
            vec!["participant".to_string()],
            Utc::now(),
            EvictionReason::LowWeight,
        );

        let xml = tombstone.to_xml();

        // Should use "unknown" for empty topics
        assert!(xml.contains("topics=\"unknown\""));
    }

    #[test]
    fn test_tombstone_to_xml_single_topic() {
        use chrono::TimeZone;
        let specific_date = Utc.with_ymd_and_hms(2024, 3, 20, 0, 0, 0).unwrap();

        let tombstone = Tombstone::new(
            Uuid::new_v4(),
            vec!["machine-learning".to_string()],
            vec![],
            specific_date,
            EvictionReason::ManualDeletion,
        );

        let xml = tombstone.to_xml();

        assert!(xml.contains("timestamp=\"2024-03-20\""));
        assert!(xml.contains("topics=\"machine-learning\""));
        assert!(xml.contains("manual deletion"));
    }
}
