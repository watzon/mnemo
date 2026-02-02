//! Remote curator provider using OpenAI-compatible APIs
//!
//! Implements the CuratorProvider trait for remote LLM APIs via HTTP.
//! Supports any OpenAI-compatible endpoint with configurable URL, model,
//! and API key via environment variable.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::config::RemoteCuratorConfig;
use crate::curator::prompts::{CLASSIFICATION_PROMPT, EXTRACTION_PROMPT};
use crate::curator::types::{CuratedMemory, CurationResult, CuratorError};
use crate::curator::CuratorProvider;
use crate::memory::types::MemoryType;

/// Remote curator using OpenAI-compatible HTTP APIs
#[derive(Debug)]
pub struct RemoteCurator {
    client: Client,
    config: RemoteCuratorConfig,
    api_key: String,
}

/// OpenAI-compatible chat completion request
#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
    max_tokens: u32,
}

/// Message in the chat completion request
#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

/// OpenAI-compatible chat completion response
#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

/// Choice in the chat completion response
#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

/// Message in the response choice
#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: String,
}

/// JSON representation of an extracted memory
#[derive(Debug, Deserialize)]
struct ExtractedMemoryJson {
    #[serde(rename = "type")]
    memory_type: String,
    content: String,
    importance: f32,
    entities: Vec<String>,
}

impl RemoteCurator {
    /// Create a new remote curator with the given configuration
    ///
    /// Reads the API key from the environment variable specified in config.api_key_env.
    /// Returns an error if the environment variable is not set.
    pub fn new(config: &RemoteCuratorConfig) -> Result<Self, CuratorError> {
        let api_key = env::var(&config.api_key_env).map_err(|_| {
            CuratorError::ConfigError(format!(
                "API key env var '{}' not set",
                config.api_key_env
            ))
        })?;

        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| CuratorError::ApiError(e.to_string()))?;

        info!(
            "RemoteCurator initialized with model: {}, api_url: {}",
            config.model, config.api_url
        );

