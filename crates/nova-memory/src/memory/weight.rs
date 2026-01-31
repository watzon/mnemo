//! Weight calculation for memory importance scoring
//!
//! This module provides weight calculation functions that determine
//! the importance of memories based on various factors including
//! access frequency, emotional content, age, and source.

use crate::memory::types::{Memory, MemorySource};
use crate::router::RouterOutput;
use chrono::Utc;

/// Configuration for weight calculation parameters
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeightConfig {
    /// Multiplier for access count logarithm (default: 0.1)
    pub access_multiplier: f32,
    /// Decay rate per day (default: 0.1)
    pub decay_rate: f32,
    /// Multiplier for emotional content (default: 0.3)
    pub emotional_multiplier: f32,
    /// Multiplier for owner importance (default: 0.5)
    pub owner_multiplier: f32,
    /// Multiplier for association strength (default: 0.05)
    pub association_multiplier: f32,
}

impl Default for WeightConfig {
    fn default() -> Self {
        Self {
            access_multiplier: 0.1,
            decay_rate: 0.1,
            emotional_multiplier: 0.3,
            owner_multiplier: 0.5,
            association_multiplier: 0.05,
        }
    }
}

impl WeightConfig {
    /// Create a new weight config with custom parameters
    pub fn new(
        access_multiplier: f32,
        decay_rate: f32,
        emotional_multiplier: f32,
        owner_multiplier: f32,
        association_multiplier: f32,
    ) -> Self {
        Self {
            access_multiplier,
            decay_rate,
            emotional_multiplier,
            owner_multiplier,
            association_multiplier,
        }
    }
}

/// Calculate the initial weight for a new memory based on router output and source
///
/// This function computes a base weight considering:
/// - Number of entities extracted (more entities = higher weight)
/// - Emotional valence (stronger emotions = higher weight)
/// - Memory source (manual entries get higher weight)
pub fn calculate_initial_weight(router_output: &RouterOutput, source: MemorySource) -> f32 {
    let mut weight = 0.5;

    // Entity bonus: more entities = more important
    weight += router_output.entities.len() as f32 * 0.05;

    // Emotional bonus: stronger emotional content = more important
    weight += router_output.emotional_valence.abs() * 0.2;

    // Source bonus: manual entries are typically more important
    match source {
        MemorySource::Conversation => weight += 0.1,
        MemorySource::Manual => weight += 0.3,
        _ => {}
    }

    weight.clamp(0.1, 1.0)
}

/// Calculate the effective weight of a memory considering time decay and access patterns
///
/// Formula: base * ln(access_count + 1) * exp(-decay_rate * age_days) * (1 + emotional * emotional_mult)
///
/// This means:
/// - Base weight is the starting point
/// - Access count logarithmically increases weight (diminishing returns)
/// - Age exponentially decays weight (memories fade over time)
/// - Emotional content provides a multiplicative boost
pub fn calculate_effective_weight(memory: &Memory, config: &WeightConfig) -> f32 {
    let age_days = (Utc::now() - memory.created_at).num_days() as f32;
    // Access factor: logarithmic increase with diminishing returns
    // Formula: 1 + multiplier * ln(access_count + 1)
    // This ensures weight starts at base when access_count = 0
    let access_factor = 1.0 + config.access_multiplier * (memory.access_count as f32 + 1.0).ln();
    let decay_factor = (-config.decay_rate * age_days).exp();

    // Calculate emotional boost from memory content
    // For now, we use a simple heuristic based on content analysis
    let emotional_boost = estimate_emotional_boost(&memory.content, config.emotional_multiplier);

    memory.weight * access_factor * decay_factor * (1.0 + emotional_boost)
}

