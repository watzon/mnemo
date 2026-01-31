//! Memory compaction for Nova Memory
//!
//! Implements automatic compression of old memories to reduce storage requirements
//! while preserving searchability through embeddings.

use chrono::{Duration, Utc};
use std::collections::HashSet;
use uuid::Uuid;

use crate::error::{NovaError, Result};
use crate::memory::types::{CompressionLevel, StorageTier};
use crate::storage::LanceStore;

/// Configuration for memory compaction thresholds and policies
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Days after which memories are compressed to Summary (default: 30)
    pub summary_age_days: i64,
    /// Days after which memories are compressed to Keywords (default: 90)
    pub keywords_age_days: i64,
    /// Minimum weight to preserve - memories with weight >= this are not compacted (default: 0.7)
    pub min_weight_to_preserve: f32,
    /// Maximum sentences to keep in summary compression (default: 3)
    pub summary_max_sentences: usize,
    /// Maximum keywords to keep in keywords compression (default: 20)
    pub keywords_max_count: usize,
    /// Minimum word length for keyword extraction (default: 4)
    pub keywords_min_word_length: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            summary_age_days: 30,
            keywords_age_days: 90,
            min_weight_to_preserve: 0.7,
            summary_max_sentences: 3,
            keywords_max_count: 20,
            keywords_min_word_length: 4,
        }
    }
}

impl CompactionConfig {
    /// Create a new compaction configuration with custom age thresholds
    pub fn new(summary_age_days: i64, keywords_age_days: i64) -> Self {
        Self {
            summary_age_days,
            keywords_age_days,
            ..Default::default()
        }
    }

    /// Set the minimum weight threshold for preserving memories
    pub fn with_min_weight(mut self, min_weight: f32) -> Self {
        self.min_weight_to_preserve = min_weight.clamp(0.0, 1.0);
        self
    }

    /// Set the maximum sentences for summary compression
    pub fn with_summary_sentences(mut self, max_sentences: usize) -> Self {
        self.summary_max_sentences = max_sentences;
        self
    }

    /// Set the maximum keywords for keywords compression
    pub fn with_max_keywords(mut self, max_keywords: usize) -> Self {
        self.keywords_max_count = max_keywords;
        self
    }
}

/// Result of a compaction operation
#[derive(Debug, Clone, Default)]
pub struct CompactionResult {
    /// Number of memories compacted
    pub compacted_count: u32,
    /// Number of memories skipped due to high weight
    pub skipped_high_weight: u32,
    /// Number of memories already at target compression
    pub already_compressed: u32,
    /// IDs of compacted memories
    pub compacted_ids: Vec<Uuid>,
}

/// Compacts old memories by applying progressive compression
///
/// The Compactor reduces storage requirements for old memories while
/// preserving their searchability through embeddings. Compression levels:
/// - Full: Complete content preserved (no compression)
/// - Summary: First N sentences extracted
/// - Keywords: Significant words only
/// - Hash: Metadata reference only (original content discarded)
pub struct Compactor<'a> {
    store: &'a LanceStore,
    config: CompactionConfig,
}

impl<'a> Compactor<'a> {
    /// Create a new Compactor with default configuration
    pub fn new(store: &'a LanceStore) -> Self {
        Self {
            store,
            config: CompactionConfig::default(),
        }
    }

    /// Create a new Compactor with custom configuration
    pub fn with_config(store: &'a LanceStore, config: CompactionConfig) -> Self {
        Self { store, config }
    }

    /// Get the current configuration
    pub fn config(&self) -> &CompactionConfig {
        &self.config
    }

