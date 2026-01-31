use crate::NovaError;
use candle_core::{DType, Device, Tensor};
use candle_nn::{Linear, Module, VarBuilder};
use candle_transformers::models::bert::{BertModel, Config as BertConfig, DTYPE};
use hf_hub::{api::sync::Api, Repo, RepoType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokenizers::Tokenizer;

const MODEL_ID: &str = "dslim/bert-base-NER";
const MODEL_REVISION: &str = "main";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityLabel {
    Person,
    Organization,
    Location,
    Misc,
}

impl EntityLabel {
    fn from_tag(tag: &str) -> Option<Self> {
        let normalized = tag.to_uppercase();
        if normalized.contains("PER") {
            Some(EntityLabel::Person)
        } else if normalized.contains("ORG") {
            Some(EntityLabel::Organization)
        } else if normalized.contains("LOC") {
            Some(EntityLabel::Location)
        } else if normalized.contains("MISC") {
            Some(EntityLabel::Misc)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub text: String,
    pub label: EntityLabel,
    pub confidence: f32,
}

pub struct NerModel {
    model: BertModel,
    classifier: Linear,
    tokenizer: Tokenizer,
    device: Device,
    id2label: HashMap<u32, String>,
}

#[derive(Deserialize)]
struct ModelConfig {
    #[serde(flatten)]
    bert_config: BertConfig,
    id2label: Option<HashMap<String, String>>,
}

impl NerModel {
    pub fn new() -> Result<Self, NovaError> {
        Self::with_cache_dir(None)
    }

    pub fn with_cache_dir(_cache_dir: Option<PathBuf>) -> Result<Self, NovaError> {
        let device = Device::Cpu;

        let api =
            Api::new().map_err(|e| NovaError::Router(format!("Failed to create HF API: {}", e)))?;

        let repo = api.repo(Repo::with_revision(
            MODEL_ID.to_string(),
            RepoType::Model,
            MODEL_REVISION.to_string(),
        ));

        let config_path = repo
            .get("config.json")
            .map_err(|e| NovaError::Router(format!("Failed to download config: {}", e)))?;

        let tokenizer_path = repo
            .get("onnx/tokenizer.json")
            .map_err(|e| NovaError::Router(format!("Failed to download tokenizer: {}", e)))?;

        let weights_path = repo
            .get("model.safetensors")
            .map_err(|e| NovaError::Router(format!("Failed to download weights: {}", e)))?;

        let config_str = std::fs::read_to_string(&config_path)
            .map_err(|e| NovaError::Router(format!("Failed to read config: {}", e)))?;

        let model_config: ModelConfig = serde_json::from_str(&config_str)
            .map_err(|e| NovaError::Router(format!("Failed to parse config: {}", e)))?;

        let id2label: HashMap<u32, String> = model_config
            .id2label
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(k, v)| k.parse::<u32>().ok().map(|id| (id, v)))
            .collect();

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| NovaError::Router(format!("Failed to load tokenizer: {}", e)))?;

        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights_path], DTYPE, &device)
                .map_err(|e| NovaError::Router(format!("Failed to load weights: {}", e)))?
        };

        let model = BertModel::load(vb.pp("bert"), &model_config.bert_config)
            .map_err(|e| NovaError::Router(format!("Failed to load BERT model: {}", e)))?;

        let num_labels = if id2label.is_empty() {
            9
        } else {
            id2label.len()
        };

        let classifier = candle_nn::linear(
            model_config.bert_config.hidden_size,
            num_labels,
            vb.pp("classifier"),
        )
        .map_err(|e| NovaError::Router(format!("Failed to load classifier: {}", e)))?;

        Ok(Self {
            model,
            classifier,
            tokenizer,
            device,
            id2label,
        })
    }

    pub fn extract_entities(&self, text: &str) -> Result<Vec<Entity>, NovaError> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| NovaError::Router(format!("Tokenization failed: {}", e)))?;

        let token_ids = encoding.get_ids();
        let tokens = encoding.get_tokens();

        if token_ids.is_empty() {
            return Ok(Vec::new());
        }

        let token_ids_tensor = Tensor::new(token_ids, &self.device)
            .map_err(|e| NovaError::Router(format!("Failed to create tensor: {}", e)))?
            .unsqueeze(0)
            .map_err(|e| NovaError::Router(format!("Failed to unsqueeze: {}", e)))?;

        let token_type_ids = token_ids_tensor
            .zeros_like()
            .map_err(|e| NovaError::Router(format!("Failed to create token_type_ids: {}", e)))?;

        let hidden_states = self
            .model
            .forward(&token_ids_tensor, &token_type_ids, None)
            .map_err(|e| NovaError::Router(format!("Model forward pass failed: {}", e)))?;

        let logits = self
            .classifier
            .forward(&hidden_states)
            .map_err(|e| NovaError::Router(format!("Classifier forward failed: {}", e)))?;

        let logits = logits
            .squeeze(0)
            .map_err(|e| NovaError::Router(format!("Failed to squeeze logits: {}", e)))?;

        let probabilities = candle_nn::ops::softmax(&logits, 1)
            .map_err(|e| NovaError::Router(format!("Softmax failed: {}", e)))?;

        let predictions = logits
            .argmax(1)
            .map_err(|e| NovaError::Router(format!("Argmax failed: {}", e)))?
            .to_vec1::<u32>()
            .map_err(|e| NovaError::Router(format!("Failed to convert predictions: {}", e)))?;

        let probs_2d = probabilities
            .to_dtype(DType::F32)
            .map_err(|e| NovaError::Router(format!("Failed to convert dtype: {}", e)))?;

        let entities = self.extract_bio_entities(tokens, &predictions, &probs_2d)?;
        Ok(entities)
    }

    fn extract_bio_entities(
        &self,
        tokens: &[String],
        predictions: &[u32],
        probabilities: &Tensor,
    ) -> Result<Vec<Entity>, NovaError> {
        let mut entities = Vec::new();
        let mut current_entity: Option<(String, EntityLabel, Vec<f32>)> = None;

        for (idx, (token, &pred_id)) in tokens.iter().zip(predictions.iter()).enumerate() {
            if token == "[CLS]" || token == "[SEP]" || token == "[PAD]" {
                if let Some((text, label, confidences)) = current_entity.take() {
                    let avg_confidence = confidences.iter().sum::<f32>() / confidences.len() as f32;
                    entities.push(Entity {
                        text: clean_token_text(&text),
                        label,
                        confidence: avg_confidence,
                    });
                }
                continue;
            }

            let tag = self
                .id2label
                .get(&pred_id)
                .map(|s| s.as_str())
                .unwrap_or("O");

            let confidence = probabilities
                .get(idx)
                .ok()
                .and_then(|row| row.max(0).ok())
                .and_then(|t| t.to_scalar::<f32>().ok())
                .unwrap_or(0.0);

            if tag.starts_with("B-") {
                if let Some((text, label, confidences)) = current_entity.take() {
                    let avg_confidence = confidences.iter().sum::<f32>() / confidences.len() as f32;
                    entities.push(Entity {
                        text: clean_token_text(&text),
                        label,
                        confidence: avg_confidence,
                    });
                }

                if let Some(label) = EntityLabel::from_tag(tag) {
                    current_entity = Some((token.clone(), label, vec![confidence]));
                }
            } else if tag.starts_with("I-") {
                if let Some((ref mut text, ref label, ref mut confidences)) = current_entity {
                    let tag_label = EntityLabel::from_tag(tag);
                    if tag_label.as_ref() == Some(label) {
                        if token.starts_with("##") {
                            text.push_str(&token[2..]);
                        } else {
                            text.push(' ');
                            text.push_str(token);
                        }
                        confidences.push(confidence);
                    } else {
                        let old_text = std::mem::take(text);
                        let old_label = *label;
                        let old_confidences = std::mem::take(confidences);
                        let avg_confidence =
                            old_confidences.iter().sum::<f32>() / old_confidences.len() as f32;
                        entities.push(Entity {
                            text: clean_token_text(&old_text),
                            label: old_label,
                            confidence: avg_confidence,
                        });
                        current_entity = None;
                    }
                }
            } else {
                if let Some((text, label, confidences)) = current_entity.take() {
                    let avg_confidence = confidences.iter().sum::<f32>() / confidences.len() as f32;
                    entities.push(Entity {
                        text: clean_token_text(&text),
                        label,
                        confidence: avg_confidence,
                    });
                }
            }
        }

        if let Some((text, label, confidences)) = current_entity {
            let avg_confidence = confidences.iter().sum::<f32>() / confidences.len() as f32;
            entities.push(Entity {
                text: clean_token_text(&text),
                label,
                confidence: avg_confidence,
            });
        }

        Ok(entities)
    }
}