/// Estimate emotional boost from memory content
///
/// This is a simplified heuristic that looks for emotional indicators in the content.
/// In a full implementation, this would use the original router output emotional valence.
fn estimate_emotional_boost(content: &str, emotional_multiplier: f32) -> f32 {
    let emotional_words = [
        "love",
        "hate",
        "amazing",
        "terrible",
        "wonderful",
        "awful",
        "great",
        "bad",
        "excellent",
        "horrible",
        "fantastic",
        "disgusting",
        "perfect",
        "worst",
        "beautiful",
        "ugly",
        "awesome",
        "dreadful",
        "brilliant",
        "pathetic",
    ];

    let content_lower = content.to_lowercase();
    let emotional_count = emotional_words
        .iter()
        .filter(|word| content_lower.contains(*word))
        .count() as f32;

    // Cap the emotional boost at a reasonable level
    (emotional_count * 0.1 * emotional_multiplier).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::types::{Memory, MemorySource, MemoryType};
    use crate::router::RouterOutput;
    use chrono::{Duration, Utc};

    fn create_test_memory(weight: f32, access_count: u32, age_days: i64) -> Memory {
        let mut memory = Memory::new(
            "Test content".to_string(),
            vec![0.1; 384],
            MemoryType::Semantic,
            MemorySource::Conversation,
        );
        memory.weight = weight;
        memory.access_count = access_count;
        memory.created_at = Utc::now() - Duration::days(age_days);
        memory
    }

    fn create_test_router_output(entities_count: usize, emotional_valence: f32) -> RouterOutput {
        RouterOutput {
            topics: vec![],
            entities: (0..entities_count)
                .map(|i| crate::router::Entity {
                    text: format!("entity{}", i),
                    label: crate::router::EntityLabel::Person,
                    confidence: 0.9,
                })
                .collect(),
            emotional_valence,
            query_keys: vec![],
            search_types: vec![MemoryType::Semantic],
        }
    }

    #[test]
    fn test_weight_config_default() {
        let config = WeightConfig::default();
        assert_eq!(config.access_multiplier, 0.1);
        assert_eq!(config.decay_rate, 0.1);
        assert_eq!(config.emotional_multiplier, 0.3);
        assert_eq!(config.owner_multiplier, 0.5);
        assert_eq!(config.association_multiplier, 0.05);
    }

    #[test]
    fn test_weight_config_new() {
        let config = WeightConfig::new(0.2, 0.3, 0.4, 0.6, 0.1);
        assert_eq!(config.access_multiplier, 0.2);
        assert_eq!(config.decay_rate, 0.3);
        assert_eq!(config.emotional_multiplier, 0.4);
        assert_eq!(config.owner_multiplier, 0.6);
        assert_eq!(config.association_multiplier, 0.1);
    }

    #[test]
    fn test_calculate_initial_weight_positive() {
        let router_output = create_test_router_output(3, 0.5);
        let weight = calculate_initial_weight(&router_output, MemorySource::Conversation);
        assert!(weight > 0.0, "Initial weight should be positive");
        assert!(
            weight >= 0.1 && weight <= 1.0,
            "Weight should be clamped to [0.1, 1.0]"
        );
    }

    #[test]
    fn test_calculate_initial_weight_with_entities() {
        let router_output_low = create_test_router_output(1, 0.0);
        let router_output_high = create_test_router_output(5, 0.0);

        let weight_low = calculate_initial_weight(&router_output_low, MemorySource::File);
        let weight_high = calculate_initial_weight(&router_output_high, MemorySource::File);

        assert!(
            weight_high > weight_low,
            "More entities should result in higher weight"
        );
    }

    #[test]
    fn test_calculate_initial_weight_with_emotion() {
        let router_output_neutral = create_test_router_output(0, 0.0);
        let router_output_emotional = create_test_router_output(0, 0.8);

        let weight_neutral = calculate_initial_weight(&router_output_neutral, MemorySource::File);
        let weight_emotional =
            calculate_initial_weight(&router_output_emotional, MemorySource::File);

        assert!(
            weight_emotional > weight_neutral,
            "Emotional content should have higher weight"
        );
    }

    #[test]
    fn test_calculate_initial_weight_source_bonus() {
        let router_output = create_test_router_output(0, 0.0);

        let weight_file = calculate_initial_weight(&router_output, MemorySource::File);
        let weight_conversation =
            calculate_initial_weight(&router_output, MemorySource::Conversation);
        let weight_manual = calculate_initial_weight(&router_output, MemorySource::Manual);

        assert!(
            weight_conversation > weight_file,
            "Conversation should have higher weight than file"
        );
        assert!(
            weight_manual > weight_conversation,
            "Manual should have higher weight than conversation"
        );
    }

    #[test]
    fn test_calculate_initial_weight_clamping() {
        // Test minimum clamping
        let router_output_empty = create_test_router_output(0, -1.0);
        let weight = calculate_initial_weight(&router_output_empty, MemorySource::File);
        assert!(weight >= 0.1, "Weight should be clamped to minimum 0.1");

        // Test maximum clamping with extreme values
        let router_output_extreme = create_test_router_output(100, 1.0);
        let weight_extreme = calculate_initial_weight(&router_output_extreme, MemorySource::Manual);
        assert!(
            weight_extreme <= 1.0,
            "Weight should be clamped to maximum 1.0"
        );
    }

    #[test]
    fn test_calculate_effective_weight_decreases_over_time() {
        let config = WeightConfig::default();
        let memory_recent = create_test_memory(0.5, 0, 0);
        let memory_old = create_test_memory(0.5, 0, 30);

        let weight_recent = calculate_effective_weight(&memory_recent, &config);
        let weight_old = calculate_effective_weight(&memory_old, &config);

        assert!(
            weight_old < weight_recent,
            "Older memories should have lower effective weight due to decay"
        );
    }

    #[test]
    fn test_calculate_effective_weight_increases_with_access() {
        let config = WeightConfig::default();
        let memory_low_access = create_test_memory(0.5, 1, 0);
        let memory_high_access = create_test_memory(0.5, 10, 0);

        let weight_low = calculate_effective_weight(&memory_low_access, &config);
        let weight_high = calculate_effective_weight(&memory_high_access, &config);

        assert!(
            weight_high > weight_low,
            "Higher access count should result in higher effective weight"
        );
    }

    #[test]
    fn test_calculate_effective_weight_with_emotional_content() {
        let config = WeightConfig::default();

        let mut memory_neutral = create_test_memory(0.5, 0, 0);
        memory_neutral.content = "The weather is nice today".to_string();

        let mut memory_emotional = create_test_memory(0.5, 0, 0);
        memory_emotional.content = "I love this amazing wonderful day".to_string();

        let weight_neutral = calculate_effective_weight(&memory_neutral, &config);
        let weight_emotional = calculate_effective_weight(&memory_emotional, &config);

        assert!(
            weight_emotional > weight_neutral,
            "Emotional content should have higher effective weight"
        );
    }

    #[test]
    fn test_calculate_effective_weight_positive() {
        let config = WeightConfig::default();
        let memory = create_test_memory(0.5, 5, 1);

        let weight = calculate_effective_weight(&memory, &config);
        assert!(weight > 0.0, "Effective weight should always be positive");
    }

    #[test]
    fn test_estimate_emotional_boost() {
        let content_neutral = "The cat sat on the mat";
        let content_emotional = "I love this amazing terrible wonderful day";

        let boost_neutral = estimate_emotional_boost(content_neutral, 0.3);
        let boost_emotional = estimate_emotional_boost(content_emotional, 0.3);

        assert!(
            boost_emotional > boost_neutral,
            "Emotional content should have higher boost"
        );
        assert!(boost_neutral >= 0.0, "Boost should be non-negative");
        assert!(boost_emotional <= 1.0, "Boost should be capped at 1.0");
    }
}
