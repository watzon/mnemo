use serde::Deserialize;
use std::path::PathBuf;

/// Main configuration structure for Mnemo
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    /// Storage configuration (hot/warm/cold tiers)
    #[serde(default)]
    pub storage: StorageConfig,
    /// HTTP proxy configuration
    #[serde(default)]
    pub proxy: ProxyConfig,
    /// Request routing configuration
    #[serde(default)]
    pub router: RouterConfig,
    /// Embedding model configuration
    #[serde(default)]
    pub embedding: EmbeddingConfig,
}

/// Storage tier configuration
#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    /// Hot cache size in GB (in-memory, fastest access)
    #[serde(default = "default_hot_cache_gb")]
    pub hot_cache_gb: u64,
    /// Warm storage size in GB (local disk, moderate access)
    #[serde(default = "default_warm_storage_gb")]
    pub warm_storage_gb: u64,
    /// Enable cold storage tier (cloud/offsite backup)
    #[serde(default = "default_cold_enabled")]
    pub cold_enabled: bool,
    /// Base directory for all storage data
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            hot_cache_gb: default_hot_cache_gb(),
            warm_storage_gb: default_warm_storage_gb(),
            cold_enabled: default_cold_enabled(),
            data_dir: default_data_dir(),
        }
    }
}

fn default_hot_cache_gb() -> u64 {
    10
}

fn default_warm_storage_gb() -> u64 {
    50
}

fn default_cold_enabled() -> bool {
    true
}

fn default_data_dir() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".mnemo"))
        .unwrap_or_else(|| PathBuf::from(".mnemo"))
}

/// HTTP proxy server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ProxyConfig {
    /// Address to listen on (e.g., "127.0.0.1:9999")
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
    /// Upstream LLM API URL (optional - can be specified per-request)
    #[serde(default)]
    pub upstream_url: Option<String>,
    /// Allowed upstream hosts (empty = allow all)
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    /// Request timeout in seconds
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// Maximum tokens to inject into context
    #[serde(default = "default_max_injection_tokens")]
    pub max_injection_tokens: usize,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            listen_addr: default_listen_addr(),
            upstream_url: None,
            allowed_hosts: Vec::new(),
            timeout_secs: default_timeout_secs(),
            max_injection_tokens: default_max_injection_tokens(),
        }
    }
}

fn default_listen_addr() -> String {
    "127.0.0.1:9999".to_string()
}

fn default_timeout_secs() -> u64 {
    300
}

fn default_max_injection_tokens() -> usize {
    2000
}

/// Request routing and strategy configuration
#[derive(Debug, Clone, Deserialize)]
pub struct RouterConfig {
    /// Strategy for selecting relevant memories (placeholder)
    #[serde(default)]
    pub strategy: String,
    /// Maximum memories to retrieve per request (placeholder)
    #[serde(default = "default_max_memories")]
    pub max_memories: usize,
    /// Minimum relevance score threshold (placeholder)
    #[serde(default = "default_relevance_threshold")]
    pub relevance_threshold: f32,
    /// Deterministic retrieval settings for improved LLM cache hit rates
    #[serde(default)]
    pub deterministic: DeterministicConfig,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            strategy: String::new(),
            max_memories: default_max_memories(),
            relevance_threshold: default_relevance_threshold(),
            deterministic: DeterministicConfig::default(),
        }
    }
}

fn default_max_memories() -> usize {
    10
}

fn default_relevance_threshold() -> f32 {
    0.7
}

/// Deterministic retrieval configuration for improved LLM cache hit rates
#[derive(Debug, Clone, Deserialize)]
pub struct DeterministicConfig {
    /// Enable deterministic memory retrieval ordering
    #[serde(default = "default_deterministic_enabled")]
    pub enabled: bool,
    /// Number of decimal places for score quantization (1-4)
    #[serde(default = "default_decimal_places")]
    pub decimal_places: u8,
    /// Weight for topic/entity overlap scoring (0.0-1.0)
    #[serde(default = "default_topic_overlap_weight")]
    pub topic_overlap_weight: f32,
}

impl Default for DeterministicConfig {
    fn default() -> Self {
        Self {
            enabled: default_deterministic_enabled(),
            decimal_places: default_decimal_places(),
            topic_overlap_weight: default_topic_overlap_weight(),
        }
    }
}

fn default_deterministic_enabled() -> bool {
    false
}

fn default_decimal_places() -> u8 {
    2
}

fn default_topic_overlap_weight() -> f32 {
    0.1
}

/// Embedding model configuration
#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddingConfig {
    /// Embedding model provider (placeholder)
    #[serde(default)]
    pub provider: String,
    /// Model name or identifier (placeholder)
    #[serde(default)]
    pub model: String,
    /// Embedding dimension size (placeholder)
    #[serde(default = "default_embedding_dimension")]
    pub dimension: usize,
    /// Batch size for embedding generation (placeholder)
    #[serde(default = "default_embedding_batch_size")]
    pub batch_size: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: String::new(),
            model: String::new(),
            dimension: default_embedding_dimension(),
            batch_size: default_embedding_batch_size(),
        }
    }
}

fn default_embedding_dimension() -> usize {
    1536
}

