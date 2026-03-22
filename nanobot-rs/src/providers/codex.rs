use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::config::CodexProviderConfig;
use crate::providers::{LlmProvider, LlmResponse, ProviderError, ProviderRequestDescriptor};

#[derive(Debug, Clone)]
pub struct CodexProvider {
    client: Client,
    config: CodexProviderConfig,
    auth: CodexAuthFile,
    auth_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
struct CodexAuthFile {
    auth_mode: String,
    tokens: CodexAuthTokens,
}

#[derive(Debug, Clone, Deserialize)]
struct CodexAuthTokens {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
}

impl CodexAuthTokens {
    fn access_token(&self) -> &str {
        self.access_token
            .as_deref()
            .expect("validated codex auth access_token")
    }

    fn account_id(&self) -> &str {
        self.account_id
            .as_deref()
            .expect("validated codex auth account_id")
    }
}

impl CodexProvider {
    pub fn from_config(config: CodexProviderConfig) -> Result<Self> {
        let auth_path = resolve_auth_path(&config.auth_file)?;
        let auth = load_auth_file(&auth_path)?;
        Ok(Self {
            client: Client::new(),
            config,
            auth,
            auth_path,
        })
    }

    #[allow(dead_code)]
    pub fn auth_path(&self) -> &Path {
        &self.auth_path
    }

    #[allow(dead_code)]
    pub fn api_base(&self) -> &str {
        &self.config.api_base
    }

    #[allow(dead_code)]
    fn auth(&self) -> &CodexAuthFile {
        &self.auth
    }

    async fn send_responses_request(&self, body: &Value) -> Result<reqwest::Response> {
        self.client
            .post(format!(
                "{}/responses",
                self.config.api_base.trim_end_matches('/')
            ))
            .bearer_auth(self.auth.tokens.access_token())
            .header("ChatGPT-Account-Id", self.auth.tokens.account_id())
            .json(body)
            .send()
            .await
            .map_err(Into::into)
    }
}

#[async_trait]
impl LlmProvider for CodexProvider {
    fn default_model(&self) -> &str {
        "codex"
    }

    async fn chat(
        &self,
        messages: Vec<Value>,
        tools: Vec<Value>,
        model: &str,
    ) -> Result<LlmResponse> {
        let request = ProviderRequestDescriptor::new("codex", model, Map::new());
        self.chat_with_request(messages, tools, &request).await
    }

    async fn chat_with_request(
        &self,
        messages: Vec<Value>,
        tools: Vec<Value>,
        request: &ProviderRequestDescriptor,
    ) -> Result<LlmResponse> {
        let body = build_request_body(messages, tools, request);
        let response = self.send_responses_request(&body).await?;
        let status = response.status();

        if status.is_success() {
            return Err(ProviderError::fatal(
                "codex response normalization is not implemented yet",
            )
            .into());
        }

        let text = response
            .text()
            .await
            .context("failed to read codex provider body")?;

        let details = if text.trim().is_empty() {
            "empty response body".to_string()
        } else {
            text.trim().to_string()
        };
        Err(ProviderError::fatal(format!("codex provider error {}: {}", status, details)).into())
    }
}

pub fn cache_key(config: &CodexProviderConfig) -> String {
    format!(
        "codex\n{}\n{}",
        config.auth_file.trim(),
        config.api_base.trim_end_matches('/')
    )
}

fn build_request_body(
    messages: Vec<Value>,
    tools: Vec<Value>,
    request: &ProviderRequestDescriptor,
) -> Value {
    let mut body = request.request_extras.clone();
    body.insert("model".to_string(), json!(request.model_name));
    body.insert(
        "input".to_string(),
        Value::Array(messages.into_iter().map(map_input_message).collect()),
    );
    body.insert("tools".to_string(), Value::Array(tools));
    Value::Object(body)
}

fn map_input_message(message: Value) -> Value {
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("user")
        .to_string();
    let content = map_input_content(message.get("content").cloned().unwrap_or(Value::Null));
    json!({
        "role": role,
        "content": content,
    })
}

fn map_input_content(content: Value) -> Vec<Value> {
    match content {
        Value::Null => Vec::new(),
        Value::String(text) => vec![json!({"type": "input_text", "text": text})],
        Value::Array(items) => items.into_iter().map(map_input_content_item).collect(),
        other => vec![json!({"type": "input_text", "text": other.to_string()})],
    }
}

fn map_input_content_item(item: Value) -> Value {
    match item {
        Value::String(text) => json!({"type": "input_text", "text": text}),
        Value::Object(map) if map.get("type").is_some() => Value::Object(map),
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                json!({"type": "input_text", "text": text})
            } else {
                json!({"type": "input_text", "text": Value::Object(map).to_string()})
            }
        }
        other => json!({"type": "input_text", "text": other.to_string()}),
    }
}

fn resolve_auth_path(raw_path: &str) -> Result<PathBuf> {
    let path = raw_path.trim();
    if path.is_empty() {
        bail!("codex auth file path must not be empty");
    }
    if path == "~" {
        return home_dir()
            .ok_or_else(|| anyhow!("failed to resolve home directory for codex auth file"));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return Ok(home_dir()
            .ok_or_else(|| anyhow!("failed to resolve home directory for codex auth file"))?
            .join(rest));
    }
    Ok(PathBuf::from(path))
}

fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

fn load_auth_file(path: &Path) -> Result<CodexAuthFile> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read codex auth file at {}", path.display()))?;
    let auth: CodexAuthFile = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse codex auth file at {}", path.display()))?;

    validate_auth(&auth)?;
    Ok(auth)
}

fn validate_auth(auth: &CodexAuthFile) -> Result<()> {
    if auth.auth_mode != "chatgpt" {
        bail!(
            "codex auth file auth_mode must be 'chatgpt' (found '{}')",
            auth.auth_mode
        );
    }

    validate_required_token("access_token", auth.tokens.access_token.as_deref())?;
    validate_required_token("refresh_token", auth.tokens.refresh_token.as_deref())?;
    validate_required_token("id_token", auth.tokens.id_token.as_deref())?;
    validate_required_token("account_id", auth.tokens.account_id.as_deref())?;
    Ok(())
}

fn validate_required_token(field: &str, value: Option<&str>) -> Result<()> {
    let value = value.ok_or_else(|| anyhow!("codex auth file missing required field '{field}'"))?;
    if value.trim().is_empty() {
        bail!("codex auth file {field} must not be empty");
    }
    Ok(())
}
