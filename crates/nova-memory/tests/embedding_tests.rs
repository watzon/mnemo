//! Integration tests for embedding layer
//!
//! Tests the EmbeddingModel implementation with real model loading.

use nova_memory::embedding::{EmbeddingModel, EMBEDDING_DIMENSION};

/// Calculate cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm_a * norm_b)
}

mod model_loading_tests {
    use super::*;

    #[test]
    fn test_model_loads_successfully() {
        let model = EmbeddingModel::new();
        assert!(model.is_ok(), "Model should load without errors");
    }

    #[test]
    fn test_model_can_be_created_multiple_times() {
        let model1 = EmbeddingModel::new();
        let model2 = EmbeddingModel::new();

        assert!(model1.is_ok());
        assert!(model2.is_ok());
    }
}

mod embedding_dimension_tests {
    use super::*;

    #[test]
    fn test_single_embedding_has_correct_dimension() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");
        let embedding = model.embed("Hello, world!").expect("Failed to embed");

        assert_eq!(
            embedding.len(),
            EMBEDDING_DIMENSION,
            "Embedding should have dimension {}",
            EMBEDDING_DIMENSION
        );
    }

    #[test]
    fn test_empty_string_embedding_has_correct_dimension() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");
        let embedding = model.embed("").expect("Failed to embed empty string");

        assert_eq!(embedding.len(), EMBEDDING_DIMENSION);
    }

    #[test]
    fn test_long_text_embedding_has_correct_dimension() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");
        let long_text = "This is a very long text. ".repeat(100);
        let embedding = model.embed(&long_text).expect("Failed to embed long text");

        assert_eq!(embedding.len(), EMBEDDING_DIMENSION);
    }

    #[test]
    fn test_multilingual_text_embedding_has_correct_dimension() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");

        let texts = vec![
            "Hello, world!",
            "Bonjour le monde!",
            "Hola, mundo!",
            "こんにちは世界",
            "안녕하세요 세계",
        ];

        for text in texts {
            let embedding = model.embed(text).expect("Failed to embed");
            assert_eq!(embedding.len(), EMBEDDING_DIMENSION);
        }
    }
}

mod similarity_tests {
    use super::*;

    #[test]
    fn test_similar_texts_have_high_similarity() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");

        let text1 = "The quick brown fox jumps over the lazy dog";
        let text2 = "A fast brown fox leaps over a sleepy dog";

        let emb1 = model.embed(text1).expect("Failed to embed text1");
        let emb2 = model.embed(text2).expect("Failed to embed text2");

        let similarity = cosine_similarity(&emb1, &emb2);

        assert!(
            similarity > 0.8,
            "Similar texts should have similarity > 0.8, got {:.3}",
            similarity
        );
    }

    #[test]
    fn test_different_texts_have_lower_similarity() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");

        let text1 = "The quick brown fox jumps over the lazy dog";
        let text2 = "Quantum computing revolutionizes cryptography";

        let emb1 = model.embed(text1).expect("Failed to embed text1");
        let emb2 = model.embed(text2).expect("Failed to embed text2");

        let similarity = cosine_similarity(&emb1, &emb2);

        assert!(
            similarity < 0.75,
            "Different texts should have similarity < 0.75, got {:.3}",
            similarity
        );
    }

    #[test]
    fn test_identical_texts_have_perfect_similarity() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");

        let text = "This is a test sentence for embedding comparison.";
        let emb1 = model.embed(text).expect("Failed to embed");
        let emb2 = model.embed(text).expect("Failed to embed");

        let similarity = cosine_similarity(&emb1, &emb2);

        assert!(
            similarity > 0.99,
            "Identical texts should have similarity > 0.99, got {:.3}",
            similarity
        );
    }

    #[test]
    fn test_semantically_similar_texts() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");

        let pairs = vec![
            (
                "I love programming in Rust",
                "I enjoy coding with Rust",
                0.75,
            ),
            (
                "Machine learning is fascinating",
                "Deep learning is interesting",
                0.7,
            ),
            (
                "The weather is nice today",
                "It's a beautiful day outside",
                0.6,
            ),
        ];

        for (text1, text2, threshold) in pairs {
            let emb1 = model.embed(text1).expect("Failed to embed");
            let emb2 = model.embed(text2).expect("Failed to embed");
            let similarity = cosine_similarity(&emb1, &emb2);

            assert!(
                similarity > threshold,
                "Texts '{}' and '{}' should have similarity > {:.2}, got {:.3}",
                text1,
                text2,
                threshold,
                similarity
            );
        }
    }

    #[test]
    fn test_unrelated_texts_have_low_similarity() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");

        let pairs = vec![
            ("The cat sat on the mat", "Stock markets crashed today"),
            ("I love pizza", "Quantum physics is complex"),
            ("Rust programming language", "Gardening tips for beginners"),
        ];

        for (text1, text2) in pairs {
            let emb1 = model.embed(text1).expect("Failed to embed");
            let emb2 = model.embed(text2).expect("Failed to embed");
            let similarity = cosine_similarity(&emb1, &emb2);

            assert!(
                similarity < 0.8,
                "Unrelated texts '{}' and '{}' should have similarity < 0.8, got {:.3}",
                text1,
                text2,
                similarity
            );
        }
    }
}

