//! Integration tests for weight calculation and decay
//!
//! Tests the weight calculation system including decay, access reinforcement,
//! and emotional boost functionality.

use chrono::{Duration, Utc};
use mnemo::memory::types::{Memory, MemorySource, MemoryType};
use mnemo::memory::weight::{
    WeightConfig, calculate_effective_weight, calculate_initial_weight,
};
use mnemo::router::{Entity, EntityLabel, RouterOutput};

fn create_test_memory(content: &str, weight: f32, access_count: u32, age_days: i64) -> Memory {
    let mut memory = Memory::new(
        content.to_string(),
        vec![0.1; 384],
        MemoryType::Semantic,
        MemorySource::Conversation,
    );
    memory.weight = weight;
    memory.access_count = access_count;
    memory.created_at = Utc::now() - Duration::days(age_days);
    memory
}

fn create_router_output(entities_count: usize, emotional_valence: f32) -> RouterOutput {
    RouterOutput {
        topics: vec![],
        entities: (0..entities_count)
            .map(|i| Entity {
                text: format!("entity{i}"),
                label: EntityLabel::Person,
                confidence: 0.9,
            })
            .collect(),
        emotional_valence,
        query_keys: vec![],
        search_types: vec![MemoryType::Semantic],
    }
}

mod weight_config_tests {
    use super::*;

    #[test]
    fn test_default_weight_config() {
        let config = WeightConfig::default();
        assert_eq!(config.access_multiplier, 0.1);
        assert_eq!(config.decay_rate, 0.1);
        assert_eq!(config.emotional_multiplier, 0.3);
        assert_eq!(config.owner_multiplier, 0.5);
        assert_eq!(config.association_multiplier, 0.05);
    }

    #[test]
    fn test_custom_weight_config() {
        let config = WeightConfig::new(0.2, 0.3, 0.4, 0.6, 0.1);
        assert_eq!(config.access_multiplier, 0.2);
        assert_eq!(config.decay_rate, 0.3);
        assert_eq!(config.emotional_multiplier, 0.4);
        assert_eq!(config.owner_multiplier, 0.6);
        assert_eq!(config.association_multiplier, 0.1);
    }

    #[test]
    fn test_weight_config_equality() {
        let config1 = WeightConfig::default();
        let config2 = WeightConfig::new(0.1, 0.1, 0.3, 0.5, 0.05);
        assert_eq!(config1, config2);
    }
}

mod initial_weight_tests {
    use super::*;

    #[test]
    fn test_initial_weight_base_value() {
        let router_output = create_router_output(0, 0.0);
        let weight = calculate_initial_weight(&router_output, MemorySource::File);
        assert!((0.1..=1.0).contains(&weight));
    }

    #[test]
    fn test_initial_weight_with_entities() {
        let router_output_no_entities = create_router_output(0, 0.0);
        let router_output_many_entities = create_router_output(5, 0.0);
        let weight_no_entities =
            calculate_initial_weight(&router_output_no_entities, MemorySource::File);
        let weight_many_entities =
            calculate_initial_weight(&router_output_many_entities, MemorySource::File);
        assert!(weight_many_entities > weight_no_entities);
    }

    #[test]
    fn test_initial_weight_with_emotion() {
        let router_output_neutral = create_router_output(0, 0.0);
        let router_output_emotional = create_router_output(0, 0.8);
        let weight_neutral = calculate_initial_weight(&router_output_neutral, MemorySource::File);
        let weight_emotional =
            calculate_initial_weight(&router_output_emotional, MemorySource::File);
        assert!(weight_emotional > weight_neutral);
    }

    #[test]
    fn test_initial_weight_source_bonus() {
        let router_output = create_router_output(0, 0.0);
        let weight_file = calculate_initial_weight(&router_output, MemorySource::File);
        let weight_conversation =
            calculate_initial_weight(&router_output, MemorySource::Conversation);
        let weight_manual = calculate_initial_weight(&router_output, MemorySource::Manual);
        assert!(weight_conversation >= weight_file);
        assert!(weight_manual >= weight_conversation);
    }

    #[test]
    fn test_initial_weight_clamped_minimum() {
        let router_output = create_router_output(0, -1.0);
        let weight = calculate_initial_weight(&router_output, MemorySource::File);
        assert!(weight >= 0.1);
    }

    #[test]
    fn test_initial_weight_clamped_maximum() {
        let router_output = create_router_output(100, 1.0);
        let weight = calculate_initial_weight(&router_output, MemorySource::Manual);
        assert!(weight <= 1.0);
    }
}

mod decay_tests {
    use super::*;

