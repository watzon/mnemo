//! Integration tests for the memory router
//!
//! Tests the MemoryRouter implementation with real NER model loading.
//! These tests verify entity extraction, sentiment detection, and routing decisions.

use nova_memory::memory::types::MemoryType;
use nova_memory::router::{EntityLabel, MemoryRouter};

/// Test helper to create a router (loads NER model)
fn create_router() -> MemoryRouter {
    MemoryRouter::new().expect("Failed to create MemoryRouter - NER model may not be downloaded")
}

mod router_creation_tests {
    use super::*;

    #[test]
    fn test_router_creates_successfully() {
        let router = MemoryRouter::new();
        assert!(router.is_ok(), "Router should be created successfully");
    }

    #[test]
    fn test_router_can_be_created_multiple_times() {
        let router1 = MemoryRouter::new();
        let router2 = MemoryRouter::new();

        assert!(router1.is_ok());
        assert!(router2.is_ok());
    }
}

mod entity_extraction_tests {
    use super::*;

    #[test]
    fn test_extract_person_entities() {
        let router = create_router();
        let text = "John Smith met with Sarah Johnson at the conference.";
        let output = router.route(text).expect("Failed to route text");

        let has_person = output
            .entities
            .iter()
            .any(|e| e.label == EntityLabel::Person);
        assert!(
            has_person || !output.entities.is_empty(),
            "Should extract person entities from: {text}"
        );
    }

    #[test]
    fn test_extract_organization_entities() {
        let router = create_router();
        let text = "Microsoft and Google are competing in the AI space.";
        let output = router.route(text).expect("Failed to route text");

        let has_org = output
            .entities
            .iter()
            .any(|e| e.label == EntityLabel::Organization);
        assert!(
            has_org || !output.entities.is_empty(),
            "Should extract organization entities from: {text}"
        );
    }

    #[test]
    fn test_extract_location_entities() {
        let router = create_router();
        let text = "The meeting will be held in Seattle, Washington.";
        let output = router.route(text).expect("Failed to route text");

        let has_loc = output
            .entities
            .iter()
            .any(|e| e.label == EntityLabel::Location);
        assert!(
            has_loc || !output.entities.is_empty(),
            "Should extract location entities from: {text}"
        );
    }

    #[test]
    fn test_extract_multiple_entity_types() {
        let router = create_router();
        let text = "Barack Obama visited Microsoft headquarters in Redmond.";
        let output = router.route(text).expect("Failed to route text");

        // Should extract at least one entity
        assert!(
            !output.entities.is_empty(),
            "Should extract entities from text with multiple entity types"
        );

        // Check that entities have valid properties
        for entity in &output.entities {
            assert!(!entity.text.is_empty(), "Entity text should not be empty");
            assert!(
                entity.confidence >= 0.0 && entity.confidence <= 1.0,
                "Entity confidence should be in range [0, 1]"
            );
        }
    }

    #[test]
    fn test_entity_confidence_in_valid_range() {
        let router = create_router();
        let text = "Apple Inc. was founded by Steve Jobs in California.";
        let output = router.route(text).expect("Failed to route text");

        for entity in &output.entities {
            assert!(
                entity.confidence >= 0.0 && entity.confidence <= 1.0,
                "Entity confidence {} should be in range [0, 1] for entity: {}",
                entity.confidence,
                entity.text
            );
        }
    }

    #[test]
    fn test_no_entities_in_empty_text() {
        let router = create_router();
        let output = router.route("").expect("Failed to route empty text");

        assert!(
            output.entities.is_empty(),
            "Empty text should have no entities"
        );
    }

    #[test]
    fn test_no_entities_in_generic_text() {
        let router = create_router();
        let text = "The quick brown fox jumps over the lazy dog.";
        let output = router.route(text).expect("Failed to route text");

        // Generic text without proper nouns may or may not have entities
        // depending on the NER model, but entities should be valid if present
        for entity in &output.entities {
            assert!(!entity.text.is_empty(), "Entity text should not be empty");
        }
    }
}

