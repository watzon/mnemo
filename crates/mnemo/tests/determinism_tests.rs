//! Integration tests for deterministic memory retrieval
//!
//! Tests verify that:
//! - Same query with same memories produces identical ordering
//! - Score quantization works correctly
//! - Tie-breaking uses timestamp then UUID
//! - Topic overlap boosts scores
//! - Non-deterministic mode is unchanged
//!
//! IMPORTANT: Run with --test-threads=1 due to ML model loading contention.
//! `cargo test -p mnemo --test determinism_tests -- --test-threads=1`

use chrono::{Duration, Utc};
use tempfile::TempDir;

use mnemo_server::config::DeterministicConfig;
use mnemo_server::memory::retrieval::{RetrievalConfig, RetrievalPipeline};
use mnemo_server::memory::types::{Memory, MemorySource, MemoryType};
use mnemo_server::storage::LanceStore;
use mnemo_server::testing::SHARED_EMBEDDING_MODEL;

async fn create_test_store() -> (LanceStore, TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
    store.create_memories_table().await.unwrap();
    (store, temp_dir)
}

fn create_memory_with_entities(
    content: &str,
    entities: Vec<String>,
    embedding: Vec<f32>,
) -> Memory {
    let mut memory = Memory::new(
        content.to_string(),
        embedding,
        MemoryType::Semantic,
        MemorySource::Manual,
    );
    memory.entities = entities;
    memory
}

fn create_memory_with_timestamp(
    content: &str,
    embedding: Vec<f32>,
    age_days: i64,
    weight: f32,
) -> Memory {
    let mut memory = Memory::new(
        content.to_string(),
        embedding,
        MemoryType::Semantic,
        MemorySource::Manual,
    );
    memory.created_at = Utc::now() - Duration::days(age_days);
    memory.weight = weight;
    memory
}

#[tokio::test]
async fn test_deterministic_same_query_same_order() {
    let (store, _temp_dir) = create_test_store().await;
    let embedding: Vec<f32> = vec![0.5; 384];

    for i in 0..5 {
        let mut mem = create_memory_with_entities(
            &format!("Memory {i}"),
            vec![format!("entity_{i}")],
            embedding.clone(),
        );
        mem.weight = (i as f32) * 0.1 + 0.5;
        store.insert(&mem).await.unwrap();
    }

    let det_config = DeterministicConfig {
        enabled: true,
        decimal_places: 2,
        topic_overlap_weight: 0.1,
    };

    let config = RetrievalConfig {
        deterministic_config: Some(det_config),
        ..Default::default()
    };

    let embedding_model = &*SHARED_EMBEDDING_MODEL;
    let mut pipeline = RetrievalPipeline::new(&store, embedding_model, config.clone());

    let results1 = pipeline
        .retrieve_by_embedding(&embedding, 10)
        .await
        .unwrap();

    let ids1: Vec<_> = results1.iter().map(|r| r.memory.id).collect();

    let mut pipeline2 = RetrievalPipeline::new(&store, embedding_model, config);
    let results2 = pipeline2
        .retrieve_by_embedding(&embedding, 10)
        .await
        .unwrap();

    let ids2: Vec<_> = results2.iter().map(|r| r.memory.id).collect();

    assert_eq!(results1.len(), results2.len());
    assert_eq!(
        ids1, ids2,
        "Memory ordering should be identical between runs"
    );
}

#[tokio::test]
async fn test_deterministic_tiebreaker_by_timestamp() {
    let (store, _temp_dir) = create_test_store().await;
    let embedding: Vec<f32> = vec![0.5; 384];

    let older_mem = create_memory_with_timestamp("Older memory", embedding.clone(), 2, 0.5);
    let newer_mem = create_memory_with_timestamp("Newer memory", embedding.clone(), 1, 0.5);

    store.insert(&newer_mem).await.unwrap();
    store.insert(&older_mem).await.unwrap();

    let det_config = DeterministicConfig {
        enabled: true,
        decimal_places: 1,
        topic_overlap_weight: 0.0,
    };

    let config = RetrievalConfig {
        deterministic_config: Some(det_config),
        ..Default::default()
    };

    let embedding_model = &*SHARED_EMBEDDING_MODEL;
    let mut pipeline = RetrievalPipeline::new(&store, embedding_model, config);

    let results = pipeline
        .retrieve_by_embedding(&embedding, 10)
        .await
        .unwrap();

    assert_eq!(results.len(), 2);
    let scores_tied = results[0].final_score == results[1].final_score;
    if scores_tied {
        assert!(
            results[0].memory.content.contains("Older"),
            "When scores tie, older memory should rank first: got {}",
            results[0].memory.content
        );
    } else {
        assert!(
            results[0].final_score > results[1].final_score,
            "Higher scored memory should rank first"
        );
    }
}