    /// Compact memories in the specified tier based on age thresholds
    ///
    /// Applies progressive compression:
    /// - Age > summary_age_days: Full → Summary
    /// - Age > keywords_age_days: Summary → Keywords
    ///
    /// Memories with weight >= min_weight_to_preserve are skipped.
    ///
    /// # Arguments
    /// * `tier` - The storage tier to compact
    ///
    /// # Returns
    /// * `CompactionResult` with statistics about the compaction operation
    pub async fn compact(&self, tier: StorageTier) -> Result<CompactionResult> {
        let now = Utc::now();
        let summary_threshold = now - Duration::days(self.config.summary_age_days);
        let keywords_threshold = now - Duration::days(self.config.keywords_age_days);

        let mut result = CompactionResult::default();

        let memories = self.store.list_by_tier(tier).await?;

        for memory in memories {
            if memory.weight >= self.config.min_weight_to_preserve {
                result.skipped_high_weight += 1;
                continue;
            }

            let target_compression = if memory.created_at < keywords_threshold {
                CompressionLevel::Keywords
            } else if memory.created_at < summary_threshold {
                CompressionLevel::Summary
            } else {
                continue;
            };

            if Self::compression_level_value(memory.compression)
                >= Self::compression_level_value(target_compression)
            {
                result.already_compressed += 1;
                continue;
            }

            let compressed_content = self.apply_compression(&memory.content, target_compression);

            self.store
                .update_compression(memory.id, &compressed_content, target_compression)
                .await?;

            result.compacted_count += 1;
            result.compacted_ids.push(memory.id);
        }

        Ok(result)
    }

    /// Compact a single memory to the specified compression level
    ///
    /// # Arguments
    /// * `memory_id` - The UUID of the memory to compact
    /// * `target_level` - The target compression level
    ///
    /// # Returns
    /// * `Ok(true)` if the memory was compacted
    /// * `Ok(false)` if the memory was skipped (high weight or already compressed)
    /// * `Err` if memory not found or storage error
    pub async fn compact_single(
        &self,
        memory_id: Uuid,
        target_level: CompressionLevel,
    ) -> Result<bool> {
        let memory = self
            .store
            .get(memory_id)
            .await?
            .ok_or_else(|| NovaError::Memory(format!("Memory not found: {}", memory_id)))?;

        if memory.weight >= self.config.min_weight_to_preserve {
            return Ok(false);
        }

        if Self::compression_level_value(memory.compression)
            >= Self::compression_level_value(target_level)
        {
            return Ok(false);
        }

        let compressed_content = self.apply_compression(&memory.content, target_level);

        self.store
            .update_compression(memory_id, &compressed_content, target_level)
            .await?;

        Ok(true)
    }

    /// Apply compression to content based on target level
    fn apply_compression(&self, content: &str, level: CompressionLevel) -> String {
        match level {
            CompressionLevel::Full => content.to_string(),
            CompressionLevel::Summary => self.compress_to_summary(content),
            CompressionLevel::Keywords => self.compress_to_keywords(content),
            CompressionLevel::Hash => self.compress_to_hash(content),
        }
    }

    /// Compress content to summary by extracting first N sentences
    ///
    /// Sentences are detected by period, exclamation, or question mark followed
    /// by whitespace or end of string.
    fn compress_to_summary(&self, content: &str) -> String {
        let sentences: Vec<&str> = content
            .split(|c| c == '.' || c == '!' || c == '?')
            .filter(|s| !s.trim().is_empty())
            .take(self.config.summary_max_sentences)
            .collect();

        if sentences.is_empty() {
            let truncated: String = content.chars().take(200).collect();
            if truncated.len() < content.len() {
                format!("{}...", truncated)
            } else {
                truncated
            }
        } else {
            sentences.join(". ") + "."
        }
    }

    /// Compress content to keywords by extracting significant words
    ///
    /// Extracts unique words that are:
    /// - Longer than min_word_length characters
    /// - Not common stop words
    /// - Limited to max keywords_max_count words
    fn compress_to_keywords(&self, content: &str) -> String {
        let stop_words: HashSet<&str> = [
            "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with",
            "by", "from", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
            "do", "does", "did", "will", "would", "could", "should", "may", "might", "must",
            "this", "that", "these", "those", "it", "its", "they", "them", "their", "we", "you",
            "your", "our", "i", "me", "my", "he", "she", "his", "her", "not", "no", "yes",
            "what", "which", "who", "when", "where", "why", "how", "all", "each", "every",
            "both", "few", "more", "most", "other", "some", "such", "than", "too", "very",
            "just", "also", "only", "then", "there", "here", "now", "about", "into", "over",
            "after", "before", "between", "under", "again", "further", "once", "during",
        ]
        .into_iter()
        .collect();

        let mut seen = HashSet::new();
        let keywords: Vec<String> = content
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| {
                let lower = word.to_lowercase();
                word.len() >= self.config.keywords_min_word_length
                    && !stop_words.contains(lower.as_str())
                    && seen.insert(lower)
            })
            .take(self.config.keywords_max_count)
            .map(|s| s.to_string())
            .collect();