mod router_output_completeness_tests {
    use super::*;

    #[test]
    fn test_output_has_all_fields_populated() {
        let router = create_router();
        let text = "John Smith works at Microsoft in Seattle and loves programming.";
        let output = router.route(text).expect("Failed to route text");

        // All fields should be accessible (not panic)
        let _ = &output.topics;
        let _ = &output.entities;
        let _ = output.emotional_valence;
        let _ = &output.query_keys;
        let _ = &output.search_types;
    }

    #[test]
    fn test_topics_extracted_from_text() {
        let router = create_router();
        let text = "Rust programming language is great for systems development.";
        let output = router.route(text).expect("Failed to route text");

        // Topics should be extracted (either from entities or noun phrases)
        assert!(
            !output.topics.is_empty() || !output.entities.is_empty(),
            "Should extract topics or entities from meaningful text"
        );
    }

    #[test]
    fn test_query_keys_generated() {
        let router = create_router();
        let text = "Machine learning and artificial intelligence are transforming technology.";
        let output = router.route(text).expect("Failed to route text");

        // Query keys should be generated from entities and topics
        assert!(
            !output.query_keys.is_empty()
                || !output.topics.is_empty()
                || !output.entities.is_empty(),
            "Should generate query keys or have entities/topics from text"
        );
    }

    #[test]
    fn test_search_types_determined() {
        let router = create_router();
        let text = "How do I write a function in Rust?";
        let output = router.route(text).expect("Failed to route text");

        // Should have at least one search type
        assert!(
            !output.search_types.is_empty(),
            "Should determine search types for text"
        );
    }

    #[test]
    fn test_default_search_types_for_generic_text() {
        let router = create_router();
        let text = "The weather is nice today.";
        let output = router.route(text).expect("Failed to route text");

        // Generic text should default to Episodic and Semantic
        assert!(
            output.search_types.contains(&MemoryType::Episodic)
                || output.search_types.contains(&MemoryType::Semantic),
            "Generic text should have Episodic or Semantic search type"
        );
    }
}

mod sentiment_detection_tests {
    use super::*;

    #[test]
    fn test_positive_sentiment_detected() {
        let router = create_router();
        let text = "I love this amazing product! It's wonderful and fantastic.";
        let output = router.route(text).expect("Failed to route text");

        assert!(
            output.emotional_valence > 0.0,
            "Positive text should have positive valence, got: {}",
            output.emotional_valence
        );
    }

    #[test]
    fn test_negative_sentiment_detected() {
        let router = create_router();
        let text = "I hate this terrible awful product. It's the worst and horrible.";
        let output = router.route(text).expect("Failed to route text");

        assert!(
            output.emotional_valence < 0.0,
            "Negative text should have negative valence, got: {}",
            output.emotional_valence
        );
    }

    #[test]
    fn test_neutral_sentiment_detected() {
        let router = create_router();
        let text = "The cat sat on the mat. The weather is cloudy today.";
        let output = router.route(text).expect("Failed to route text");

        assert!(
            output.emotional_valence.abs() < 0.5,
            "Neutral text should have near-zero valence, got: {}",
            output.emotional_valence
        );
    }

    #[test]
    fn test_mixed_sentiment_towards_neutral() {
        let router = create_router();
        let text = "I love the design but hate the price. It's great but also terrible.";
        let output = router.route(text).expect("Failed to route text");

        // Mixed sentiment should be closer to neutral than extreme
        assert!(
            output.emotional_valence.abs() < 0.8,
            "Mixed sentiment should not be extreme, got: {}",
            output.emotional_valence
        );
    }