    #[test]
    fn test_weight_decays_over_time() {
        let config = WeightConfig::default();
        let memory_recent = create_test_memory("Test", 0.5, 0, 0);
        let memory_old = create_test_memory("Test", 0.5, 0, 30);
        let weight_recent = calculate_effective_weight(&memory_recent, &config);
        let weight_old = calculate_effective_weight(&memory_old, &config);
        assert!(weight_old < weight_recent);
    }

    #[test]
    fn test_decay_with_zero_days() {
        let config = WeightConfig::default();
        let memory = create_test_memory("Test", 0.5, 0, 0);
        let weight = calculate_effective_weight(&memory, &config);
        assert!(weight > 0.0);
    }

    #[test]
    fn test_decay_with_very_old_memory() {
        let config = WeightConfig::default();
        let memory = create_test_memory("Test", 0.5, 0, 365);
        let weight = calculate_effective_weight(&memory, &config);
        assert!(weight > 0.0);
    }

    #[test]
    fn test_decay_rate_affects_weight() {
        let memory = create_test_memory("Test", 0.5, 0, 30);
        let config_low_decay = WeightConfig::new(0.1, 0.01, 0.3, 0.5, 0.05);
        let config_high_decay = WeightConfig::new(0.1, 0.5, 0.3, 0.5, 0.05);
        let weight_low_decay = calculate_effective_weight(&memory, &config_low_decay);
        let weight_high_decay = calculate_effective_weight(&memory, &config_high_decay);
        assert!(weight_low_decay > weight_high_decay);
    }

    #[test]
    fn test_exponential_decay_behavior() {
        let config = WeightConfig::default();
        let base_weight = 0.5;
        let days = [0, 7, 30, 90, 365];
        let mut previous_weight = f32::MAX;
        for day in &days {
            let memory = create_test_memory("Test", base_weight, 0, *day);
            let weight = calculate_effective_weight(&memory, &config);
            assert!(weight <= previous_weight);
            previous_weight = weight;
        }
    }
}

mod access_reinforcement_tests {
    use super::*;

    #[test]
    fn test_access_increases_weight() {
        let config = WeightConfig::default();
        let memory_no_access = create_test_memory("Test", 0.5, 0, 0);
        let memory_high_access = create_test_memory("Test", 0.5, 10, 0);
        let weight_no_access = calculate_effective_weight(&memory_no_access, &config);
        let weight_high_access = calculate_effective_weight(&memory_high_access, &config);
        assert!(weight_high_access > weight_no_access);
    }

    #[test]
    fn test_access_increases_weight_monotonically() {
        let config = WeightConfig::default();
        let base_weight = 0.5;
        // Test that access count increases weight monotonically
        let access_counts = [0, 1, 5, 10, 50, 100];
        let mut previous_weight = 0.0;
        for count in &access_counts {
            let memory = create_test_memory("Test content", base_weight, *count, 0);
            let weight = calculate_effective_weight(&memory, &config);
            assert!(
                weight >= previous_weight,
                "Weight should increase or stay same with more accesses: count={count} weight={weight} previous={previous_weight}"
            );
            previous_weight = weight;
        }
    }

    #[test]
    fn test_access_multiplier_affects_reinforcement() {
        let memory = create_test_memory("Test", 0.5, 10, 0);
        let config_low_mult = WeightConfig::new(0.01, 0.1, 0.3, 0.5, 0.05);
        let config_high_mult = WeightConfig::new(0.5, 0.1, 0.3, 0.5, 0.05);
        let weight_low = calculate_effective_weight(&memory, &config_low_mult);
        let weight_high = calculate_effective_weight(&memory, &config_high_mult);
        assert!(weight_high > weight_low);
    }

    #[test]
    fn test_access_counteracts_decay() {
        let config = WeightConfig::default();
        let memory_old_no_access = create_test_memory("Test", 0.5, 0, 60);
        let memory_old_high_access = create_test_memory("Test", 0.5, 50, 60);
        let weight_no_access = calculate_effective_weight(&memory_old_no_access, &config);
        let weight_high_access = calculate_effective_weight(&memory_old_high_access, &config);
        assert!(weight_high_access > weight_no_access);
    }

    #[test]
    fn test_zero_access_has_base_factor() {
        let config = WeightConfig::default();
        let memory = create_test_memory("Test", 0.5, 0, 0);
        let weight = calculate_effective_weight(&memory, &config);
        assert!(weight >= 0.5);
    }
}

mod emotional_boost_tests {
    use super::*;

    #[test]
    fn test_emotional_content_gets_boost() {
        let config = WeightConfig::default();
        let memory_neutral = create_test_memory("The weather is nice today", 0.5, 0, 0);
        let memory_emotional = create_test_memory("I love this amazing day", 0.5, 0, 0);
        let weight_neutral = calculate_effective_weight(&memory_neutral, &config);
        let weight_emotional = calculate_effective_weight(&memory_emotional, &config);
        assert!(weight_emotional > weight_neutral);
    }

