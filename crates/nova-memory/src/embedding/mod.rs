use crate::NovaError;
use fastembed::{EmbeddingModel as FastEmbedModel, InitOptions, TextEmbedding};

pub const EMBEDDING_DIMENSION: usize = 384;

pub struct EmbeddingModel {
    model: TextEmbedding,
}

impl EmbeddingModel {
    pub fn new() -> Result<Self, NovaError> {
        let model = TextEmbedding::try_new(InitOptions::new(FastEmbedModel::MultilingualE5Small))
            .map_err(|e| NovaError::Embedding(e.to_string()))?;
        Ok(Self { model })
    }

    pub fn embed(&mut self, text: &str) -> Result<Vec<f32>, NovaError> {
        let embeddings = self
            .model
            .embed(vec![text.to_string()], None)
            .map_err(|e| NovaError::Embedding(e.to_string()))?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| NovaError::Embedding("No embedding returned".to_string()))
    }

    pub fn embed_batch(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, NovaError> {
        self.model
            .embed(texts.to_vec(), None)
            .map_err(|e| NovaError::Embedding(e.to_string()))
    }
}

#[cfg(test)]
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_loads() {
        let model = EmbeddingModel::new();
        assert!(model.is_ok(), "Model should load successfully");
    }

    #[test]
    fn test_embed_returns_correct_dimension() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");
        let embedding = model.embed("Hello, world!").expect("Failed to embed");
        assert_eq!(
            embedding.len(),
            EMBEDDING_DIMENSION,
            "Embedding dimension should be {}",
            EMBEDDING_DIMENSION
        );
    }

    #[test]
    fn test_similar_texts_have_high_similarity() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");

        let text1 = "The quick brown fox jumps over the lazy dog";
        let text2 = "A fast brown fox leaps over a sleepy dog";
        let text3 = "Quantum computing revolutionizes cryptography";

        let emb1 = model.embed(text1).expect("Failed to embed text1");
        let emb2 = model.embed(text2).expect("Failed to embed text2");
        let emb3 = model.embed(text3).expect("Failed to embed text3");

        let sim_similar = cosine_similarity(&emb1, &emb2);
        let sim_different = cosine_similarity(&emb1, &emb3);

        assert!(
            sim_similar > sim_different,
            "Similar texts ({:.3}) should have higher similarity than different texts ({:.3})",
            sim_similar,
            sim_different
        );
        assert!(
            sim_similar > 0.8,
            "Similar texts should have similarity > 0.8, got {:.3}",
            sim_similar
        );
    }

    #[test]
    fn test_batch_embedding() {
        let mut model = EmbeddingModel::new().expect("Failed to load model");
        let texts = vec![
            "First sentence".to_string(),
            "Second sentence".to_string(),
            "Third sentence".to_string(),
        ];
        let embeddings = model.embed_batch(&texts).expect("Failed to embed batch");
        assert_eq!(embeddings.len(), 3, "Should return 3 embeddings");
        for emb in &embeddings {
            assert_eq!(
                emb.len(),
                EMBEDDING_DIMENSION,
                "Each embedding should have dimension {}",
                EMBEDDING_DIMENSION
            );
        }
    }
}