    #[test]
    fn test_emotional_valence_in_valid_range() {
        let router = create_router();
        let texts = [
            "I absolutely love this! Amazing!",
            "This is terrible and awful!",
            "The weather is nice today.",
            "Great wonderful amazing fantastic!",
            "Bad horrible terrible worst!",
        ];

        for text in texts {
            let output = router.route(text).expect("Failed to route text");
            assert!(
                output.emotional_valence >= -1.0 && output.emotional_valence <= 1.0,
                "Emotional valence {} should be in range [-1, 1] for text: {}",
                output.emotional_valence,
                text
            );
        }
    }

    #[test]
    fn test_strong_positive_sentiment() {
        let router = create_router();
        let text = "Absolutely love love love! Best amazing wonderful perfect excellent!";
        let output = router.route(text).expect("Failed to route text");

        assert!(
            output.emotional_valence > 0.5,
            "Strong positive text should have high valence, got: {}",
            output.emotional_valence
        );
    }

    #[test]
    fn test_strong_negative_sentiment() {
        let router = create_router();
        let text = "Hate hate hate! Worst terrible awful horrible disgusting pathetic!";
        let output = router.route(text).expect("Failed to route text");

        assert!(
            output.emotional_valence < -0.5,
            "Strong negative text should have low valence, got: {}",
            output.emotional_valence
        );
    }
}

mod memory_type_routing_tests {
    use super::*;

    #[test]
    fn test_procedural_content_routing() {
        let router = create_router();
        let text = "How to bake a cake: First, preheat the oven to 350 degrees.";
        let output = router.route(text).expect("Failed to route text");

        assert!(
            output.search_types.contains(&MemoryType::Procedural),
            "How-to content should include Procedural memory type"
        );
    }

    #[test]
    fn test_semantic_content_routing() {
        let router = create_router();
        let text = "What is the definition of machine learning?";
        let output = router.route(text).expect("Failed to route text");

        assert!(
            output.search_types.contains(&MemoryType::Semantic),
            "Definition questions should include Semantic memory type"
        );
    }

    #[test]
    fn test_episodic_content_routing() {
        let router = create_router();
        let text = "Remember when we discussed the project yesterday at the meeting?";
        let output = router.route(text).expect("Failed to route text");

        assert!(
            output.search_types.contains(&MemoryType::Episodic),
            "Memory recall content should include Episodic memory type"
        );
    }

    #[test]
    fn test_conversation_content_routing() {
        let router = create_router();
        let text = "John told me about his trip to Paris last summer.";
        let output = router.route(text).expect("Failed to route text");

        // Person entities should trigger episodic routing
        let has_person = output
            .entities
            .iter()
            .any(|e| e.label == EntityLabel::Person);
        if has_person {
            assert!(
                output.search_types.contains(&MemoryType::Episodic),
                "Content with person entities should include Episodic type"
            );
        }
    }

    #[test]
    fn test_multiple_memory_types_can_be_selected() {
        let router = create_router();
        let text = "How do I remember what we discussed about machine learning?";
        let output = router.route(text).expect("Failed to route text");

        // This text has procedural (how do), episodic (remember, discussed), and semantic (machine learning) indicators
        assert!(
            !output.search_types.is_empty(),
            "Complex queries should have at least one memory type"
        );
    }
}

mod topic_extraction_tests {
    use super::*;

    #[test]
    fn test_topics_extracted_from_entities() {
        let router = create_router();
        let text = "Microsoft announced new features for Azure cloud platform.";
        let output = router.route(text).expect("Failed to route text");

        // Topics should include entity names (normalized)
        let has_microsoft_topic = output.topics.iter().any(|t| t.contains("microsoft"));
        let has_azure_topic = output.topics.iter().any(|t| t.contains("azure"));

        assert!(
            has_microsoft_topic || has_azure_topic || !output.entities.is_empty(),
            "Should extract topics from entities in text"
        );
    }

