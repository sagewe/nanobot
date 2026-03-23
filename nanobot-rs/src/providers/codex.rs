use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::config::CodexProviderConfig;
use crate::providers::{
    LlmProvider, LlmResponse, ProviderError, ProviderRequestDescriptor, ToolCall,
};

#[derive(Debug, Clone)]
enum CodexEvent {
    OutputItemDone(Value),
    FunctionCallArgumentsDelta { item_id: String, delta: String },
    FunctionCallArgumentsDone { item_id: String, arguments: String },
    ResponseCompleted,
}

#[derive(Debug, Clone)]
struct PendingFunctionCall {
    name: String,
    collected_arguments: String,
    completed_arguments: Option<String>,
    saw_done: bool,
}

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
        let text = response
            .text()
            .await
            .context("failed to read codex provider body")?;

        if !status.is_success() {
            return Err(classify_http_error(status, &text).into());
        }

        parse_success_response(&text)
            .map_err(|error| ProviderError::fatal(error.to_string()).into())
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

fn classify_http_error(status: reqwest::StatusCode, body: &str) -> ProviderError {
    let details = extract_error_message(body);
    let message = format!("codex provider error {}: {}", status, details);
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        ProviderError::fatal(message)
    } else if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        ProviderError::retryable(message)
    } else {
        ProviderError::fatal(message)
    }
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

fn parse_success_response(body: &str) -> Result<LlmResponse> {
    let parsed: Value = serde_json::from_str(body)
        .with_context(|| format!("invalid codex response JSON: {body}"))?;
    parse_response_value(&parsed)
}

fn parse_response_value(value: &Value) -> Result<LlmResponse> {
    let output = value
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("codex response normalization failed: missing output array"))?;

    let mut events = Vec::new();
    let mut saw_response_completed = false;
    for item in output {
        let Some(item_type) = item.get("type").and_then(Value::as_str) else {
            continue;
        };

        match item_type {
            "response.output_item.done" => {
                let item = item.get("item").ok_or_else(|| {
                    malformed_event_error("response.output_item.done missing item")
                })?;
                events.push(CodexEvent::OutputItemDone(item.clone()));
            }
            "response.function_call_arguments.delta" => {
                events.push(CodexEvent::FunctionCallArgumentsDelta {
                    item_id: required_string_field(item, "item_id")?,
                    delta: required_string_field(item, "delta")?,
                });
            }
            "response.function_call_arguments.done" => {
                events.push(CodexEvent::FunctionCallArgumentsDone {
                    item_id: required_string_field(item, "item_id")?,
                    arguments: required_string_field(item, "arguments")?,
                });
            }
            "response.completed" => {
                saw_response_completed = true;
                events.push(CodexEvent::ResponseCompleted);
            }
            "message" => {
                events.push(CodexEvent::OutputItemDone(item.clone()));
            }
            "function_call" => {
                events.push(CodexEvent::OutputItemDone(item.clone()));
            }
            _ => {}
        }
    }

    if !saw_response_completed && value.get("status").and_then(Value::as_str) == Some("completed") {
        events.push(CodexEvent::ResponseCompleted);
    }

    aggregate_events(&events)
}

