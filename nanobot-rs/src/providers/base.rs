use std::fmt::{Display, Formatter};
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

impl ToolCall {
    pub fn to_openai_tool_call(&self) -> Value {
        json!({
            "id": self.id,
            "type": "function",
            "function": {
                "name": self.name,
                "arguments": self.arguments.to_string(),
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: String,
}

impl LlmResponse {
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: Some(message.into()),
            tool_calls: Vec::new(),
            finish_reason: "error".to_string(),
        }
    }
}

#[derive(Debug)]
pub struct ProviderError {
    message: String,
    retryable: bool,
}

impl ProviderError {
    pub fn retryable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: true,
        }
    }

    pub fn fatal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: false,
        }
    }

    pub fn is_retryable(&self) -> bool {
        self.retryable
    }
}

impl Display for ProviderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProviderError {}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn default_model(&self) -> &str;

    async fn chat(
        &self,
        messages: Vec<Value>,
        tools: Vec<Value>,
        model: &str,
    ) -> Result<LlmResponse>;

    async fn chat_with_retry(
        &self,
        messages: Vec<Value>,
        tools: Vec<Value>,
        model: &str,
    ) -> Result<LlmResponse> {
        let delays = [1u64, 2, 4];
        let mut attempt = 0usize;

        loop {
            match self.chat(messages.clone(), tools.clone(), model).await {
                Ok(response) => return Ok(response),
                Err(error) => {
                    if !should_retry(&error) || attempt >= delays.len() {
                        return Ok(LlmResponse::error(error.to_string()));
                    }
                    tokio::time::sleep(Duration::from_secs(delays[attempt])).await;
                    attempt += 1;
                }
            }
        }
    }
}

pub fn should_retry(error: &anyhow::Error) -> bool {
    if let Some(provider_error) = error.downcast_ref::<ProviderError>() {
        return provider_error.is_retryable();
    }
    if let Some(reqwest_error) = error.downcast_ref::<reqwest::Error>() {
        return reqwest_error.is_timeout()
            || reqwest_error.is_connect()
            || reqwest_error.is_request()
            || reqwest_error.is_body()
            || reqwest_error.is_decode();
    }
    false
}
