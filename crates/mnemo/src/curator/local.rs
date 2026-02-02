//! Local curator provider using mistral.rs for on-device inference
//!
//! Implements the CuratorProvider trait using locally-loaded models
//! via the mistral.rs library for CPU or GPU inference.

use async_trait::async_trait;
use mistralrs::{IsqType, Model, TextMessageRole, TextMessages, TextModelBuilder};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::config::LocalCuratorConfig;
use crate::curator::prompts::{CLASSIFICATION_PROMPT, EXTRACTION_PROMPT};
use crate::curator::types::{CuratedMemory, CurationResult, CuratorError};
use crate::curator::CuratorProvider;
use crate::memory::types::MemoryType;

pub struct LocalCurator {
    model: Arc<Model>,
    #[allow(dead_code)]
    config: LocalCuratorConfig,
}

impl std::fmt::Debug for LocalCurator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalCurator")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Deserialize)]
struct ExtractedMemoryJson {
    #[serde(rename = "type")]
    memory_type: String,
    content: String,
    importance: f32,
    entities: Vec<String>,
}

impl LocalCurator {
    pub async fn new(config: &LocalCuratorConfig) -> Result<Self, CuratorError> {
        let models_dir = get_models_dir()?;
        let model_cache_name = config.model_id.replace('/', "--");
        let model_path = models_dir.join(format!("models--{model_cache_name}"));

        if !model_path.exists() {
            return Err(CuratorError::ModelNotFound(format!(
                "Model '{}' not found at {:?}. Download it first with 'mnemo-cli model download {}'",
                config.model_id, model_path, config.model_id
            )));
        }

        info!(
            "Loading local curator model: {} (quantization: {})",
            config.model_id, config.quantization
        );

        let isq_type = parse_quantization(&config.quantization)?;

        let builder = TextModelBuilder::new(&config.model_id).with_isq(isq_type);

        if config.use_gpu {
            debug!("GPU acceleration enabled for local curator");
        } else {
            debug!("Running local curator on CPU");
        }

        let model = builder
            .build()
            .await
            .map_err(|e| CuratorError::ModelLoadFailed(e.to_string()))?;

        info!(
            "Local curator model loaded: {} with {} quantization",
            config.model_id, config.quantization
        );

        Ok(Self {
            model: Arc::new(model),
            config: config.clone(),
        })
    }

    async fn classify(&self, conversation: &str) -> Result<bool, CuratorError> {
        let prompt = CLASSIFICATION_PROMPT.replace("{conversation}", conversation);

        let messages = TextMessages::new()
            .add_message(TextMessageRole::System, "You are a memory curator assistant.")
            .add_message(TextMessageRole::User, &prompt);

        let response = self
            .model
            .send_chat_request(messages)
            .await
            .map_err(|e| CuratorError::InferenceFailed(e.to_string()))?;

        let content = response
            .choices
            .first()
            .and_then(|c| c.message.content.as_ref())
            .ok_or_else(|| CuratorError::InferenceFailed("Empty response from model".to_string()))?;

        debug!("Classification response: {}", content);
        Ok(content.trim().to_uppercase().contains("YES"))
    }

    async fn extract(&self, conversation: &str) -> Result<Vec<CuratedMemory>, CuratorError> {
        let prompt = EXTRACTION_PROMPT.replace("{conversation}", conversation);

        let messages = TextMessages::new()
            .add_message(
                TextMessageRole::System,
                "You are a memory extraction assistant. Always respond with valid JSON.",
            )
            .add_message(TextMessageRole::User, &prompt);

        let response = self
            .model
            .send_chat_request(messages)
            .await
            .map_err(|e| CuratorError::InferenceFailed(e.to_string()))?;

        let content = response
            .choices
            .first()
            .and_then(|c| c.message.content.as_ref())
            .ok_or_else(|| CuratorError::InferenceFailed("Empty response from model".to_string()))?;

        debug!("Extraction response: {}", content);

        let json_str = extract_json_from_response(content);

        let extracted: Vec<ExtractedMemoryJson> = serde_json::from_str(json_str).map_err(|e| {
            warn!("Failed to parse extraction JSON: {}, raw: {}", e, content);
            CuratorError::ParseError(format!("Failed to parse extraction JSON: {e}"))
        })?;

        let memories = extracted
            .into_iter()
            .map(|m| {
                let memory_type = match m.memory_type.to_lowercase().as_str() {
                    "episodic" => MemoryType::Episodic,
                    "procedural" => MemoryType::Procedural,
                    _ => MemoryType::Semantic,
                };

                CuratedMemory::new(memory_type, m.content, m.importance, m.entities)
            })
            .collect();

        Ok(memories)
    }
}

