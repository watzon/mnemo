//! Integration tests for the ingestion pipeline
//!
//! Tests the full end-to-end ingestion flow from text to stored memory.
//! These tests verify filtering, memory creation, and storage integration.

use mnemo::memory::ingestion::IngestionPipeline;
use mnemo::memory::types::{CompressionLevel, MemorySource, MemoryType, StorageTier};
use mnemo::storage::LanceStore;
use tempfile::tempdir;

/// Test helper: Create a test store in a temporary directory
async fn create_test_store() -> (LanceStore, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let mut store = LanceStore::connect(dir.path()).await.unwrap();
    store.create_memories_table().await.unwrap();
    (store, dir)
}

mod end_to_end_pipeline_tests {
    use super::*;

    #[tokio::test]
    async fn test_full_pipeline_creates_memory() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline
            .ingest(
                "This is a comprehensive test of the full ingestion pipeline.",
                MemorySource::Manual,
                None,
            )
            .await;

        assert!(result.is_ok(), "Pipeline should complete without errors");
        let memory = result.unwrap();
        assert!(memory.is_some(), "Pipeline should create a memory");

        let memory = memory.unwrap();
        assert!(!memory.content.is_empty());
        assert_eq!(memory.embedding.len(), 384);
        assert!(memory.weight > 0.0);
    }

    #[tokio::test]
    async fn test_pipeline_with_conversation_source() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline
            .ingest(
                "User mentioned they prefer Python for data science tasks.",
                MemorySource::Conversation,
                Some("conv-456".to_string()),
            )
            .await;

        let memory = result.unwrap().unwrap();
        assert_eq!(memory.memory_type, MemoryType::Episodic);
        assert_eq!(memory.conversation_id, Some("conv-456".to_string()));
        assert_eq!(memory.source, MemorySource::Conversation);
    }

    #[tokio::test]
    async fn test_pipeline_with_file_source() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline
            .ingest(
                "Important information extracted from a document file.",
                MemorySource::File,
                None,
            )
            .await;

        let memory = result.unwrap().unwrap();
        assert_eq!(memory.memory_type, MemoryType::Semantic);
        assert_eq!(memory.source, MemorySource::File);
    }

    #[tokio::test]
    async fn test_pipeline_with_web_source() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline
            .ingest(
                "Information scraped from a website about machine learning.",
                MemorySource::Web,
                None,
            )
            .await;

        let memory = result.unwrap().unwrap();
        assert_eq!(memory.memory_type, MemoryType::Semantic);
        assert_eq!(memory.source, MemorySource::Web);
    }

    #[tokio::test]
    async fn test_pipeline_generates_valid_embedding() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline
            .ingest(
                "Test content for embedding validation.",
                MemorySource::Manual,
                None,
            )
            .await;

        let memory = result.unwrap().unwrap();
        assert_eq!(memory.embedding.len(), 384);

        // Embedding should not be all zeros
        let sum: f32 = memory.embedding.iter().sum();
        assert!(sum.abs() > 0.0, "Embedding should not be all zeros");
    }

    #[tokio::test]
    async fn test_pipeline_extracts_entities() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline
            .ingest(
                "John Smith from Microsoft visited Google headquarters in Mountain View.",
                MemorySource::Manual,
                None,
            )
            .await;

        let memory = result.unwrap().unwrap();
        // Weight should be boosted by entity extraction
        assert!(memory.weight >= 0.5, "Weight should be at least base value");
    }

    #[tokio::test]
    async fn test_pipeline_stores_in_lancedb() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();

        let memory_id = {
            let mut store = LanceStore::connect(&path).await.unwrap();
            store.create_memories_table().await.unwrap();

            let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");
            let result = pipeline
                .ingest(
                    "This memory should be persisted in LanceDB storage.",
                    MemorySource::Manual,
                    None,
                )
                .await;

            result.unwrap().unwrap().id
        };

        // Reopen store and verify persistence
        let mut store2 = LanceStore::connect(&path).await.unwrap();
        store2.open_memories_table().await.unwrap();

        let retrieved = store2.get(memory_id).await.unwrap();
        assert!(retrieved.is_some(), "Memory should be persisted in storage");
        assert_eq!(
            retrieved.unwrap().content,
            "This memory should be persisted in LanceDB storage."
        );
    }

    #[tokio::test]
    async fn test_pipeline_multiple_ingestions() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let texts = [
            "First memory about Rust programming.",
            "Second memory about Python data science.",
            "Third memory about JavaScript web development.",
        ];

        let mut ids = vec![];
        for text in &texts {
            let result = pipeline.ingest(text, MemorySource::Manual, None).await;
            ids.push(result.unwrap().unwrap().id);
        }

        assert_eq!(ids.len(), 3);
        // All IDs should be unique
        let unique_ids: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique_ids.len(), 3, "All memory IDs should be unique");
    }
}

mod content_filtering_tests {
    use super::*;