        Ok(Self {
            client,
            config: config.clone(),
            api_key,
        })
    }

    /// Call the remote API with exponential backoff for rate limiting
    ///
    /// Makes up to 3 retries with backoff delays of 1s, 2s, 4s on 429 errors.
    async fn call_api(&self, prompt: &str) -> Result<String, CuratorError> {
        let request = ChatCompletionRequest {
            model: self.config.model.clone(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: "You are a helpful assistant.".to_string(),
                },
                Message {
                    role: "user".to_string(),
                    content: prompt.to_string(),
                },
            ],
            temperature: 0.2,
            max_tokens: 1024,
        };

        let url = format!("{}/chat/completions", self.config.api_url.trim_end_matches('/'));
        debug!("Calling remote API at: {}", url);

        let mut last_error = None;
        let mut delay = Duration::from_secs(1);
        const MAX_RETRIES: u32 = 3;

        for attempt in 0..MAX_RETRIES {
            match self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&request)
                .send()
                .await
            {
                Ok(response) => {
                    let status = response.status();

                    if status == 429 {
                        warn!(
                            "Rate limited on attempt {}/{}, waiting {:?}",
                            attempt + 1,
                            MAX_RETRIES,
                            delay
                        );
                        tokio::time::sleep(delay).await;
                        delay *= 2; // Exponential backoff
                        continue;
                    }

                    if !status.is_success() {
                        let error_text = response
                            .text()
                            .await
                            .unwrap_or_else(|_| "Unknown error".to_string());
                        return Err(CuratorError::ApiError(format!(
                            "API returned {status}: {error_text}"
                        )));
                    }

                    let completion: ChatCompletionResponse = response
                        .json()
                        .await
                        .map_err(|e| CuratorError::ParseError(e.to_string()))?;

                    return completion
                        .choices
                        .into_iter()
                        .next()
                        .map(|c| c.message.content)
                        .ok_or_else(|| CuratorError::ApiError("Empty response".to_string()));
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    last_error = Some(err_msg.clone());
                    if attempt < MAX_RETRIES - 1 {
                        warn!(
                            "Request failed on attempt {}/{}, retrying: {}",
                            attempt + 1,
                            MAX_RETRIES,
                            err_msg
                        );
                        tokio::time::sleep(delay).await;
                        delay *= 2;
                    }
                }
            }
        }

        Err(CuratorError::ApiError(format!(
            "Failed after {} retries: {}",
            MAX_RETRIES,
            last_error.unwrap_or_else(|| "Unknown error".to_string())
        )))
    }

    /// Classify whether a conversation should be stored as memory
    ///
    /// Returns true if the conversation contains memory-worthy information.
    async fn classify(&self, conversation: &str) -> Result<bool, CuratorError> {
        let prompt = CLASSIFICATION_PROMPT.replace("{conversation}", conversation);
        let response = self.call_api(&prompt).await?;
        debug!("Classification response: {}", response);
        Ok(response.trim().to_uppercase().contains("YES"))
    }

    /// Extract memories from a conversation
    ///
    /// Returns a vector of curated memories extracted from the conversation.
    async fn extract(&self, conversation: &str) -> Result<Vec<CuratedMemory>, CuratorError> {
        let prompt = EXTRACTION_PROMPT.replace("{conversation}", conversation);
        let response = self.call_api(&prompt).await?;
        debug!("Extraction response: {}", response);

        // Parse JSON response
        let extracted: Vec<ExtractedMemoryJson> =
            serde_json::from_str(&response).map_err(|e| {
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
impl CuratorProvider for RemoteCurator {
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
            "Extracted via remote API".to_string(),
        ))
    }

    async fn is_available(&self) -> bool {
        // For now, assume available if we have an API key
        // Could add a health check endpoint call here in the future
        !self.api_key.is_empty() && !self.config.api_url.is_empty()
    }

    fn name(&self) -> &'static str {
        "remote"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn create_test_config(api_url: String) -> RemoteCuratorConfig {
        RemoteCuratorConfig {
            api_url,
            api_key_env: "TEST_API_KEY".to_string(),
            model: "gpt-4o-mini".to_string(),
            timeout_secs: 30,
        }
    }

    #[tokio::test]
    async fn test_remote_curator_new_missing_api_key() {
        // Ensure the env var is not set
        unsafe { env::remove_var("TEST_API_KEY") };

        let config = create_test_config("https://api.example.com/v1".to_string());
        let result = RemoteCurator::new(&config);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("TEST_API_KEY"));
    }

    #[tokio::test]
    async fn test_remote_curator_classify_yes() {
        let mock_server = MockServer::start().await;
        let api_url = mock_server.uri();

        // Mock the API response for classification returning YES
        let response_body = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "YES - This contains important user preferences"
                }
            }]
        });

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
            .mount(&mock_server)
            .await;

        unsafe { env::set_var("TEST_API_KEY", "test-key") };
        let config = create_test_config(api_url);
        let curator = RemoteCurator::new(&config).unwrap();

        let result = curator.classify("User said they prefer dark mode").await;
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_remote_curator_classify_no() {
        let mock_server = MockServer::start().await;
        let api_url = mock_server.uri();

        // Mock the API response for classification returning NO
        let response_body = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "NO - This is just casual conversation"
                }
            }]
        });

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
            .mount(&mock_server)
            .await;

        unsafe { env::set_var("TEST_API_KEY", "test-key") };
        let config = create_test_config(api_url);
        let curator = RemoteCurator::new(&config).unwrap();

        let result = curator.classify("Hello, how are you?").await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_remote_curator_extract_memories() {
        let mock_server = MockServer::start().await;
        let api_url = mock_server.uri();

        // Mock the API response for extraction returning JSON
        let response_body = serde_json::json!({
            "choices": [{
                "message": {
                    "content": r#"[
                        {
                            "type": "semantic",
                            "content": "User prefers dark mode interfaces",
                            "importance": 0.8,
                            "entities": ["dark mode", "UI preferences"]
                        },
                        {
                            "type": "episodic",
                            "content": "User mentioned they are learning Rust",
                            "importance": 0.7,
                            "entities": ["Rust", "learning"]
                        }
                    ]"#
                }
            }]
        });

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
            .mount(&mock_server)
            .await;

        unsafe { env::set_var("TEST_API_KEY", "test-key") };
        let config = create_test_config(api_url);
        let curator = RemoteCurator::new(&config).unwrap();

        let result = curator
            .extract("User said they prefer dark mode and are learning Rust")
            .await;
        assert!(result.is_ok());

        let memories = result.unwrap();
        assert_eq!(memories.len(), 2);
        assert_eq!(memories[0].memory_type, MemoryType::Semantic);
        assert_eq!(memories[0].content, "User prefers dark mode interfaces");
        assert_eq!(memories[0].importance, 0.8);
        assert_eq!(memories[1].memory_type, MemoryType::Episodic);
    }

    #[tokio::test]
    async fn test_remote_curator_curate_full_flow() {
        let mock_server = MockServer::start().await;
        let api_url = mock_server.uri();

        // First call: classification (YES)
        let classify_response = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "YES - Contains important preferences"
                }
            }]
        });

        // Second call: extraction
        let extract_response = serde_json::json!({
            "choices": [{
                "message": {
                    "content": r#"[
                        {
                            "type": "semantic",
                            "content": "User prefers VS Code",
                            "importance": 0.9,
                            "entities": ["VS Code", "editor"]
                        }
                    ]"#
                }
            }]
        });

        // Mount both mocks - they'll match in order
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(classify_response))
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(extract_response))
            .mount(&mock_server)
            .await;

        unsafe { env::set_var("TEST_API_KEY", "test-key") };
        let config = create_test_config(api_url);
        let curator = RemoteCurator::new(&config).unwrap();

        let result = curator.curate("User said they prefer VS Code").await;
        assert!(result.is_ok());

        let curation = result.unwrap();
        assert!(curation.should_store);
        assert_eq!(curation.memories.len(), 1);
        assert_eq!(curation.memories[0].content, "User prefers VS Code");
    }

    #[tokio::test]
    async fn test_remote_curator_curate_should_not_store() {
        let mock_server = MockServer::start().await;
        let api_url = mock_server.uri();

        // Classification returns NO
        let response_body = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "NO - Just casual greeting"
                }
            }]
        });

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
            .mount(&mock_server)
            .await;

        unsafe { env::set_var("TEST_API_KEY", "test-key") };
        let config = create_test_config(api_url);
        let curator = RemoteCurator::new(&config).unwrap();

        let result = curator.curate("Hello!").await;
        assert!(result.is_ok());

        let curation = result.unwrap();
        assert!(!curation.should_store);
        assert!(curation.memories.is_empty());
    }

    #[tokio::test]
    async fn test_remote_curator_rate_limit_retry() {
        let mock_server = MockServer::start().await;
        let api_url = mock_server.uri();

        // First call returns 429, second succeeds
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        let success_response = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "YES"
                }
            }]
        });

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(success_response))
            .mount(&mock_server)
            .await;

        unsafe { env::set_var("TEST_API_KEY", "test-key") };
        let config = create_test_config(api_url);
        let curator = RemoteCurator::new(&config).unwrap();

        let start = std::time::Instant::now();
        let result = curator.classify("Test conversation").await;
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        assert!(result.unwrap());
        // Should have waited at least 1 second for retry
        assert!(elapsed >= Duration::from_millis(900));
    }

    #[tokio::test]
    async fn test_remote_curator_api_error() {
        let mock_server = MockServer::start().await;
        let api_url = mock_server.uri();

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&mock_server)
            .await;

        unsafe { env::set_var("TEST_API_KEY", "test-key") };
        let config = create_test_config(api_url);
        let curator = RemoteCurator::new(&config).unwrap();

        let result = curator.classify("Test").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("500"));
    }

    #[tokio::test]
    async fn test_remote_curator_is_available() {
        unsafe { env::set_var("TEST_API_KEY", "test-key") };
        let config = create_test_config("https://api.example.com/v1".to_string());
        let curator = RemoteCurator::new(&config).unwrap();

        assert!(curator.is_available().await);
    }

    #[tokio::test]
    async fn test_remote_curator_name() {
        unsafe { env::set_var("TEST_API_KEY", "test-key") };
        let config = create_test_config("https://api.example.com/v1".to_string());
        let curator = RemoteCurator::new(&config).unwrap();

        assert_eq!(curator.name(), "remote");
    }

    #[tokio::test]
    async fn test_remote_curator_invalid_json_response() {
        let mock_server = MockServer::start().await;
        let api_url = mock_server.uri();

        // Return invalid JSON for extraction
        let response_body = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "not valid json"
                }
            }]
        });

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
            .mount(&mock_server)
            .await;

        unsafe { env::set_var("TEST_API_KEY", "test-key") };
        let config = create_test_config(api_url);
        let curator = RemoteCurator::new(&config).unwrap();

        let result = curator.extract("Test conversation").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("parse") || err.contains("JSON"));
    }
}