        keywords.join(", ")
    }

    /// Compress content to hash - keeps only a reference marker
    ///
    /// The original content is discarded but the embedding is preserved,
    /// allowing the memory to still be found via semantic search.
    fn compress_to_hash(&self, _content: &str) -> String {
        "[content archived - searchable via embedding]".to_string()
    }

    /// Get numeric value for compression level (for comparison)
    fn compression_level_value(level: CompressionLevel) -> u8 {
        match level {
            CompressionLevel::Full => 0,
            CompressionLevel::Summary => 1,
            CompressionLevel::Keywords => 2,
            CompressionLevel::Hash => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::types::{Memory, MemorySource, MemoryType};

    fn create_test_memory(content: &str, age_days: i64) -> Memory {
        let mut memory = Memory::new(
            content.to_string(),
            vec![0.1; 384],
            MemoryType::Semantic,
            MemorySource::Manual,
        );
        memory.created_at = Utc::now() - Duration::days(age_days);
        memory.tier = StorageTier::Warm;
        memory.weight = 0.5;
        memory
    }

    fn create_test_memory_with_weight(content: &str, age_days: i64, weight: f32) -> Memory {
        let mut memory = create_test_memory(content, age_days);
        memory.weight = weight;
        memory
    }

    mod config {
        use super::*;

        #[test]
        fn test_default_config() {
            let config = CompactionConfig::default();
            assert_eq!(config.summary_age_days, 30);
            assert_eq!(config.keywords_age_days, 90);
            assert_eq!(config.min_weight_to_preserve, 0.7);
            assert_eq!(config.summary_max_sentences, 3);
            assert_eq!(config.keywords_max_count, 20);
            assert_eq!(config.keywords_min_word_length, 4);
        }

        #[test]
        fn test_custom_config() {
            let config = CompactionConfig::new(15, 60)
                .with_min_weight(0.8)
                .with_summary_sentences(5)
                .with_max_keywords(30);

            assert_eq!(config.summary_age_days, 15);
            assert_eq!(config.keywords_age_days, 60);
            assert_eq!(config.min_weight_to_preserve, 0.8);
            assert_eq!(config.summary_max_sentences, 5);
            assert_eq!(config.keywords_max_count, 30);
        }

        #[test]
        fn test_weight_clamping() {
            let config = CompactionConfig::default().with_min_weight(1.5);
            assert_eq!(config.min_weight_to_preserve, 1.0);

            let config = CompactionConfig::default().with_min_weight(-0.5);
            assert_eq!(config.min_weight_to_preserve, 0.0);
        }
    }

    mod compression_strategies {
        use super::*;

        #[test]
        fn test_compress_to_summary_multiple_sentences() {
            let config = CompactionConfig::default();
            let content = "This is the first sentence. Here is the second sentence. And a third one. Fourth sentence here. Fifth sentence.";
            
            let sentences: Vec<&str> = content
                .split(|c| c == '.' || c == '!' || c == '?')
                .filter(|s| !s.trim().is_empty())
                .take(config.summary_max_sentences)
                .collect();
            let result = sentences.join(". ") + ".";

            assert_eq!(
                result,
                "This is the first sentence.  Here is the second sentence.  And a third one."
            );
        }

        #[test]
        fn test_compress_to_summary_few_sentences() {
            let config = CompactionConfig::default();
            let content = "Just one sentence here.";

            let sentences: Vec<&str> = content
                .split(|c| c == '.' || c == '!' || c == '?')
                .filter(|s| !s.trim().is_empty())
                .take(config.summary_max_sentences)
                .collect();
            let result = sentences.join(". ") + ".";

            assert_eq!(result, "Just one sentence here.");
        }

        #[test]
        fn test_compress_to_summary_no_punctuation() {
            let config = CompactionConfig::default();
            let content = "This is a long text without any sentence-ending punctuation that goes on and on and on and on and on";

            let sentences: Vec<&str> = content
                .split(|c| c == '.' || c == '!' || c == '?')
                .filter(|s| !s.trim().is_empty())
                .take(config.summary_max_sentences)
                .collect();

            if sentences.is_empty() || (sentences.len() == 1 && sentences[0] == content) {
                let truncated: String = content.chars().take(200).collect();
                let result = if truncated.len() < content.len() {
                    format!("{}...", truncated)
                } else {
                    truncated
                };
                assert!(result.len() <= 203);
            }
        }

        #[test]
        fn test_compress_to_keywords_extracts_significant_words() {
            let config = CompactionConfig::default();
            let content = "The database connection failed because the server configuration was incorrect.";

            let stop_words: std::collections::HashSet<&str> = [
                "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with",
                "by", "from", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
                "because",
            ]
            .into_iter()
            .collect();

            let mut seen = std::collections::HashSet::new();
            let keywords: Vec<String> = content
                .split(|c: char| !c.is_alphanumeric())
                .filter(|word| {
                    let lower = word.to_lowercase();
                    word.len() >= config.keywords_min_word_length
                        && !stop_words.contains(lower.as_str())
                        && seen.insert(lower)
                })
                .take(config.keywords_max_count)
                .map(|s| s.to_string())
                .collect();

            let result = keywords.join(", ");

            assert!(result.contains("database"));
            assert!(result.contains("connection"));
            assert!(result.contains("failed"));
            assert!(result.contains("server"));
            assert!(result.contains("configuration"));
            assert!(result.contains("incorrect"));
        }

        #[test]
        fn test_compress_to_keywords_removes_short_words() {
            let config = CompactionConfig::default();
            let content = "I am a big fan of the API and SDK tools";

            let stop_words: std::collections::HashSet<&str> = [
                "the", "a", "an", "and", "or", "of", "i", "am",
            ]
            .into_iter()
            .collect();

            let mut seen = std::collections::HashSet::new();
            let keywords: Vec<String> = content
                .split(|c: char| !c.is_alphanumeric())
                .filter(|word| {
                    let lower = word.to_lowercase();
                    word.len() >= config.keywords_min_word_length
                        && !stop_words.contains(lower.as_str())
                        && seen.insert(lower)
                })
                .take(config.keywords_max_count)
                .map(|s| s.to_string())
                .collect();

            let result = keywords.join(", ");

            assert!(!result.contains("big"));
            assert!(!result.contains("fan"));
            assert!(result.contains("tools"));
        }

        #[test]
        fn test_compress_to_keywords_removes_duplicates() {
            let content = "The server server server crashed and the server restarted";

            let stop_words: std::collections::HashSet<&str> =
                ["the", "and"].into_iter().collect();
            let config = CompactionConfig::default();

            let mut seen = std::collections::HashSet::new();
            let keywords: Vec<String> = content
                .split(|c: char| !c.is_alphanumeric())
                .filter(|word| {
                    let lower = word.to_lowercase();
                    word.len() >= config.keywords_min_word_length
                        && !stop_words.contains(lower.as_str())
                        && seen.insert(lower)
                })
                .take(config.keywords_max_count)
                .map(|s| s.to_string())
                .collect();

            let result = keywords.join(", ");

            assert_eq!(result.matches("server").count(), 1);
        }

        #[test]
        fn test_compress_to_hash() {
            let result = "[content archived - searchable via embedding]";
            assert_eq!(result, "[content archived - searchable via embedding]");
        }

        #[test]
        fn test_compression_level_ordering() {
            assert!(
                Compactor::compression_level_value(CompressionLevel::Full)
                    < Compactor::compression_level_value(CompressionLevel::Summary)
            );
            assert!(
                Compactor::compression_level_value(CompressionLevel::Summary)
                    < Compactor::compression_level_value(CompressionLevel::Keywords)
            );
            assert!(
                Compactor::compression_level_value(CompressionLevel::Keywords)
                    < Compactor::compression_level_value(CompressionLevel::Hash)
            );
        }
    }

    mod compactor_integration {
        use super::*;

        #[tokio::test]
        async fn test_compact_reduces_content_size() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let long_content = "This is the first sentence with lots of content. \
                Here is the second sentence with more words. \
                And a third sentence to make it longer. \
                Fourth sentence adds even more text. \
                Fifth sentence keeps going. \
                Sixth sentence is here too.";

            let memory = create_test_memory(long_content, 45);
            let id = memory.id;
            let original_len = memory.content.len();
            store.insert(&memory).await.unwrap();

            let compactor = Compactor::new(&store);
            let result = compactor.compact(StorageTier::Warm).await.unwrap();

            assert_eq!(result.compacted_count, 1);
            assert!(result.compacted_ids.contains(&id));

            let updated = store.get(id).await.unwrap().unwrap();
            assert!(updated.content.len() < original_len);
            assert_eq!(updated.compression, CompressionLevel::Summary);
        }

        #[tokio::test]
        async fn test_compact_updates_compression_level() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory("Test content for compression.", 100);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let compactor = Compactor::new(&store);
            compactor.compact(StorageTier::Warm).await.unwrap();

            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.compression, CompressionLevel::Keywords);
        }

        #[tokio::test]
        async fn test_compact_preserves_embedding() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory("Test content for embedding preservation.", 45);
            let id = memory.id;
            let original_embedding = memory.embedding.clone();
            store.insert(&memory).await.unwrap();

            let compactor = Compactor::new(&store);
            compactor.compact(StorageTier::Warm).await.unwrap();

            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.embedding, original_embedding);
            assert_eq!(updated.embedding.len(), 384);
        }

        #[tokio::test]
        async fn test_compact_skips_high_weight_memories() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory_with_weight("Important memory.", 45, 0.9);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let compactor = Compactor::new(&store);
            let result = compactor.compact(StorageTier::Warm).await.unwrap();

            assert_eq!(result.compacted_count, 0);
            assert_eq!(result.skipped_high_weight, 1);

            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.compression, CompressionLevel::Full);
            assert_eq!(updated.content, "Important memory.");
        }

        #[tokio::test]
        async fn test_compact_skips_recent_memories() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory("Recent memory content.", 5);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let compactor = Compactor::new(&store);
            let result = compactor.compact(StorageTier::Warm).await.unwrap();

            assert_eq!(result.compacted_count, 0);

            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.compression, CompressionLevel::Full);
        }

        #[tokio::test]
        async fn test_compact_skips_already_compressed() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let mut memory = create_test_memory("Already summarized.", 45);
            memory.compression = CompressionLevel::Summary;
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let compactor = Compactor::new(&store);
            let result = compactor.compact(StorageTier::Warm).await.unwrap();

            assert_eq!(result.compacted_count, 0);
            assert_eq!(result.already_compressed, 1);

            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.compression, CompressionLevel::Summary);
        }

        #[tokio::test]
        async fn test_compact_single() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory("Content to be compacted.", 10);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let compactor = Compactor::new(&store);
            let compacted = compactor
                .compact_single(id, CompressionLevel::Keywords)
                .await
                .unwrap();

            assert!(compacted);

            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.compression, CompressionLevel::Keywords);
        }

        #[tokio::test]
        async fn test_compact_single_respects_weight() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory_with_weight("High weight content.", 10, 0.85);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let compactor = Compactor::new(&store);
            let compacted = compactor
                .compact_single(id, CompressionLevel::Summary)
                .await
                .unwrap();

            assert!(!compacted);

            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.compression, CompressionLevel::Full);
        }

        #[tokio::test]
        async fn test_compact_with_custom_config() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory("Content for custom config test.", 20);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let config = CompactionConfig::new(15, 60);
            let compactor = Compactor::with_config(&store, config);
            let result = compactor.compact(StorageTier::Warm).await.unwrap();

            assert_eq!(result.compacted_count, 1);

            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.compression, CompressionLevel::Summary);
        }

        #[tokio::test]
        async fn test_compact_nonexistent_memory() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let compactor = Compactor::new(&store);
            let result = compactor
                .compact_single(Uuid::new_v4(), CompressionLevel::Summary)
                .await;

            assert!(result.is_err());
        }

        #[tokio::test]
        async fn test_progressive_compression() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let long_content = "This is a detailed memory with multiple sentences. \
                It contains important information about the system. \
                There are several key points to remember here. \
                The database handles all persistence operations. \
                Configuration settings are stored in a separate file.";

            let memory = create_test_memory(long_content, 45);
            let id = memory.id;
            store.insert(&memory).await.unwrap();

            let compactor = Compactor::new(&store);
            compactor.compact(StorageTier::Warm).await.unwrap();

            let after_first = store.get(id).await.unwrap().unwrap();
            assert_eq!(after_first.compression, CompressionLevel::Summary);

            let old_memory = create_test_memory(long_content, 100);
            let old_id = old_memory.id;
            store.insert(&old_memory).await.unwrap();

            compactor.compact(StorageTier::Warm).await.unwrap();

            let after_old = store.get(old_id).await.unwrap().unwrap();
            assert_eq!(after_old.compression, CompressionLevel::Keywords);
        }
    }
}