#[async_trait]
impl CuratorProvider for LocalCurator {
    async fn curate(&self, conversation: &str) -> Result<CurationResult, CuratorError> {
        let should_store = self.classify(conversation).await?;
        if !should_store {
            debug!("Conversation classified as not memory-worthy");
            return Ok(CurationResult::should_not_store(
                "Not memory-worthy".to_string(),
            ));
        }

        let memories = self.extract(conversation).await?;
        info!("Extracted {} memories from conversation", memories.len());
        Ok(CurationResult::should_store(
            memories,
            "Extracted via local model".to_string(),
        ))
    }

    async fn is_available(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "local"
    }
}

fn get_models_dir() -> Result<PathBuf, CuratorError> {
    dirs::home_dir()
        .map(|h| h.join(".mnemo").join("models"))
        .ok_or_else(|| CuratorError::ConfigError("Could not determine home directory".to_string()))
}

fn parse_quantization(quant_str: &str) -> Result<IsqType, CuratorError> {
    match quant_str.to_uppercase().as_str() {
        "Q2K" => Ok(IsqType::Q2K),
        "Q3K" => Ok(IsqType::Q3K),
        "Q4K" => Ok(IsqType::Q4K),
        "Q5K" => Ok(IsqType::Q5K),
        "Q6K" => Ok(IsqType::Q6K),
        "Q8K" => Ok(IsqType::Q8K),
        "Q4_0" => Ok(IsqType::Q4_0),
        "Q4_1" => Ok(IsqType::Q4_1),
        "Q5_0" => Ok(IsqType::Q5_0),
        "Q5_1" => Ok(IsqType::Q5_1),
        "Q8_0" => Ok(IsqType::Q8_0),
        "Q8_1" => Ok(IsqType::Q8_1),
        "HQQ4" => Ok(IsqType::HQQ4),
        "HQQ8" => Ok(IsqType::HQQ8),
        "F8E4M3" => Ok(IsqType::F8E4M3),
        _ => Err(CuratorError::ConfigError(format!(
            "Unknown quantization type: {quant_str}. Valid options: Q2K, Q3K, Q4K, Q5K, Q6K, Q8K, Q4_0, Q4_1, Q5_0, Q5_1, Q8_0, Q8_1, HQQ4, HQQ8, F8E4M3"
        ))),
    }
}

fn extract_json_from_response(content: &str) -> &str {
    let trimmed = content.trim();

    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            return &trimmed[start..=end];
        }
    }

    if trimmed.starts_with("```json") {
        if let Some(end) = trimmed.rfind("```") {
            let json_start = trimmed.find('\n').map(|i| i + 1).unwrap_or(7);
            if json_start < end {
                return trimmed[json_start..end].trim();
            }
        }
    }

    if trimmed.starts_with("```") {
        if let Some(end) = trimmed.rfind("```") {
            let json_start = trimmed.find('\n').map(|i| i + 1).unwrap_or(3);
            if json_start < end {
                return trimmed[json_start..end].trim();
            }
        }
    }

    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_quantization_valid() {
        assert!(matches!(parse_quantization("Q4K"), Ok(IsqType::Q4K)));
        assert!(matches!(parse_quantization("q4k"), Ok(IsqType::Q4K)));
        assert!(matches!(parse_quantization("Q8_0"), Ok(IsqType::Q8_0)));
        assert!(matches!(parse_quantization("HQQ4"), Ok(IsqType::HQQ4)));
    }

    #[test]
    fn test_parse_quantization_invalid() {
        assert!(parse_quantization("INVALID").is_err());
        assert!(parse_quantization("Q99").is_err());
    }

    #[test]
    fn test_extract_json_from_response_clean() {
        let input = r#"[{"type": "semantic", "content": "test", "importance": 0.5, "entities": []}]"#;
        let result = extract_json_from_response(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_extract_json_from_response_with_prefix() {
        let input = r#"Here is the JSON:
[{"type": "semantic", "content": "test", "importance": 0.5, "entities": []}]"#;
        let result = extract_json_from_response(input);
        assert!(result.starts_with('['));
        assert!(result.ends_with(']'));
    }

    #[test]
    fn test_extract_json_from_response_code_block() {
        let input = r#"```json
[{"type": "semantic", "content": "test", "importance": 0.5, "entities": []}]
```"#;
        let result = extract_json_from_response(input);
        assert!(result.starts_with('['));
        assert!(result.ends_with(']'));
    }

    #[test]
    fn test_get_models_dir() {
        let result = get_models_dir();
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.to_string_lossy().contains(".mnemo"));
        assert!(path.to_string_lossy().contains("models"));
    }

    #[test]
    #[ignore]
    fn test_local_curator_model_not_found() {
        let config = LocalCuratorConfig {
            model_id: "nonexistent/model".to_string(),
            quantization: "Q4K".to_string(),
            use_gpu: false,
            context_length: 4096,
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(LocalCurator::new(&config));

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }
}