fn default_embedding_batch_size() -> usize {
    32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.storage.hot_cache_gb, 10);
        assert_eq!(config.storage.warm_storage_gb, 50);
        assert!(config.storage.cold_enabled);
        assert_eq!(config.proxy.listen_addr, "127.0.0.1:9999");
        assert!(config.proxy.upstream_url.is_none());
        assert!(config.proxy.allowed_hosts.is_empty());
        assert_eq!(config.proxy.timeout_secs, 300);
        assert_eq!(config.proxy.max_injection_tokens, 2000);
        assert_eq!(config.router.max_memories, 10);
        assert_eq!(config.router.relevance_threshold, 0.7);
        assert_eq!(config.embedding.dimension, 1536);
        assert_eq!(config.embedding.batch_size, 32);
    }

    #[test]
    fn test_toml_deserialization() {
        let toml_str = r#"
[storage]
hot_cache_gb = 20
warm_storage_gb = 100
cold_enabled = false
data_dir = "/tmp/mnemo"

[proxy]
listen_addr = "0.0.0.0:8080"
upstream_url = "https://api.openai.com/v1"
timeout_secs = 60
max_injection_tokens = 4000

[router]
strategy = "semantic"
max_memories = 20
relevance_threshold = 0.8

[embedding]
provider = "openai"
model = "text-embedding-3-small"
dimension = 1536
batch_size = 64
"#;

        let config: Config = toml::from_str(toml_str).expect("Failed to parse TOML");

        assert_eq!(config.storage.hot_cache_gb, 20);
        assert_eq!(config.storage.warm_storage_gb, 100);
        assert!(!config.storage.cold_enabled);
        assert_eq!(config.storage.data_dir, PathBuf::from("/tmp/mnemo"));

        assert_eq!(config.proxy.listen_addr, "0.0.0.0:8080");
        assert_eq!(
            config.proxy.upstream_url,
            Some("https://api.openai.com/v1".to_string())
        );
        assert!(config.proxy.allowed_hosts.is_empty());
        assert_eq!(config.proxy.timeout_secs, 60);
        assert_eq!(config.proxy.max_injection_tokens, 4000);

        assert_eq!(config.router.strategy, "semantic");
        assert_eq!(config.router.max_memories, 20);
        assert_eq!(config.router.relevance_threshold, 0.8);

        assert_eq!(config.embedding.provider, "openai");
        assert_eq!(config.embedding.model, "text-embedding-3-small");
        assert_eq!(config.embedding.dimension, 1536);
        assert_eq!(config.embedding.batch_size, 64);
    }

    #[test]
    fn test_toml_partial_deserialization() {
        // Test that we can deserialize with only required fields
        let toml_str = r#"
[proxy]
upstream_url = "https://api.example.com"
"#;

        let config: Config = toml::from_str(toml_str).expect("Failed to parse partial TOML");

        // Check defaults are applied
        assert_eq!(config.storage.hot_cache_gb, 10);
        assert_eq!(config.proxy.listen_addr, "127.0.0.1:9999");
        assert_eq!(
            config.proxy.upstream_url,
            Some("https://api.example.com".to_string())
        );
        assert!(config.proxy.allowed_hosts.is_empty());
    }

    #[test]
    fn test_upstream_url_none_when_not_provided() {
        // Test that upstream_url is None when not provided in TOML
        let toml_str = r#"
[proxy]
listen_addr = "127.0.0.1:9999"
"#;

        let config: Config = toml::from_str(toml_str).expect("Failed to parse TOML");
        assert!(config.proxy.upstream_url.is_none());
    }

    #[test]
    fn test_allowed_hosts_defaults_to_empty() {
        // Test that allowed_hosts defaults to empty Vec when not provided
        let toml_str = r#"
[proxy]
upstream_url = "https://api.openai.com/v1"
"#;

        let config: Config = toml::from_str(toml_str).expect("Failed to parse TOML");
        assert!(config.proxy.allowed_hosts.is_empty());
    }

    #[test]
    fn test_allowed_hosts_parses_from_toml() {
        // Test that allowed_hosts parses correctly from TOML array
        let toml_str = r#"
[proxy]
upstream_url = "https://api.openai.com/v1"
allowed_hosts = ["api.openai.com", "api.anthropic.com"]
"#;

        let config: Config = toml::from_str(toml_str).expect("Failed to parse TOML");
        assert_eq!(config.proxy.allowed_hosts.len(), 2);
        assert_eq!(config.proxy.allowed_hosts[0], "api.openai.com");
        assert_eq!(config.proxy.allowed_hosts[1], "api.anthropic.com");
    }

    #[test]
    fn test_deterministic_config_defaults() {
        let config = Config::default();
        assert!(!config.router.deterministic.enabled);
        assert_eq!(config.router.deterministic.decimal_places, 2);
        assert!((config.router.deterministic.topic_overlap_weight - 0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn test_deterministic_config_from_toml() {
        let toml_str = r#"
[router]
strategy = "semantic"

[router.deterministic]
enabled = true
decimal_places = 3
topic_overlap_weight = 0.2
"#;

        let config: Config = toml::from_str(toml_str).expect("Failed to parse TOML");
        assert!(config.router.deterministic.enabled);
        assert_eq!(config.router.deterministic.decimal_places, 3);
        assert!((config.router.deterministic.topic_overlap_weight - 0.2).abs() < f32::EPSILON);
    }
}