    #[test]
    fn test_negative_emotion_also_gets_boost() {
        let config = WeightConfig::default();
        let memory_neutral = create_test_memory("The cat sat on the mat", 0.5, 0, 0);
        let memory_negative = create_test_memory("I hate this terrible awful day", 0.5, 0, 0);
        let weight_neutral = calculate_effective_weight(&memory_neutral, &config);
        let weight_negative = calculate_effective_weight(&memory_negative, &config);
        assert!(weight_negative > weight_neutral);
    }

    #[test]
    fn test_emotional_multiplier_affects_boost() {
        let memory = create_test_memory("I love this amazing wonderful day", 0.5, 0, 0);
        let config_low = WeightConfig::new(0.1, 0.1, 0.01, 0.5, 0.05);
        let config_high = WeightConfig::new(0.1, 0.1, 1.0, 0.5, 0.05);
        let weight_low = calculate_effective_weight(&memory, &config_low);
        let weight_high = calculate_effective_weight(&memory, &config_high);
        assert!(weight_high > weight_low);
    }

    #[test]
    fn test_multiple_emotional_words_increase_boost() {
        let config = WeightConfig::default();
        let memory_one_emotion = create_test_memory("This is good", 0.5, 0, 0);
        let memory_many_emotions =
            create_test_memory("This is good amazing wonderful fantastic", 0.5, 0, 0);
        let weight_one = calculate_effective_weight(&memory_one_emotion, &config);
        let weight_many = calculate_effective_weight(&memory_many_emotions, &config);
        assert!(weight_many >= weight_one);
    }

    #[test]
    fn test_emotional_boost_capped() {
        let config = WeightConfig::default();
        let memory_extreme = create_test_memory(
            "Love love love hate amazing terrible wonderful awful great bad perfect worst",
            0.5,
            0,
            0,
        );
        let weight = calculate_effective_weight(&memory_extreme, &config);
        assert!(weight < 2.0);
    }
}

mod combined_weight_tests {
    use super::*;

    #[test]
    fn test_all_factors_combined() {
        let config = WeightConfig::default();
        let memory_strong =
            create_test_memory("I love this amazing important information", 0.8, 20, 0);
        let memory_weak = create_test_memory("The weather is nice", 0.3, 0, 90);
        let weight_strong = calculate_effective_weight(&memory_strong, &config);
        let weight_weak = calculate_effective_weight(&memory_weak, &config);
        assert!(weight_strong > weight_weak);
    }

    #[test]
    fn test_weight_always_positive() {
        let config = WeightConfig::default();
        let test_cases = [(0.1, 0, 365), (0.5, 0, 365), (0.1, 100, 365)];
        for (base_weight, access, age) in &test_cases {
            let memory = create_test_memory("Test", *base_weight, *access, *age);
            let weight = calculate_effective_weight(&memory, &config);
            assert!(weight > 0.0);
        }
    }

    #[test]
    fn test_weight_calculation_consistency() {
        let config = WeightConfig::default();
        let memory = create_test_memory("Test content", 0.5, 5, 10);
        let weight1 = calculate_effective_weight(&memory, &config);
        let weight2 = calculate_effective_weight(&memory, &config);
        let weight3 = calculate_effective_weight(&memory, &config);
        assert_eq!(weight1, weight2);
        assert_eq!(weight2, weight3);
    }
}

mod edge_case_tests {
    use super::*;

    #[test]
    fn test_very_high_access_count() {
        let config = WeightConfig::default();
        let memory = create_test_memory("Test", 0.5, 10000, 0);
        let weight = calculate_effective_weight(&memory, &config);
        assert!(weight > 0.0 && weight < f32::INFINITY);
    }

    #[test]
    fn test_very_old_memory() {
        let config = WeightConfig::default();
        let memory = create_test_memory("Test", 0.5, 0, 1000);
        let weight = calculate_effective_weight(&memory, &config);
        assert!(weight > 0.0);
    }

    #[test]
    fn test_zero_base_weight() {
        let config = WeightConfig::default();
        let memory = create_test_memory("Test", 0.0, 10, 0);
        let weight = calculate_effective_weight(&memory, &config);
        assert!(weight >= 0.0);
    }

    #[test]
    fn test_maximum_base_weight() {
        let config = WeightConfig::default();
        let memory = create_test_memory("Test", 1.0, 100, 0);
        let weight = calculate_effective_weight(&memory, &config);
        assert!(weight > 0.0);
    }
}