    #[tokio::test]
    async fn test_filter_empty_content() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline.ingest("", MemorySource::Manual, None).await;
        assert!(result.is_ok());
        assert!(
            result.unwrap().is_none(),
            "Empty content should be filtered"
        );
    }

    #[tokio::test]
    async fn test_filter_whitespace_only_content() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let whitespaces = ["   ", "\n\t  \n", "     ", "\t\t\t"];

        for ws in &whitespaces {
            let result = pipeline.ingest(ws, MemorySource::Manual, None).await;
            assert!(result.is_ok());
            assert!(
                result.unwrap().is_none(),
                "Whitespace-only content '{ws}' should be filtered"
            );
        }
    }

    #[tokio::test]
    async fn test_filter_short_content() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        // Content shorter than 10 characters should be filtered
        let result = pipeline.ingest("short", MemorySource::Manual, None).await;
        assert!(result.is_ok());
        assert!(
            result.unwrap().is_none(),
            "Short content (< 10 chars) should be filtered"
        );
    }

    #[tokio::test]
    async fn test_accept_minimum_length_content() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        // Content with exactly 10 characters should be accepted
        let result = pipeline
            .ingest("1234567890", MemorySource::Manual, None)
            .await;
        assert!(result.is_ok());
        assert!(
            result.unwrap().is_some(),
            "Content with exactly 10 chars should be accepted"
        );
    }

    #[tokio::test]
    async fn test_accept_long_content() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let long_text = "This is a very long piece of content that should definitely be accepted by the filtering mechanism.".repeat(10);
        let result = pipeline
            .ingest(&long_text, MemorySource::Manual, None)
            .await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_some(), "Long content should be accepted");
    }

    #[tokio::test]
    async fn test_filter_content_at_boundary() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        // 9 characters - should be filtered
        let result = pipeline
            .ingest("123456789", MemorySource::Manual, None)
            .await;
        assert!(
            result.unwrap().is_none(),
            "9 char content should be filtered"
        );

        // 10 characters - should be accepted
        let result = pipeline
            .ingest("1234567890", MemorySource::Manual, None)
            .await;
        assert!(
            result.unwrap().is_some(),
            "10 char content should be accepted"
        );

        // 11 characters - should be accepted
        let result = pipeline
            .ingest("12345678901", MemorySource::Manual, None)
            .await;
        assert!(
            result.unwrap().is_some(),
            "11 char content should be accepted"
        );
    }
}

mod memory_type_assignment_tests {
    use super::*;

    #[tokio::test]
    async fn test_conversation_source_assigns_episodic() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let sources = [MemorySource::Conversation];

        for source in &sources {
            let result = pipeline
                .ingest(
                    "User said they like pizza for dinner.",
                    *source,
                    Some("test-conv".to_string()),
                )
                .await;

            let memory = result.unwrap().unwrap();
            assert_eq!(
                memory.memory_type,
                MemoryType::Episodic,
                "Conversation source should create Episodic memory"
            );
        }
    }

    #[tokio::test]
    async fn test_non_conversation_sources_assign_semantic() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let sources = [MemorySource::Manual, MemorySource::File, MemorySource::Web];

        for source in &sources {
            let result = pipeline
                .ingest("Important fact to remember.", *source, None)
                .await;

            let memory = result.unwrap().unwrap();
            assert_eq!(
                memory.memory_type,
                MemoryType::Semantic,
                "{source:?} source should create Semantic memory"
            );
        }
    }

    #[tokio::test]
    async fn test_conversation_id_preserved() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let conversation_id = "conversation-123-abc";
        let result = pipeline
            .ingest(
                "User preference noted.",
                MemorySource::Conversation,
                Some(conversation_id.to_string()),
            )
            .await;

        let memory = result.unwrap().unwrap();
        assert_eq!(
            memory.conversation_id,
            Some(conversation_id.to_string()),
            "Conversation ID should be preserved"
        );
    }

    #[tokio::test]
    async fn test_no_conversation_id_for_non_conversation() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline
            .ingest(
                "Manual entry without conversation.",
                MemorySource::Manual,
                None,
            )
            .await;

        let memory = result.unwrap().unwrap();
        assert!(
            memory.conversation_id.is_none(),
            "Non-conversation memory should not have conversation ID"
        );
    }
}

mod compression_level_tests {
    use super::*;

    #[tokio::test]
    async fn test_short_content_gets_full_compression() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline
            .ingest("Short text.", MemorySource::Manual, None)
            .await;

        let memory = result.unwrap().unwrap();
        assert_eq!(
            memory.compression,
            CompressionLevel::Full,
            "Short content (< 100 chars) should have Full compression"
        );
    }

    #[tokio::test]
    async fn test_medium_content_gets_summary_compression() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let text = "a".repeat(250); // 250 characters
        let result = pipeline.ingest(&text, MemorySource::Manual, None).await;

        let memory = result.unwrap().unwrap();
        assert_eq!(
            memory.compression,
            CompressionLevel::Summary,
            "Medium content (100-499 chars) should have Summary compression"
        );
    }

    #[tokio::test]
    async fn test_long_content_gets_keywords_compression() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let text = "a".repeat(1000); // 1000 characters
        let result = pipeline.ingest(&text, MemorySource::Manual, None).await;

        let memory = result.unwrap().unwrap();
        assert_eq!(
            memory.compression,
            CompressionLevel::Keywords,
            "Long content (500-1999 chars) should have Keywords compression"
        );
    }

    #[tokio::test]
    async fn test_very_long_content_gets_hash_compression() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let text = "a".repeat(2500); // 2500 characters
        let result = pipeline.ingest(&text, MemorySource::Manual, None).await;

        let memory = result.unwrap().unwrap();
        assert_eq!(
            memory.compression,
            CompressionLevel::Hash,
            "Very long content (>= 2000 chars) should have Hash compression"
        );
    }
}

