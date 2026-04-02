use std::collections::HashMap;

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{Map, Value, json};

use super::{
    LlmProvider, LlmResponse, ProviderError, ProviderRequestDescriptor, ResolvedProviderConfig,
    ToolCall,
};

#[derive(Clone)]
pub struct OpenAICompatibleProvider {
    client: reqwest::Client,
    api_key: String,
    api_base: String,
    default_model: String,
    extra_headers: HeaderMap,
}

impl OpenAICompatibleProvider {
    pub fn new(api_key: String, api_base: String, default_model: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            api_base,
            default_model,
            extra_headers: HeaderMap::new(),
        }
    }

    pub fn from_config(config: ResolvedProviderConfig) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::new(),
            api_key: config.api_key,
            api_base: config.api_base,
            default_model: config.default_model,
            extra_headers: build_headers(&config.extra_headers)?,
        })
    }
}

#[async_trait]
impl LlmProvider for OpenAICompatibleProvider {
    fn default_model(&self) -> &str {
        &self.default_model
    }

    async fn chat(
        &self,
        messages: Vec<Value>,
        tools: Vec<Value>,
        model: &str,
    ) -> Result<LlmResponse> {
        let request = ProviderRequestDescriptor::new("openai", model, Map::new());
        self.chat_with_request(messages, tools, &request).await
    }

    async fn chat_with_request(
        &self,
        messages: Vec<Value>,
        tools: Vec<Value>,
        request: &ProviderRequestDescriptor,
    ) -> Result<LlmResponse> {
        let mut body = request.request_extras.clone();
        body.insert("model".to_string(), json!(request.model_name));
        body.insert("messages".to_string(), Value::Array(messages));
        body.insert(
            "tools".to_string(),
            if tools.is_empty() {
                Value::Null
            } else {
                Value::Array(tools)
            },
        );
        let url = format!("{}/chat/completions", self.api_base.trim_end_matches('/'));
        let mut request = self
            .client
            .post(url)
            .headers(self.extra_headers.clone())
            .json(&Value::Object(body));
        if !self.api_key.is_empty() {
            request = request.bearer_auth(&self.api_key);
        }

        let response = request.send().await?;
        let status = response.status();
        let text = response
            .text()
            .await
            .context("failed to read provider body")?;

        if !status.is_success() {
            let details = extract_error_message(&text);
            let message = format!("provider error {}: {}", status, details);
            let retryable = status.as_u16() == 429
                || status.is_server_error()
                || details.to_ascii_lowercase().contains("overloaded")
                || details
                    .to_ascii_lowercase()
                    .contains("temporarily unavailable");
            let error = if retryable {
                ProviderError::retryable(message)
            } else {
                ProviderError::fatal(message)
            };
            return Err(error.into());
        }

        let value: Value = serde_json::from_str(&text)
            .with_context(|| format!("invalid provider JSON: {text}"))?;
        let choice = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .ok_or_else(|| anyhow::anyhow!("provider returned no choices"))?;
        let message = choice
            .get("message")
            .and_then(Value::as_object)
            .ok_or_else(|| anyhow::anyhow!("provider returned no message"))?;
        let mut extra = message.clone();
        extra.remove("role");
        extra.remove("content");
        extra.remove("tool_calls");
        let content = message.get("content").and_then(normalize_content);
        let tool_calls = message
            .get("tool_calls")
            .and_then(Value::as_array)
            .map(|calls| {
                calls
                    .iter()
                    .filter_map(|call| {
                        let id = call.get("id")?.as_str()?.to_string();
                        let function = call.get("function")?;
                        let name = function.get("name")?.as_str()?.to_string();
                        let raw_args = function.get("arguments")?.as_str().unwrap_or("{}");
                        let arguments = serde_json::from_str(raw_args)
                            .unwrap_or_else(|_| json!({ "_raw": raw_args }));
                        Some(ToolCall {
                            id,
                            name,
                            arguments,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let finish_reason = choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .unwrap_or("stop")
            .to_string();
        Ok(LlmResponse {
            content,
            tool_calls,
            finish_reason,
            extra,
        })
    }
}

fn build_headers(raw_headers: &HashMap<String, String>) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    for (name, value) in raw_headers {
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .with_context(|| format!("invalid header name '{name}'"))?;
        let header_value =
            HeaderValue::from_str(value).with_context(|| format!("invalid header '{name}'"))?;
        headers.insert(header_name, header_value);
    }
    Ok(headers)
}

fn extract_error_message(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
            return message.to_string();
        }
        if let Some(message) = value.pointer("/message").and_then(Value::as_str) {
            return message.to_string();
        }
        if let Some(message) = value.pointer("/error").and_then(Value::as_str) {
            return message.to_string();
        }
        return value.to_string();
    }
    let trimmed = body.trim();
    if trimmed.is_empty() {
        "empty response body".to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_content(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let mut chunks = Vec::new();
            for item in items {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    chunks.push(text.to_string());
                }
            }
            if chunks.is_empty() {
                None
            } else {
                Some(chunks.join("\n"))
            }
        }
        _ => Some(value.to_string()),
    }
}