fn aggregate_events(events: &[CodexEvent]) -> Result<LlmResponse> {
    let mut content_chunks = Vec::new();
    let mut tool_calls = Vec::new();
    let mut extra = Map::new();
    let mut pending_calls: HashMap<String, PendingFunctionCall> = HashMap::new();
    let mut pending_order = Vec::new();
    let mut saw_response_completed = false;

    for event in events {
        match event {
            CodexEvent::OutputItemDone(item) => {
                if saw_response_completed {
                    return Err(malformed_event_error(
                        "event arrived after response.completed",
                    ));
                }
                match item.get("type").and_then(Value::as_str) {
                    Some("message") => {
                        if item.get("role").and_then(Value::as_str) != Some("assistant") {
                            continue;
                        }
                        merge_extra_fields(&mut extra, item, &["type", "role", "content"]);
                        content_chunks.extend(extract_message_texts(item.get("content")));
                    }
                    Some("function_call") => {
                        merge_extra_fields(
                            &mut extra,
                            item,
                            &["type", "call_id", "id", "name", "arguments"],
                        );
                        let function_call = parse_function_call_item(item)?;
                        if let Some(arguments) = function_call.arguments {
                            let name = function_call.name;
                            tool_calls.push(ToolCall {
                                id: function_call.id,
                                name: name.clone(),
                                arguments: parse_arguments_value(&name, &arguments)?,
                            });
                        } else {
                            let call_id = function_call.id.clone();
                            if pending_calls.contains_key(&call_id) {
                                return Err(malformed_event_error(format!(
                                    "duplicate function_call item for {call_id}"
                                )));
                            }
                            pending_order.push(call_id.clone());
                            pending_calls.insert(
                                call_id,
                                PendingFunctionCall {
                                    name: function_call.name,
                                    collected_arguments: String::new(),
                                    completed_arguments: None,
                                    saw_done: false,
                                },
                            );
                        }
                    }
                    _ => {}
                }
            }
            CodexEvent::FunctionCallArgumentsDelta { item_id, delta } => {
                if saw_response_completed {
                    return Err(malformed_event_error(
                        "event arrived after response.completed",
                    ));
                }
                let Some(pending) = pending_calls.get_mut(item_id) else {
                    return Err(malformed_event_error(format!(
                        "function_call_arguments.delta for unknown item_id {item_id}"
                    )));
                };
                if pending.saw_done {
                    return Err(malformed_event_error(format!(
                        "function_call_arguments.delta arrived after completion for {item_id}"
                    )));
                }
                pending.collected_arguments.push_str(delta);
            }
            CodexEvent::FunctionCallArgumentsDone { item_id, arguments } => {
                if saw_response_completed {
                    return Err(malformed_event_error(
                        "event arrived after response.completed",
                    ));
                }
                let Some(pending) = pending_calls.get_mut(item_id) else {
                    return Err(malformed_event_error(format!(
                        "function_call_arguments.done for unknown item_id {item_id}"
                    )));
                };
                if pending.saw_done {
                    return Err(malformed_event_error(format!(
                        "duplicate function_call_arguments.done for {item_id}"
                    )));
                }
                pending.saw_done = true;
                if !arguments.trim().is_empty() {
                    if !pending.collected_arguments.trim().is_empty()
                        && !arguments_payloads_match(&pending.collected_arguments, arguments)
                    {
                        return Err(malformed_event_error(format!(
                            "function_call_arguments.done for {item_id} conflicts with collected deltas"
                        )));
                    }
                    pending.completed_arguments = Some(arguments.clone());
                }
            }
            CodexEvent::ResponseCompleted => {
                if saw_response_completed {
                    return Err(malformed_event_error("duplicate response.completed event"));
                }
                saw_response_completed = true;
            }
        }
    }

    if !saw_response_completed {
        return Err(malformed_event_error("missing response.completed event"));
    }

    for call_id in pending_order {
        let pending = pending_calls.remove(&call_id).ok_or_else(|| {
            malformed_event_error(format!("missing pending function call for {call_id}"))
        })?;
        let arguments = pending
            .completed_arguments
            .filter(|arguments| !arguments.trim().is_empty())
            .unwrap_or(pending.collected_arguments);
        if arguments.trim().is_empty() {
            return Err(malformed_event_error(format!(
                "function call {call_id} missing completed arguments"
            )));
        }
        let name = pending.name;
        tool_calls.push(ToolCall {
            id: call_id,
            name: name.clone(),
            arguments: parse_arguments_value(&name, &arguments)?,
        });
    }

    if content_chunks.is_empty() && tool_calls.is_empty() {
        return Err(malformed_event_error(
            "assistant response did not contain text or tool calls",
        ));
    }

    let content = if content_chunks.is_empty() {
        None
    } else {
        Some(content_chunks.join(""))
    };
    let finish_reason = if tool_calls.is_empty() {
        "stop"
    } else {
        "tool_calls"
    }
    .to_string();

    Ok(LlmResponse {
        content,
        tool_calls,
        finish_reason,
        extra,
    })
}

#[derive(Debug, Clone)]
struct ParsedFunctionCall {
    id: String,
    name: String,
    arguments: Option<String>,
}

fn parse_function_call_item(item: &Value) -> Result<ParsedFunctionCall> {
    let id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| malformed_event_error("function_call missing call_id"))?
        .to_string();
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| malformed_event_error("function_call missing name"))?
        .to_string();
    let arguments = match item.get("arguments") {
        None => None,
        Some(value) => normalize_arguments_value(value)?,
    };
    Ok(ParsedFunctionCall {
        id,
        name,
        arguments,
    })
}

fn parse_arguments_value(name: &str, arguments: &str) -> Result<Value> {
    serde_json::from_str(arguments).with_context(|| {
        format!("codex event aggregation failed: malformed function_call arguments for {name}")
    })
}

fn arguments_payloads_match(collected: &str, completed: &str) -> bool {
    if collected == completed {
        return true;
    }

    match (
        serde_json::from_str::<Value>(collected),
        serde_json::from_str::<Value>(completed),
    ) {
        (Ok(collected), Ok(completed)) => collected == completed,
        _ => false,
    }
}

fn normalize_arguments_value(value: &Value) -> Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) if text.trim().is_empty() => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        other => {
            let text = other.to_string();
            if text.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(text))
            }
        }
    }
}

fn required_string_field(item: &Value, field: &str) -> Result<String> {
    item.get(field)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| malformed_event_error(format!("response.{field} missing string field")))
}

fn malformed_event_error(message: impl Into<String>) -> anyhow::Error {
    anyhow!(
        "codex event aggregation failed: malformed {}",
        message.into()
    )
}

fn merge_extra_fields(target: &mut Map<String, Value>, source: &Value, excluded: &[&str]) {
    let Some(object) = source.as_object() else {
        return;
    };

    for (key, value) in object {
        if excluded.iter().any(|excluded| excluded == &key.as_str()) {
            continue;
        }
        target.insert(key.clone(), value.clone());
    }
}

fn extract_message_texts(content: Option<&Value>) -> Vec<String> {
    let Some(content) = content else {
        return Vec::new();
    };
    match content {
        Value::String(text) => vec![text.clone()],
        Value::Array(items) => items.iter().filter_map(extract_content_text).collect(),
        Value::Object(_) => extract_content_text(content).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn extract_content_text(value: &Value) -> Option<String> {
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(kind) = value.get("type").and_then(Value::as_str) {
        if matches!(kind, "output_text" | "text") {
            if let Some(text) = value.get("text").and_then(Value::as_str) {
                return Some(text.to_string());
            }
        }
    }
    match value {
        Value::String(text) => Some(text.clone()),
        _ => None,
    }
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