mod weight_calculation_tests {
    use super::*;

    #[tokio::test]
    async fn test_weight_in_valid_range() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline
            .ingest(
                "Test content for weight validation.",
                MemorySource::Manual,
                None,
            )
            .await;

        let memory = result.unwrap().unwrap();
        assert!(
            memory.weight >= 0.0 && memory.weight <= 1.0,
            "Weight should be in range [0, 1], got: {}",
            memory.weight
        );
    }

    #[tokio::test]
    async fn test_base_weight_minimum() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        // Content with no entities should have base weight
        let result = pipeline
            .ingest(
                "The quick brown fox jumps over the lazy dog.",
                MemorySource::Manual,
                None,
            )
            .await;

        let memory = result.unwrap().unwrap();
        assert!(
            memory.weight >= 0.5,
            "Base weight should be at least 0.5, got: {}",
            memory.weight
        );
    }

    #[tokio::test]
    async fn test_weight_boosted_by_entities() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        // Content with named entities should have higher weight
        let result = pipeline
            .ingest(
                "John Smith from Microsoft and Google discussed AI with Sarah Johnson.",
                MemorySource::Manual,
                None,
            )
            .await;

        let memory = result.unwrap().unwrap();
        assert!(
            memory.weight > 0.5,
            "Weight should be boosted by entities, got: {}",
            memory.weight
        );
    }

    #[tokio::test]
    async fn test_weight_capped_at_maximum() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        // Even with many entities, weight should not exceed 1.0
        let result = pipeline
            .ingest(
                "John Smith, Jane Doe, Bob Wilson, Alice Brown, Charlie Davis, Eve Miller, Frank White, Grace Lee, Henry Taylor, Ivy Chen all attended.",
                MemorySource::Manual,
                None,
            )
            .await;

        let memory = result.unwrap().unwrap();
        assert!(
            memory.weight <= 1.0,
            "Weight should be capped at 1.0, got: {}",
            memory.weight
        );
    }
}

mod storage_tier_tests {
    use super::*;

    #[tokio::test]
    async fn test_new_memory_gets_hot_tier() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let result = pipeline
            .ingest(
                "New memory should be in hot tier.",
                MemorySource::Manual,
                None,
            )
            .await;

        let memory = result.unwrap().unwrap();
        assert_eq!(
            memory.tier,
            StorageTier::Hot,
            "New memory should be in Hot tier"
        );
    }
}

mod edge_case_tests {
    use super::*;

    #[tokio::test]
    async fn test_unicode_content() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let unicode_texts = [
            "Hello 世界! 🌍",
            "Привет мир!",
            "مرحبا بالعالم",
            "🎉 Celebration! 🎊",
        ];

        for text in &unicode_texts {
            let result = pipeline.ingest(text, MemorySource::Manual, None).await;
            assert!(result.is_ok(), "Should handle unicode content: {text}");
            // Should either create memory or filter (both are valid)
            let _ = result.unwrap();
        }
    }

    #[tokio::test]
    async fn test_special_characters_in_content() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let special_text = "Special chars: @#$%^&*()_+-=[]{}|;':\",./<>?";
        let result = pipeline
            .ingest(special_text, MemorySource::Manual, None)
            .await;

        assert!(result.is_ok(), "Should handle special characters");
    }

    #[tokio::test]
    async fn test_multiline_content() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        let multiline_text = "Line 1\nLine 2\nLine 3\n\nLine after blank";
        let result = pipeline
            .ingest(multiline_text, MemorySource::Manual, None)
            .await;

        assert!(result.is_ok());
        let memory = result.unwrap();
        if let Some(m) = memory {
            assert!(m.content.contains("Line 1"));
            assert!(m.content.contains("Line 2"));
        }
    }

    #[tokio::test]
    async fn test_very_long_content() {
        let (store, _dir) = create_test_store().await;
        let mut pipeline = IngestionPipeline::new_owned(store).expect("Failed to create pipeline");

        // Test with content that's long but within BERT's 512 token limit
        // BERT tokenizer typically produces ~1.3 tokens per word, so 300 words is safe
        let long_content = "This is a test sentence with meaningful content. ".repeat(50);
        let result = pipeline
            .ingest(&long_content, MemorySource::Manual, None)
            .await;

        assert!(
            result.is_ok(),
            "Should handle long content: {:?}",
            result.err()
        );
        assert!(result.unwrap().is_some(), "Long content should be accepted");
    }
}