#[tokio::test]
async fn test_deterministic_tiebreaker_by_id() {
    let (store, _temp_dir) = create_test_store().await;
    let embedding: Vec<f32> = vec![0.5; 384];
    let timestamp = Utc::now();

    let mut mem1 = Memory::new(
        "Memory A".to_string(),
        embedding.clone(),
        MemoryType::Semantic,
        MemorySource::Manual,
    );
    mem1.created_at = timestamp;
    mem1.weight = 0.5;

    let mut mem2 = Memory::new(
        "Memory B".to_string(),
        embedding.clone(),
        MemoryType::Semantic,
        MemorySource::Manual,
    );
    mem2.created_at = timestamp;
    mem2.weight = 0.5;

    store.insert(&mem1).await.unwrap();
    store.insert(&mem2).await.unwrap();

    let det_config = DeterministicConfig {
        enabled: true,
        decimal_places: 2,
        topic_overlap_weight: 0.0,
    };

    let config = RetrievalConfig {
        deterministic_config: Some(det_config),
        ..Default::default()
    };

    let embedding_model = &*SHARED_EMBEDDING_MODEL;
    let mut pipeline = RetrievalPipeline::new(&store, embedding_model, config.clone());

    let results1 = pipeline
        .retrieve_by_embedding(&embedding, 10)
        .await
        .unwrap();

    let mut pipeline2 = RetrievalPipeline::new(&store, embedding_model, config);
    let results2 = pipeline2
        .retrieve_by_embedding(&embedding, 10)
        .await
        .unwrap();

    assert_eq!(results1.len(), 2);
    assert_eq!(results1[0].memory.id, results2[0].memory.id);
    assert_eq!(results1[1].memory.id, results2[1].memory.id);
}

#[tokio::test]
async fn test_topic_overlap_boosts_score() {
    let (store, _temp_dir) = create_test_store().await;
    let embedding: Vec<f32> = vec![0.5; 384];

    let mem_with_entities = create_memory_with_entities(
        "Memory with matching entities",
        vec!["Rust".to_string(), "Python".to_string()],
        embedding.clone(),
    );

    let mem_no_entities =
        create_memory_with_entities("Memory without entities", vec![], embedding.clone());

    store.insert(&mem_no_entities).await.unwrap();
    store.insert(&mem_with_entities).await.unwrap();

    let query_entities = vec!["Rust".to_string(), "Python".to_string()];

    let det_config = DeterministicConfig {
        enabled: true,
        decimal_places: 2,
        topic_overlap_weight: 0.5,
    };

    let config = RetrievalConfig {
        deterministic_config: Some(det_config),
        ..Default::default()
    };

    let embedding_model = &*SHARED_EMBEDDING_MODEL;
    let mut pipeline = RetrievalPipeline::new(&store, embedding_model, config);

    let results = pipeline
        .retrieve_by_embedding_filtered_with_entities(
            &embedding,
            &Default::default(),
            10,
            Some(&query_entities),
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 2);
    assert!(
        results[0].memory.content.contains("matching entities"),
        "Memory with matching entities should rank first: got {}",
        results[0].memory.content
    );
    assert!(
        results[0].final_score > results[1].final_score,
        "Memory with entities should have higher score"
    );
}

#[tokio::test]
async fn test_nondeterministic_mode_unchanged() {
    let (store, _temp_dir) = create_test_store().await;
    let embedding: Vec<f32> = vec![0.5; 384];

    let mut mem1 = Memory::new(
        "High weight".to_string(),
        embedding.clone(),
        MemoryType::Semantic,
        MemorySource::Manual,
    );
    mem1.weight = 1.0;

    let mut mem2 = Memory::new(
        "Low weight".to_string(),
        embedding.clone(),
        MemoryType::Semantic,
        MemorySource::Manual,
    );
    mem2.weight = 0.1;

    store.insert(&mem2).await.unwrap();
    store.insert(&mem1).await.unwrap();

    let config = RetrievalConfig {
        deterministic_config: None,
        ..Default::default()
    };

    let embedding_model = &*SHARED_EMBEDDING_MODEL;
    let mut pipeline = RetrievalPipeline::new(&store, embedding_model, config);

    let results = pipeline
        .retrieve_by_embedding(&embedding, 10)
        .await
        .unwrap();

    assert_eq!(results.len(), 2);
    assert!(
        results[0].memory.content.contains("High weight"),
        "High weight memory should rank first in non-deterministic mode"
    );
}