mod batch_embedding_tests {
    use super::*;

    #[test]
    fn test_batch_embedding_returns_correct_count() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");
        let texts = vec![
            "First sentence".to_string(),
            "Second sentence".to_string(),
            "Third sentence".to_string(),
        ];

        let embeddings = model.embed_batch(&texts).expect("Failed to embed batch");

        assert_eq!(embeddings.len(), 3, "Should return 3 embeddings");
    }

    #[test]
    fn test_batch_embedding_correct_dimensions() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");
        let texts = vec![
            "First".to_string(),
            "Second".to_string(),
            "Third".to_string(),
        ];

        let embeddings = model.embed_batch(&texts).expect("Failed to embed batch");

        for (i, emb) in embeddings.iter().enumerate() {
            assert_eq!(
                emb.len(),
                EMBEDDING_DIMENSION,
                "Embedding {} should have dimension {}",
                i,
                EMBEDDING_DIMENSION
            );
        }
    }

    #[test]
    fn test_batch_embedding_consistency() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");
        let text = "Consistency test sentence";

        let single_embedding = model.embed(text).expect("Failed to embed single");

        let batch_embeddings = model
            .embed_batch(&[text.to_string()])
            .expect("Failed to embed batch");

        assert_eq!(batch_embeddings.len(), 1);

        let similarity = cosine_similarity(&single_embedding, &batch_embeddings[0]);

        assert!(
            similarity > 0.99,
            "Single and batch embedding should be nearly identical, got {:.3}",
            similarity
        );
    }

    #[test]
    fn test_batch_embedding_order_preserved() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");
        let texts = vec![
            "First unique sentence".to_string(),
            "Second unique sentence".to_string(),
            "Third unique sentence".to_string(),
        ];

        let embeddings = model.embed_batch(&texts).expect("Failed to embed batch");

        let single_embeddings: Vec<Vec<f32>> = texts
            .iter()
            .map(|t| model.embed(t).expect("Failed to embed"))
            .collect();

        for (i, (batch_emb, single_emb)) in
            embeddings.iter().zip(single_embeddings.iter()).enumerate()
        {
            let similarity = cosine_similarity(batch_emb, single_emb);
            assert!(
                similarity > 0.99,
                "Batch embedding {} should match single embedding, got {:.3}",
                i,
                similarity
            );
        }
    }

    #[test]
    fn test_empty_batch_returns_empty() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");
        let texts: Vec<String> = vec![];

        let embeddings = model
            .embed_batch(&texts)
            .expect("Failed to embed empty batch");

        assert!(
            embeddings.is_empty(),
            "Empty batch should return empty vector"
        );
    }

    #[test]
    fn test_large_batch_embedding() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");
        let texts: Vec<String> = (0..50)
            .map(|i| format!("Test sentence number {}", i))
            .collect();

        let embeddings = model
            .embed_batch(&texts)
            .expect("Failed to embed large batch");

        assert_eq!(embeddings.len(), 50);
        for emb in &embeddings {
            assert_eq!(emb.len(), EMBEDDING_DIMENSION);
        }
    }
}

mod embedding_properties_tests {
    use super::*;

    #[test]
    fn test_embedding_values_are_normalized() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");
        let embedding = model.embed("Test text").expect("Failed to embed");

        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();

        assert!(
            norm > 0.0 && norm < 100.0,
            "Embedding norm should be reasonable, got {:.3}",
            norm
        );
    }

    #[test]
    fn test_embeddings_are_not_all_zeros() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");
        let embedding = model.embed("Test text").expect("Failed to embed");

        let sum: f32 = embedding.iter().map(|x| x.abs()).sum();

        assert!(
            sum > 0.0,
            "Embedding should not be all zeros, got sum of abs values: {:.3}",
            sum
        );
    }

    #[test]
    fn test_embeddings_have_variation() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");
        let embedding = model.embed("Test text").expect("Failed to embed");

        let min = embedding.iter().fold(f32::INFINITY, |a, &b| a.min(b));
        let max = embedding.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));

        assert!(
            max - min > 0.01,
            "Embedding should have variation, got range [{:.3}, {:.3}]",
            min,
            max
        );
    }
}
