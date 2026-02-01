use axum::http::HeaderMap;
use serde_json::Value;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    OpenAI,
    Anthropic,
    Unknown,
}

impl Provider {
    pub fn detect(url: &Url, headers: &HeaderMap, body: &Value) -> Self {
        if let Some(provider) = Self::detect_from_url(url) {
            return provider;
        }

        if let Some(provider) = Self::detect_from_headers(headers) {
            return provider;
        }

        if let Some(provider) = Self::detect_from_body(body) {
            return provider;
        }

        Provider::Unknown
    }

    fn detect_from_url(url: &Url) -> Option<Self> {
        let host = url.host_str()?;
        let host_lower = host.to_lowercase();

        if host_lower.ends_with("openai.com") || host_lower == "openai.com" {
            return Some(Provider::OpenAI);
        }

        if host_lower.ends_with("anthropic.com") || host_lower == "anthropic.com" {
            return Some(Provider::Anthropic);
        }

        None
    }

    fn detect_from_headers(headers: &HeaderMap) -> Option<Self> {
        for (name, _value) in headers {
            let name_lower = name.as_str().to_lowercase();

            if name_lower == "x-api-key" {
                return Some(Provider::Anthropic);
            }
        }

        if let Some(auth) = headers.get("authorization") {
            if let Ok(auth_str) = auth.to_str() {
                if auth_str.to_lowercase().starts_with("bearer") {
                    return Some(Provider::OpenAI);
                }
            }
        }

        None
    }

    fn detect_from_body(body: &Value) -> Option<Self> {
        if body.get("system").is_some() {
            return Some(Provider::Anthropic);
        }

        if body.get("max_tokens").is_some() {
            return Some(Provider::Anthropic);
        }

        if let Some(messages) = body.get("messages") {
            if let Some(messages_array) = messages.as_array() {
                for message in messages_array {
                    if let Some(content) = message.get("content") {
                        if content.is_array() {
                            return Some(Provider::Anthropic);
                        }
                    }

                    if let Some(role) = message.get("role") {
                        if let Some(role_str) = role.as_str() {
                            if role_str == "system" {
                                return Some(Provider::OpenAI);
                            }
                        }
                    }
                }
            }
        }

        None
    }
}