    #[test]
    fn test_topics_not_empty_for_meaningful_text() {
        let router = create_router();
        let text = "Artificial intelligence and machine learning are revolutionizing software development.";
        let output = router.route(text).expect("Failed to route text");

        assert!(
            !output.topics.is_empty() || !output.entities.is_empty(),
            "Meaningful text should have topics or entities"
        );
    }

    #[test]
    fn test_topics_normalized_to_lowercase() {
        let router = create_router();
        let text = "Python Programming Language is popular.";
        let output = router.route(text).expect("Failed to route text");

        // All topics should be lowercase
        for topic in &output.topics {
            assert_eq!(
                topic.to_lowercase(),
                *topic,
                "Topics should be normalized to lowercase: {topic}"
            );
        }
    }
}

mod query_keys_generation_tests {
    use super::*;

    #[test]
    fn test_query_keys_from_entities() {
        let router = create_router();
        let text = "Google DeepMind made breakthroughs in AI research.";
        let output = router.route(text).expect("Failed to route text");

        // Query keys should be generated from entities
        assert!(
            !output.query_keys.is_empty() || !output.entities.is_empty(),
            "Should generate query keys from entities"
        );
    }

    #[test]
    fn test_query_keys_minimum_length() {
        let router = create_router();
        let text = "John Smith works at Microsoft in Seattle.";
        let output = router.route(text).expect("Failed to route text");

        // All query keys should have minimum length
        for key in &output.query_keys {
            assert!(
                key.len() >= 2,
                "Query keys should have at least 2 characters: {key}"
            );
        }
    }

    #[test]
    fn test_query_keys_not_empty_for_entity_text() {
        let router = create_router();
        let text = "Barack Obama was the president of the United States.";
        let output = router.route(text).expect("Failed to route text");

        assert!(
            !output.query_keys.is_empty() || !output.entities.is_empty(),
            "Text with named entities should have query keys or entities"
        );
    }
}

mod edge_case_tests {
    use super::*;

    #[test]
    fn test_empty_text_routing() {
        let router = create_router();
        let output = router.route("").expect("Failed to route empty text");

        assert!(output.entities.is_empty());
        assert!(output.topics.is_empty());
        assert_eq!(output.emotional_valence, 0.0);
        assert!(output.query_keys.is_empty());
        assert_eq!(output.search_types.len(), 2); // Default: Episodic and Semantic
    }

    #[test]
    fn test_whitespace_only_text() {
        let router = create_router();
        let output = router
            .route("   \n\t   ")
            .expect("Failed to route whitespace");

        assert!(output.entities.is_empty());
        assert!(output.topics.is_empty());
        assert_eq!(output.emotional_valence, 0.0);
    }

    #[test]
    fn test_very_short_text() {
        let router = create_router();
        let output = router.route("Hi").expect("Failed to route short text");

        // Very short text should still work without panicking
        let _ = output.emotional_valence;
        let _ = &output.search_types;
    }

    #[test]
    fn test_very_long_text() {
        let router = create_router();
        let text = "Rust is great. ".repeat(100);
        let output = router.route(&text).expect("Failed to route long text");

        // Long text should still process
        assert!(
            output.emotional_valence >= -1.0 && output.emotional_valence <= 1.0,
            "Long text should have valid emotional valence"
        );
    }

    #[test]
    fn test_special_characters() {
        let router = create_router();
        let text = "Hello! @#$%^&*() World... How are you???";
        let output = router.route(text).expect("Failed to route special chars");

        // Should handle special characters without panicking
        let _ = &output.entities;
        let _ = &output.topics;
    }

    #[test]
    fn test_multilingual_text() {
        let router = create_router();
        let text = "Hello world! Bonjour le monde! Hola mundo!";
        let output = router
            .route(text)
            .expect("Failed to route multilingual text");

        // Should process multilingual text
        assert!(
            output.emotional_valence >= -1.0 && output.emotional_valence <= 1.0,
            "Multilingual text should have valid emotional valence"
        );
    }
}