fn clean_token_text(text: &str) -> String {
    text.replace(" ##", "").replace("##", "").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entity_label_from_tag() {
        assert_eq!(EntityLabel::from_tag("B-PER"), Some(EntityLabel::Person));
        assert_eq!(EntityLabel::from_tag("I-PER"), Some(EntityLabel::Person));
        assert_eq!(
            EntityLabel::from_tag("B-ORG"),
            Some(EntityLabel::Organization)
        );
        assert_eq!(EntityLabel::from_tag("B-LOC"), Some(EntityLabel::Location));
        assert_eq!(EntityLabel::from_tag("B-MISC"), Some(EntityLabel::Misc));
        assert_eq!(EntityLabel::from_tag("O"), None);
    }

    #[test]
    fn test_clean_token_text() {
        assert_eq!(clean_token_text("Hello ##World"), "HelloWorld");
        assert_eq!(clean_token_text("  test  "), "test");
        assert_eq!(clean_token_text("##suffix"), "suffix");
    }

    #[test]
    fn test_model_loads() {
        let model = NerModel::new();
        assert!(model.is_ok(), "NER model should load successfully");
    }

    #[test]
    fn test_extract_entities_basic() {
        let model = NerModel::new().expect("Failed to load NER model");
        let text = "John Smith works at Microsoft in Seattle.";
        let entities = model
            .extract_entities(text)
            .expect("Failed to extract entities");

        let has_person = entities.iter().any(|e| e.label == EntityLabel::Person);
        let has_org = entities
            .iter()
            .any(|e| e.label == EntityLabel::Organization);
        let has_loc = entities.iter().any(|e| e.label == EntityLabel::Location);

        assert!(
            has_person || has_org || has_loc,
            "Should extract at least one entity from: '{}'. Got: {:?}",
            text,
            entities
        );
    }

    #[test]
    fn test_extract_entities_empty() {
        let model = NerModel::new().expect("Failed to load NER model");
        let entities = model.extract_entities("").expect("Failed on empty text");
        assert!(entities.is_empty(), "Empty text should return no entities");
    }

    #[test]
    fn test_entity_confidence_range() {
        let model = NerModel::new().expect("Failed to load NER model");
        let text = "Barack Obama was the president of the United States.";
        let entities = model
            .extract_entities(text)
            .expect("Failed to extract entities");

        for entity in &entities {
            assert!(
                entity.confidence >= 0.0 && entity.confidence <= 1.0,
                "Confidence should be between 0 and 1, got: {}",
                entity.confidence
            );
        }
    }
}
