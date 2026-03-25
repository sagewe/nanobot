use std::collections::VecDeque;
use std::fs;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use axum::extract::State;
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::routing::any;
use axum::{Json, Router};
use nanobot_rs::agent::{AgentLoop, ContextBuilder, SubagentManager};
use nanobot_rs::bus::{InboundMessage, MessageBus};
use nanobot_rs::config::{AgentProfileConfig, Config, WebToolsConfig};
use nanobot_rs::providers::{
    LlmProvider, LlmResponse, ProviderPool, ProviderRequestDescriptor, ToolCall,
};
use serde_json::{json, Map, Value};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify};

#[derive(Clone)]
struct MockProvider {
    model: String,
    responses: Arc<Mutex<VecDeque<LlmResponse>>>,
}

#[async_trait]
impl LlmProvider for MockProvider {
    fn default_model(&self) -> &str {
        &self.model
    }

    async fn chat(
        &self,
        _messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        _model: &str,
    ) -> Result<LlmResponse> {
        self.responses
            .lock()
            .await
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("no more responses"))
    }
}

fn mock_provider(responses: Vec<LlmResponse>) -> Arc<dyn LlmProvider> {
    Arc::new(MockProvider {
        model: "mock-model".to_string(),
        responses: Arc::new(Mutex::new(responses.into())),
    })
}

#[derive(Clone, Debug)]
struct CapturedCodexRequest {
    method: String,
    path: String,
    authorization: Option<String>,
    account_id: Option<String>,
    accept: Option<String>,
    body: Value,
}

#[derive(Clone)]
struct CodexCaptureState {
    requests: Arc<Mutex<Vec<CapturedCodexRequest>>>,
    responses: Arc<Mutex<VecDeque<(StatusCode, String)>>>,
}

async fn capture_codex_responses_request(
    State(state): State<CodexCaptureState>,
    method: Method,
    headers: HeaderMap,
    uri: Uri,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    state.requests.lock().await.push(CapturedCodexRequest {
        method: method.as_str().to_string(),
        path: uri.path().to_string(),
        authorization: headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(ToString::to_string),
        account_id: headers
            .get("ChatGPT-Account-Id")
            .and_then(|value| value.to_str().ok())
            .map(ToString::to_string),
        accept: headers
            .get(header::ACCEPT)
            .and_then(|value| value.to_str().ok())
            .map(ToString::to_string),
        body: payload,
    });

    let (status, body) = state
        .responses
        .lock()
        .await
        .pop_front()
        .expect("unexpected extra codex request");
    (
        status,
        [
            (header::CONTENT_TYPE, "text/event-stream"),
            (header::CACHE_CONTROL, "no-cache"),
            (header::CONNECTION, "keep-alive"),
        ],
        body,
    )
}

async fn start_codex_capture_server(
    responses: Vec<(StatusCode, String)>,
) -> (SocketAddr, Arc<Mutex<Vec<CapturedCodexRequest>>>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route(
            "/backend-api/codex/responses",
            any(capture_codex_responses_request),
        )
        .with_state(CodexCaptureState {
            requests: requests.clone(),
            responses: Arc::new(Mutex::new(responses.into_iter().collect())),
        });
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (addr, requests)
}

fn codex_sse_body(events: Vec<(&str, Value)>) -> String {
    let mut lines = Vec::new();
    for (event_name, payload) in events {
        lines.push(format!("event: {event_name}"));
        lines.push(format!("data: {}", payload));
        lines.push(String::new());
    }
    lines.push("data: [DONE]".to_string());
    lines.push(String::new());
    lines.join("\n")
}

fn response_output_item_done(item: Value) -> Value {
    json!({
        "type": "response.output_item.done",
        "item": item,
    })
}

fn response_function_call_arguments_done(item_id: &str, arguments: &str) -> Value {
    json!({
        "type": "response.function_call_arguments.done",
        "item_id": item_id,
        "arguments": arguments,
    })
}

fn response_completed() -> Value {
    json!({"type": "response.completed"})
}

fn codex_agent_config(
    workspace: &std::path::Path,
    auth_file: &std::path::Path,
    api_base: String,
) -> Config {
    let mut config = Config::default();
    config.agents.defaults.workspace = workspace.display().to_string();
    config.agents.defaults.default_profile = "openai:gpt-4.1-mini".to_string();
    config.agents.profiles.insert(
        "codex:gpt-5.4".to_string(),
        AgentProfileConfig {
            provider: "codex".to_string(),
            model: "gpt-5.4".to_string(),
            request: Map::new(),
        },
    );
    config.providers.codex.auth_file = auth_file.display().to_string();
    config.providers.codex.api_base = api_base;
    config
}

fn valid_codex_auth_json() -> &'static str {
    r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "access-token",
    "refresh_token": "refresh-token",
    "id_token": "id-token",
    "account_id": "account-id"
  }
}"#
}

#[derive(Default)]
struct ConcurrentProviderState {
    plain_started: AtomicBool,
    plain_finished: AtomicBool,
    tool_second_seen: AtomicBool,
    plain_started_notify: Notify,
    plain_finished_notify: Notify,
    tool_second_notify: Notify,
}

#[derive(Clone)]
struct ConcurrentProvider {
    state: Arc<ConcurrentProviderState>,
}

#[async_trait]
impl LlmProvider for ConcurrentProvider {
    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn chat(
        &self,
        messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        _model: &str,
    ) -> Result<LlmResponse> {
        let user_content = messages
            .iter()
            .rev()
            .find(|message| message.get("role").and_then(|role| role.as_str()) == Some("user"))
            .and_then(|message| message.get("content").and_then(|content| content.as_str()))
            .unwrap_or_default();
        let has_tool_result = messages
            .iter()
            .any(|message| message.get("role").and_then(|role| role.as_str()) == Some("tool"));

        if user_content.ends_with("plain") && !has_tool_result {
            self.state.plain_started.store(true, Ordering::SeqCst);
            self.state.plain_started_notify.notify_waiters();
            while !self.state.tool_second_seen.load(Ordering::SeqCst) {
                self.state.tool_second_notify.notified().await;
            }
            return Ok(LlmResponse {
                content: Some("plain final".to_string()),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
                extra: Map::new(),
            });
        }

        if user_content.ends_with("tool") && !has_tool_result {
            return Ok(LlmResponse {
                content: Some("sending".to_string()),
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "message".to_string(),
                    arguments: json!({"content": "tool reply"}),
                }],
                finish_reason: "tool_calls".to_string(),
                extra: Map::new(),
            });
        }

        if user_content.ends_with("tool") && has_tool_result {
            self.state.tool_second_seen.store(true, Ordering::SeqCst);
            self.state.tool_second_notify.notify_waiters();
            while !self.state.plain_finished.load(Ordering::SeqCst) {
                self.state.plain_finished_notify.notified().await;
            }
            return Ok(LlmResponse {
                content: Some("tool final".to_string()),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
                extra: Map::new(),
            });
        }

        Err(anyhow::anyhow!(
            "unexpected request shape for concurrent provider"
        ))
    }
}

#[derive(Debug, Clone)]
struct RecordedRequest {
    provider: String,
    model: String,
    extras: Map<String, Value>,
}

#[derive(Clone)]
struct RequestRecordingProvider {
    records: Arc<Mutex<Vec<RecordedRequest>>>,
}

#[async_trait]
impl LlmProvider for RequestRecordingProvider {
    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn chat(
        &self,
        _messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        _model: &str,
    ) -> Result<LlmResponse> {
        Err(anyhow::anyhow!(
            "chat() should not be used for profile-aware tests"
        ))
    }

    async fn chat_with_request(
        &self,
        _messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        request: &ProviderRequestDescriptor,
    ) -> Result<LlmResponse> {
        self.records.lock().await.push(RecordedRequest {
            provider: request.provider_name.clone(),
            model: request.model_name.clone(),
            extras: request.request_extras.clone(),
        });
        Ok(LlmResponse {
            content: Some("ok".to_string()),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
            extra: Map::new(),
        })
    }
}

#[derive(Clone)]
struct ReplayAwareProvider {
    call_count: Arc<Mutex<usize>>,
}

#[async_trait]
impl LlmProvider for ReplayAwareProvider {
    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn chat(
        &self,
        _messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        _model: &str,
    ) -> Result<LlmResponse> {
        Err(anyhow::anyhow!(
            "chat() should not be used for replay-aware tests"
        ))
    }

    async fn chat_with_request(
        &self,
        messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        _request: &ProviderRequestDescriptor,
    ) -> Result<LlmResponse> {
        let mut call_count = self.call_count.lock().await;
        match *call_count {
            0 => {
                *call_count += 1;
                let mut extra = Map::new();
                extra.insert("reasoning_content".to_string(), json!("chain"));
                Ok(LlmResponse {
                    content: Some("thinking".to_string()),
                    tool_calls: vec![ToolCall {
                        id: "call_1".to_string(),
                        name: "list_dir".to_string(),
                        arguments: json!({"path": "."}),
                    }],
                    finish_reason: "tool_calls".to_string(),
                    extra,
                })
            }
            1 => {
                let saw_reasoning = messages.iter().any(|message| {
                    message.get("role").and_then(Value::as_str) == Some("assistant")
                        && message.get("reasoning_content").and_then(Value::as_str) == Some("chain")
                });
                if !saw_reasoning {
                    return Err(anyhow::anyhow!(
                        "missing reasoning_content in replayed assistant message"
                    ));
                }
                *call_count += 1;
                Ok(LlmResponse {
                    content: Some("done".to_string()),
                    tool_calls: Vec::new(),
                    finish_reason: "stop".to_string(),
                    extra: Map::new(),
                })
            }
            _ => Err(anyhow::anyhow!("unexpected extra call")),
        }
    }
}

#[derive(Default)]
struct BtwState {
    main_started: AtomicBool,
    main_released: AtomicBool,
    main_start_count: AtomicUsize,
    btw_started: AtomicBool,
    btw_released: AtomicBool,
    btw_ready: AtomicBool,
    btw_completed: AtomicBool,
    main_started_notify: Notify,
    main_start_count_notify: Notify,
    main_release_notify: Notify,
    btw_started_notify: Notify,
    btw_release_notify: Notify,
    btw_ready_notify: Notify,
    btw_completed_notify: Notify,
    records: Arc<Mutex<Vec<RecordedRequest>>>,
    histories: Arc<Mutex<Vec<Vec<Value>>>>,
}

#[derive(Clone)]
struct BtwTestProvider {
    state: Arc<BtwState>,
}

impl BtwTestProvider {
    fn user_content(messages: &[Value]) -> String {
        messages
            .iter()
            .rev()
            .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
            .and_then(|message| message.get("content").and_then(Value::as_str))
            .map(ContextBuilder::strip_runtime_prefix)
            .flatten()
            .unwrap_or_default()
            .to_string()
    }

    fn strip_btw_prefix(content: &str) -> &str {
        content
            .strip_prefix("/btw ")
            .or_else(|| content.strip_prefix("/btw"))
            .unwrap_or(content)
            .trim()
    }

    async fn record_request(
        &self,
        messages: &[Value],
        request: Option<&ProviderRequestDescriptor>,
    ) {
        self.state.histories.lock().await.push(messages.to_vec());
        if let Some(request) = request {
            self.state.records.lock().await.push(RecordedRequest {
                provider: request.provider_name.clone(),
                model: request.model_name.clone(),
                extras: request.request_extras.clone(),
            });
        }
    }

    async fn reply(
        &self,
        messages: Vec<Value>,
        request: Option<&ProviderRequestDescriptor>,
    ) -> Result<LlmResponse> {
        self.record_request(&messages, request).await;
        let content = Self::user_content(&messages);
        let normalized = Self::strip_btw_prefix(&content);
        let has_tool_result = messages
            .iter()
            .any(|message| message.get("role").and_then(Value::as_str) == Some("tool"));

        if normalized.contains("inflight-shadow") {
            if !has_tool_result {
                return Ok(LlmResponse {
                    content: Some("inflight-shadow".to_string()),
                    tool_calls: vec![ToolCall {
                        id: "call_1".to_string(),
                        name: "list_dir".to_string(),
                        arguments: json!({"path": "."}),
                    }],
                    finish_reason: "tool_calls".to_string(),
                    extra: Map::new(),
                });
            }
            self.state.main_started.store(true, Ordering::SeqCst);
            self.state.main_started_notify.notify_waiters();
            wait_for_flag(&self.state.main_released, &self.state.main_release_notify).await;
            return Ok(LlmResponse {
                content: Some("main final".to_string()),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
                extra: Map::new(),
            });
        }

        if normalized.contains("main-hold") || normalized.contains("main block") {
            self.state.main_started.store(true, Ordering::SeqCst);
            self.state.main_start_count.fetch_add(1, Ordering::SeqCst);
            self.state.main_started_notify.notify_waiters();
            self.state.main_start_count_notify.notify_waiters();
            wait_for_flag(&self.state.main_released, &self.state.main_release_notify).await;
            return Ok(LlmResponse {
                content: Some("main final".to_string()),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
                extra: Map::new(),
            });
        }

        if normalized.contains("hold-btw") {
            self.state.btw_started.store(true, Ordering::SeqCst);
            self.state.btw_started_notify.notify_waiters();
            wait_for_flag(&self.state.btw_released, &self.state.btw_release_notify).await;
            return Ok(LlmResponse {
                content: Some("btw final".to_string()),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
                extra: Map::new(),
            });
        }

        if normalized.contains("tool-fail") {
            return Err(anyhow::anyhow!("btw tool failed"));
        }

        if normalized.contains("stale-generation") {
            wait_for_flag(&self.state.btw_ready, &self.state.btw_ready_notify).await;
        }

        if normalized.contains("profile-check")
            || normalized.contains("status?")
            || normalized.contains("stale-generation")
            || normalized.contains("history-check")
            || normalized.contains("inflight-check")
            || normalized.contains("second-btw")
        {
            self.state.btw_started.store(true, Ordering::SeqCst);
            self.state.btw_started_notify.notify_waiters();
        }

        let response = LlmResponse {
            content: Some(if content.contains("[Compressed user burst]") {
                content.clone()
            } else {
                "btw reply".to_string()
            }),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
            extra: Map::new(),
        };
        if normalized.contains("history-check") {
            self.state.btw_completed.store(true, Ordering::SeqCst);
            self.state.btw_completed_notify.notify_waiters();
        }
        Ok(response)
    }
}

#[async_trait]
impl LlmProvider for BtwTestProvider {
    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn chat(
        &self,
        messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        _model: &str,
    ) -> Result<LlmResponse> {
        self.reply(messages, None).await
    }

    async fn chat_with_request(
        &self,
        messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        request: &ProviderRequestDescriptor,
    ) -> Result<LlmResponse> {
        self.reply(messages, Some(request)).await
    }
}

#[derive(Default)]
struct SessionConcurrencyState {
    first_started: AtomicBool,
    first_released: AtomicBool,
    second_started: AtomicBool,
    first_started_notify: Notify,
    first_release_notify: Notify,
    second_started_notify: Notify,
}

#[derive(Clone)]
struct CrossSessionBusProvider {
    state: Arc<SessionConcurrencyState>,
}

#[async_trait]
impl LlmProvider for CrossSessionBusProvider {
    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn chat(
        &self,
        messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        _model: &str,
    ) -> Result<LlmResponse> {
        let user_content = messages
            .iter()
            .rev()
            .find(|message| message.get("role").and_then(|role| role.as_str()) == Some("user"))
            .and_then(|message| message.get("content").and_then(|content| content.as_str()))
            .unwrap_or_default();

        if user_content.ends_with("hold-a") {
            self.state.first_started.store(true, Ordering::SeqCst);
            self.state.first_started_notify.notify_waiters();
            while !self.state.first_released.load(Ordering::SeqCst) {
                self.state.first_release_notify.notified().await;
            }
            return Ok(LlmResponse {
                content: Some("session-a done".to_string()),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
                extra: Map::new(),
            });
        }

        if user_content.ends_with("fast-b") {
            self.state.second_started.store(true, Ordering::SeqCst);
            self.state.second_started_notify.notify_waiters();
            return Ok(LlmResponse {
                content: Some("session-b done".to_string()),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
                extra: Map::new(),
            });
        }

        Err(anyhow::anyhow!(
            "unexpected request shape for cross-session bus provider"
        ))
    }
}

#[derive(Clone)]
struct SameSessionDirectProvider {
    state: Arc<SessionConcurrencyState>,
}

#[async_trait]
impl LlmProvider for SameSessionDirectProvider {
    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn chat(
        &self,
        messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        _model: &str,
    ) -> Result<LlmResponse> {
        let user_content = messages
            .iter()
            .rev()
            .find(|message| message.get("role").and_then(|role| role.as_str()) == Some("user"))
            .and_then(|message| message.get("content").and_then(|content| content.as_str()))
            .unwrap_or_default();

        if user_content.ends_with("first") {
            self.state.first_started.store(true, Ordering::SeqCst);
            self.state.first_started_notify.notify_waiters();
            while !self.state.first_released.load(Ordering::SeqCst) {
                self.state.first_release_notify.notified().await;
            }
            return Ok(LlmResponse {
                content: Some("first done".to_string()),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
                extra: Map::new(),
            });
        }

        if user_content.ends_with("second") {
            self.state.second_started.store(true, Ordering::SeqCst);
            self.state.second_started_notify.notify_waiters();
            return Ok(LlmResponse {
                content: Some("second done".to_string()),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
                extra: Map::new(),
            });
        }

        Err(anyhow::anyhow!(
            "unexpected request shape for same-session direct provider"
        ))
    }
}

#[derive(Default)]
struct SystemSessionState {
    user_started: AtomicBool,
    user_released: AtomicBool,
    system_started: AtomicBool,
    system_released: AtomicBool,
    user_started_notify: Notify,
    user_release_notify: Notify,
    system_started_notify: Notify,
    system_release_notify: Notify,
}

#[derive(Clone)]
struct SystemSessionProvider {
    state: Arc<SystemSessionState>,
}

#[async_trait]
impl LlmProvider for SystemSessionProvider {
    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn chat(
        &self,
        messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        _model: &str,
    ) -> Result<LlmResponse> {
        let last_message = messages.last().cloned().unwrap_or_else(|| json!({}));
        let role = last_message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let content = last_message
            .get("content")
            .and_then(Value::as_str)
            .and_then(ContextBuilder::strip_runtime_prefix)
            .unwrap_or_default();

        match (role, content.as_str()) {
            ("user", "hold-user") => {
                self.state.user_started.store(true, Ordering::SeqCst);
                self.state.user_started_notify.notify_waiters();
                while !self.state.user_released.load(Ordering::SeqCst) {
                    self.state.user_release_notify.notified().await;
                }
                Ok(LlmResponse {
                    content: Some("user done".to_string()),
                    tool_calls: Vec::new(),
                    finish_reason: "stop".to_string(),
                    extra: Map::new(),
                })
            }
            ("assistant", "system-hold") => {
                self.state.system_started.store(true, Ordering::SeqCst);
                self.state.system_started_notify.notify_waiters();
                while !self.state.system_released.load(Ordering::SeqCst) {
                    self.state.system_release_notify.notified().await;
                }
                Ok(LlmResponse {
                    content: Some("system done".to_string()),
                    tool_calls: Vec::new(),
                    finish_reason: "stop".to_string(),
                    extra: Map::new(),
                })
            }
            _ => Err(anyhow::anyhow!(
                "unexpected request shape for system-session provider"
            )),
        }
    }
}

#[derive(Default)]
struct DebounceProviderState {
    contents: Arc<Mutex<Vec<String>>>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    block_started: AtomicBool,
    block_released: AtomicBool,
    block_started_notify: Notify,
    block_release_notify: Notify,
}

#[derive(Clone)]
struct DebounceRecordingProvider {
    state: Arc<DebounceProviderState>,
}

#[async_trait]
impl LlmProvider for DebounceRecordingProvider {
    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn chat(
        &self,
        _messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        _model: &str,
    ) -> Result<LlmResponse> {
        Err(anyhow::anyhow!(
            "chat() should not be used for debounce recording provider"
        ))
    }

    async fn chat_with_request(
        &self,
        messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        request: &ProviderRequestDescriptor,
    ) -> Result<LlmResponse> {
        let raw_user_content = messages
            .iter()
            .rev()
            .find(|message| message.get("role").and_then(|role| role.as_str()) == Some("user"))
            .and_then(|message| message.get("content").and_then(|content| content.as_str()))
            .unwrap_or_default();
        let user_content = ContextBuilder::strip_runtime_prefix(raw_user_content)
            .unwrap_or_else(|| raw_user_content.to_string());

        self.state.contents.lock().await.push(user_content.clone());
        self.state.requests.lock().await.push(RecordedRequest {
            provider: request.provider_name.clone(),
            model: request.model_name.clone(),
            extras: request.request_extras.clone(),
        });

        if user_content == "block" {
            self.state.block_started.store(true, Ordering::SeqCst);
            self.state.block_started_notify.notify_waiters();
            while !self.state.block_released.load(Ordering::SeqCst) {
                self.state.block_release_notify.notified().await;
            }
        }

        Ok(LlmResponse {
            content: Some(format!("processed: {user_content}")),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
            extra: Map::new(),
        })
    }
}

fn multi_profile_config(workspace: &std::path::Path) -> Config {
    let mut config = Config::default();
    config.agents.defaults.workspace = workspace.display().to_string();
    config.agents.defaults.default_profile = "openai:gpt-4.1-mini".to_string();
    config.agents.profiles = [
        (
            "openai:gpt-4.1-mini".to_string(),
            AgentProfileConfig {
                provider: "openai".to_string(),
                model: "gpt-4.1-mini".to_string(),
                request: [("temperature".to_string(), json!(0.3))]
                    .into_iter()
                    .collect(),
            },
        ),
        (
            "openrouter:deepseek-r1".to_string(),
            AgentProfileConfig {
                provider: "openrouter".to_string(),
                model: "deepseek/deepseek-r1".to_string(),
                request: [
                    ("temperature".to_string(), json!(0.1)),
                    ("reasoning".to_string(), json!({"enabled": true})),
                ]
                .into_iter()
                .collect(),
            },
        ),
    ]
    .into_iter()
    .collect();
    config
}

fn debounce_config(workspace: &std::path::Path, debounce_ms: u64) -> Config {
    let mut config = multi_profile_config(workspace);
    config.agents.defaults.message_debounce_ms = debounce_ms;
    config
}

fn inbound_message(
    channel: &str,
    chat_id: &str,
    session_key: &str,
    content: &str,
) -> InboundMessage {
    InboundMessage {
        channel: channel.to_string(),
        sender_id: "user".to_string(),
        chat_id: chat_id.to_string(),
        content: content.to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some(session_key.to_string()),
    }
}

fn spawn_runner(agent: AgentLoop) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move { agent.run().await })
}

async fn wait_for_flag(flag: &AtomicBool, notify: &Notify) {
    loop {
        if flag.load(Ordering::SeqCst) {
            return;
        }
        let notified = notify.notified();
        if flag.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::timeout(Duration::from_secs(1), notified)
            .await
            .expect("flag should be set");
    }
}

async fn wait_for_count(counter: &AtomicUsize, notify: &Notify, expected: usize) {
    loop {
        if counter.load(Ordering::SeqCst) >= expected {
            return;
        }
        let notified = notify.notified();
        if counter.load(Ordering::SeqCst) >= expected {
            return;
        }
        tokio::time::timeout(Duration::from_secs(1), notified)
            .await
            .expect("counter should reach expected value");
    }
}

#[tokio::test]
async fn agent_executes_tool_loop() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("note.txt"), "hello from file").expect("write note");
    let provider = mock_provider(vec![
        LlmResponse {
            content: Some("looking".to_string()),
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "read_file".to_string(),
                arguments: json!({"path": "note.txt"}),
            }],
            finish_reason: "tool_calls".to_string(),
            extra: Map::new(),
        },
        LlmResponse {
            content: Some("done".to_string()),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
            extra: Map::new(),
        },
    ]);
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus,
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    )
    .await
    .expect("agent");
    let result = agent
        .process_direct("read note", "cli:test", "cli", "test")
        .await
        .expect("process");
    assert_eq!(result, "done");
}

#[tokio::test]
async fn agent_process_direct_returns_message_tool_reply() {
    let dir = tempdir().expect("tempdir");
    let long_chinese =
        "请问您想查询哪个城市的天气？请提供城市名称或位置信息，这样我才能帮您查询天气情况。";
    let provider = mock_provider(vec![
        LlmResponse {
            content: Some("sending".to_string()),
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "message".to_string(),
                arguments: json!({"content": long_chinese}),
            }],
            finish_reason: "tool_calls".to_string(),
            extra: Map::new(),
        },
        LlmResponse {
            content: Some("done".to_string()),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
            extra: Map::new(),
        },
    ]);
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus.clone(),
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    )
    .await
    .expect("agent");
    let result = agent
        .process_direct("say hi", "cli:test", "cli", "test")
        .await
        .expect("process");
    assert_eq!(result, long_chinese);
    assert!(tokio::time::timeout(
        std::time::Duration::from_millis(100),
        bus.consume_outbound()
    )
    .await
    .is_err());
}

#[tokio::test]
async fn agent_bus_mode_suppresses_duplicate_final_reply_after_message_tool() {
    let dir = tempdir().expect("tempdir");
    let long_chinese =
        "请问您想查询哪个城市的天气？请提供城市名称或位置信息，这样我才能帮您查询天气情况。";
    let provider = mock_provider(vec![
        LlmResponse {
            content: Some("sending".to_string()),
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "message".to_string(),
                arguments: json!({"content": long_chinese}),
            }],
            finish_reason: "tool_calls".to_string(),
            extra: Map::new(),
        },
        LlmResponse {
            content: Some("done".to_string()),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
            extra: Map::new(),
        },
    ]);
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus.clone(),
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    )
    .await
    .expect("agent");
    let runner = {
        let agent = agent.clone();
        tokio::spawn(async move {
            agent.run().await;
        })
    };
    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "test".to_string(),
        content: "say hi".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:test".to_string()),
    })
    .await
    .expect("publish inbound");
    let outbound = loop {
        let outbound = bus.consume_outbound().await.expect("message tool outbound");
        let is_progress = outbound
            .metadata
            .get("_progress")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        if !is_progress {
            break outbound;
        }
    };
    assert_eq!(outbound.content, long_chinese);
    assert!(tokio::time::timeout(
        std::time::Duration::from_millis(100),
        bus.consume_outbound()
    )
    .await
    .is_err());
    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn agent_returns_iteration_limit_message() {
    let dir = tempdir().expect("tempdir");
    let provider = mock_provider(vec![
        LlmResponse {
            content: None,
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "list_dir".to_string(),
                arguments: json!({"path": "."}),
            }],
            finish_reason: "tool_calls".to_string(),
            extra: Map::new(),
        },
        LlmResponse {
            content: None,
            tool_calls: vec![ToolCall {
                id: "call_2".to_string(),
                name: "list_dir".to_string(),
                arguments: json!({"path": "."}),
            }],
            finish_reason: "tool_calls".to_string(),
            extra: Map::new(),
        },
    ]);
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus,
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        1,
        10,
        false,
        WebToolsConfig::default(),
    )
    .await
    .expect("agent");
    let result = agent
        .process_direct("loop", "cli:test", "cli", "test")
        .await
        .expect("process");
    assert!(result.contains("maximum number of tool call iterations (1)"));
}

#[tokio::test]
async fn subagent_reports_back_via_bus() {
    let dir = tempdir().expect("tempdir");
    let provider = mock_provider(vec![LlmResponse {
        content: Some("background result".to_string()),
        tool_calls: Vec::new(),
        finish_reason: "stop".to_string(),
        extra: Map::new(),
    }]);
    let bus = MessageBus::new(32);
    let manager = SubagentManager::new(
        provider,
        dir.path().to_path_buf(),
        bus.clone(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    );
    let status = manager
        .spawn(
            "do background work".to_string(),
            Some("job".to_string()),
            "cli".to_string(),
            "test".to_string(),
        )
        .await;
    assert!(status.contains("Subagent [job] started"));
    let inbound = tokio::time::timeout(std::time::Duration::from_secs(2), bus.consume_inbound())
        .await
        .expect("timely inbound")
        .expect("inbound message");
    assert_eq!(inbound.channel, "system");
    assert!(inbound.content.contains("background result"));
}

#[tokio::test]
async fn concurrent_direct_requests_do_not_share_tool_state() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(ConcurrentProviderState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(ConcurrentProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus,
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    )
    .await
    .expect("agent");

    let plain_agent = agent.clone();
    let plain_task = tokio::spawn(async move {
        plain_agent
            .process_direct("plain", "web:plain", "web", "plain")
            .await
    });

    while !state.plain_started.load(Ordering::SeqCst) {
        tokio::time::timeout(
            Duration::from_secs(1),
            state.plain_started_notify.notified(),
        )
        .await
        .expect("plain request should start")
    }

    let tool_agent = agent.clone();
    let tool_task = tokio::spawn(async move {
        tool_agent
            .process_direct("tool", "web:tool", "web", "tool")
            .await
    });

    let plain_result = plain_task.await.expect("plain join").expect("plain result");
    state.plain_finished.store(true, Ordering::SeqCst);
    state.plain_finished_notify.notify_waiters();
    let tool_result = tool_task.await.expect("tool join").expect("tool result");

    assert_eq!(plain_result, "plain final");
    assert_eq!(tool_result, "tool reply");
}

#[tokio::test]
async fn help_lists_model_commands() {
    let dir = tempdir().expect("tempdir");
    let bus = MessageBus::new(32);
    let provider = mock_provider(Vec::new());
    let agent = AgentLoop::from_config(bus, provider, multi_profile_config(dir.path()))
        .await
        .expect("agent");

    let result = agent
        .process_direct("/help", "cli:test", "cli", "test")
        .await
        .expect("help");

    assert!(result.contains("/models"));
    assert!(result.contains("/model <provider:model>"));
}

#[tokio::test]
async fn models_command_lists_profiles_and_marks_current_one() {
    let dir = tempdir().expect("tempdir");
    let bus = MessageBus::new(32);
    let provider = mock_provider(Vec::new());
    let agent = AgentLoop::from_config(bus, provider, multi_profile_config(dir.path()))
        .await
        .expect("agent");

    let result = agent
        .process_direct("/models", "cli:test", "cli", "test")
        .await
        .expect("models");

    assert!(result.contains("* openai:gpt-4.1-mini"));
    assert!(result.contains("openrouter:deepseek-r1"));
}

#[tokio::test]
async fn model_command_can_switch_a_session_to_a_codex_profile_and_use_the_codex_backend() {
    let dir = tempdir().expect("tempdir");
    let auth_file = dir.path().join("codex-auth.json");
    fs::write(&auth_file, valid_codex_auth_json()).expect("write auth file");
    let (addr, requests) = start_codex_capture_server(vec![(
        StatusCode::OK,
        codex_sse_body(vec![
            (
                "response.output_text.delta",
                json!({"type": "response.output_text.delta", "delta": "codex "}),
            ),
            (
                "response.output_text.delta",
                json!({"type": "response.output_text.delta", "delta": "reply"}),
            ),
            (
                "response.output_text.done",
                json!({"type": "response.output_text.done", "text": "codex reply"}),
            ),
            ("response.completed", response_completed()),
        ]),
    )])
    .await;
    let config = codex_agent_config(
        dir.path(),
        &auth_file,
        format!("http://{addr}/backend-api/codex"),
    );
    let provider: Arc<dyn LlmProvider> = Arc::new(ProviderPool::new(config.clone()));
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus, provider, config)
        .await
        .expect("agent");

    let switched = agent
        .process_direct("/model codex:gpt-5.4", "cli:codex", "cli", "codex")
        .await
        .expect("switch");
    assert!(switched.contains("codex:gpt-5.4"), "{switched}");
    assert_eq!(
        agent
            .current_profile_for_session("cli:codex")
            .expect("profile"),
        "codex:gpt-5.4"
    );

    let reply = agent
        .process_direct("hello", "cli:codex", "cli", "codex")
        .await
        .expect("reply");
    assert_eq!(reply, "codex reply");

    let requests = requests.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/backend-api/codex/responses");
    assert_eq!(
        requests[0].authorization.as_deref(),
        Some("Bearer access-token")
    );
    assert_eq!(requests[0].account_id.as_deref(), Some("account-id"));
    assert!(requests[0]
        .accept
        .as_deref()
        .is_some_and(|value| value.contains("text/event-stream")));
    assert_eq!(
        requests[0].body.get("model").and_then(Value::as_str),
        Some("gpt-5.4")
    );
    assert_eq!(requests[0].body.get("stream"), Some(&json!(true)));
}

#[tokio::test]
async fn codex_default_profile_fails_without_falling_back_to_openai() {
    let dir = tempdir().expect("tempdir");
    let missing_auth_file = dir.path().join("missing-codex-auth.json");
    let config = codex_agent_config(
        dir.path(),
        &missing_auth_file,
        "https://chatgpt.com/backend-api/codex".to_string(),
    );
    let mut config = config;
    config.agents.defaults.default_profile = "codex:gpt-5.4".to_string();

    let provider: Arc<dyn LlmProvider> = Arc::new(ProviderPool::new(config.clone()));
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus, provider, config)
        .await
        .expect("agent");

    let err = agent
        .process_direct("hello", "cli:codex", "cli", "codex")
        .await
        .expect_err("missing auth file should fail");

    assert!(err.to_string().contains("auth file"), "{err}");
}

#[tokio::test]
async fn codex_profile_runs_a_tool_call_second_round_and_sends_tool_results_back() {
    let dir = tempdir().expect("tempdir");
    let auth_file = dir.path().join("codex-auth.json");
    fs::write(&auth_file, valid_codex_auth_json()).expect("write auth file");
    fs::write(dir.path().join("sample.txt"), "hello from workspace").expect("write sample");

    let (addr, requests) = start_codex_capture_server(vec![
        (
            StatusCode::OK,
            codex_sse_body(vec![
                (
                    "response.output_item.done",
                    response_output_item_done(json!({
                        "type": "function_call",
                        "call_id": "call_1",
                        "id": "call_1",
                        "name": "list_dir",
                        "status": "completed"
                    })),
                ),
                (
                    "response.function_call_arguments.done",
                    response_function_call_arguments_done(
                        "call_1",
                        &json!({"path": dir.path().display().to_string()}).to_string(),
                    ),
                ),
                ("response.completed", response_completed()),
            ]),
        ),
        (
            StatusCode::OK,
            codex_sse_body(vec![
                (
                    "response.output_text.delta",
                    json!({"type": "response.output_text.delta", "delta": "final codex "}),
                ),
                (
                    "response.output_text.delta",
                    json!({"type": "response.output_text.delta", "delta": "answer"}),
                ),
                (
                    "response.output_text.done",
                    json!({"type": "response.output_text.done", "text": "final codex answer"}),
                ),
                ("response.completed", response_completed()),
            ]),
        ),
    ])
    .await;

    let config = codex_agent_config(
        dir.path(),
        &auth_file,
        format!("http://{addr}/backend-api/codex"),
    );
    let provider: Arc<dyn LlmProvider> = Arc::new(ProviderPool::new(config.clone()));
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus, provider, config)
        .await
        .expect("agent");

    let switched = agent
        .process_direct(
            "/model codex:gpt-5.4",
            "cli:codex-tool",
            "cli",
            "codex-tool",
        )
        .await
        .expect("switch");
    assert!(switched.contains("codex:gpt-5.4"), "{switched}");

    let reply = agent
        .process_direct("list the workspace", "cli:codex-tool", "cli", "codex-tool")
        .await
        .expect("reply");
    assert_eq!(reply, "final codex answer");

    let requests = requests.lock().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].path, "/backend-api/codex/responses");
    assert_eq!(requests[1].path, "/backend-api/codex/responses");
    assert_eq!(
        requests[1].body.get("model").and_then(Value::as_str),
        Some("gpt-5.4")
    );
    assert!(requests[1]
        .accept
        .as_deref()
        .is_some_and(|value| value.contains("text/event-stream")));

    let second_input = requests[1]
        .body
        .get("input")
        .and_then(Value::as_array)
        .expect("second request input");
    let function_call = second_input
        .iter()
        .find(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        .expect("function_call item");
    assert_eq!(
        function_call.get("call_id").and_then(Value::as_str),
        Some("call_1")
    );
    assert_eq!(
        function_call.get("name").and_then(Value::as_str),
        Some("list_dir")
    );

    let tool_output = second_input
        .iter()
        .find(|item| item.get("type").and_then(Value::as_str) == Some("function_call_output"))
        .expect("function_call_output item");
    assert_eq!(
        tool_output.get("call_id").and_then(Value::as_str),
        Some("call_1")
    );
    let tool_text = tool_output
        .get("output")
        .and_then(Value::as_str)
        .expect("tool result text");
    assert!(tool_text.contains("sample.txt"), "{tool_text}");
}

#[tokio::test]
async fn model_command_switches_only_the_current_session() {
    let dir = tempdir().expect("tempdir");
    let records = Arc::new(Mutex::new(Vec::new()));
    let provider: Arc<dyn LlmProvider> = Arc::new(RequestRecordingProvider {
        records: records.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus, provider, multi_profile_config(dir.path()))
        .await
        .expect("agent");

    let switched = agent
        .process_direct("/model openrouter:deepseek-r1", "cli:one", "cli", "one")
        .await
        .expect("switch");
    assert!(switched.contains("openrouter:deepseek-r1"));

    agent
        .process_direct("hello", "cli:one", "cli", "one")
        .await
        .expect("session one");
    agent
        .process_direct("hello", "cli:two", "cli", "two")
        .await
        .expect("session two");

    let records = records.lock().await;
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].provider, "openrouter");
    assert_eq!(records[0].model, "deepseek/deepseek-r1");
    assert_eq!(records[0].extras.get("temperature"), Some(&json!(0.1)));
    assert_eq!(
        records[0].extras.get("reasoning"),
        Some(&json!({"enabled": true}))
    );
    assert_eq!(records[1].provider, "openai");
    assert_eq!(records[1].model, "gpt-4.1-mini");
    assert_eq!(records[1].extras.get("temperature"), Some(&json!(0.3)));
}

#[tokio::test]
async fn new_resets_the_session_profile_to_default() {
    let dir = tempdir().expect("tempdir");
    let records = Arc::new(Mutex::new(Vec::new()));
    let provider: Arc<dyn LlmProvider> = Arc::new(RequestRecordingProvider {
        records: records.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus, provider, multi_profile_config(dir.path()))
        .await
        .expect("agent");

    agent
        .process_direct("/model openrouter:deepseek-r1", "cli:one", "cli", "one")
        .await
        .expect("switch");
    agent
        .process_direct("/new", "cli:one", "cli", "one")
        .await
        .expect("new");
    agent
        .process_direct("hello", "cli:one", "cli", "one")
        .await
        .expect("message");

    let records = records.lock().await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].provider, "openai");
    assert_eq!(records[0].model, "gpt-4.1-mini");
}

#[tokio::test]
async fn system_turn_uses_the_target_sessions_active_profile() {
    let dir = tempdir().expect("tempdir");
    let records = Arc::new(Mutex::new(Vec::new()));
    let provider: Arc<dyn LlmProvider> = Arc::new(RequestRecordingProvider {
        records: records.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, multi_profile_config(dir.path()))
        .await
        .expect("agent");
    let runner = {
        let agent = agent.clone();
        tokio::spawn(async move { agent.run().await })
    };

    agent
        .process_direct("/model openrouter:deepseek-r1", "cli:one", "cli", "one")
        .await
        .expect("switch");

    bus.publish_inbound(InboundMessage {
        channel: "system".to_string(),
        sender_id: "subagent".to_string(),
        chat_id: "cli:one".to_string(),
        content: "background".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: None,
    })
    .await
    .expect("publish");

    let outbound = tokio::time::timeout(Duration::from_secs(2), bus.consume_outbound())
        .await
        .expect("timely")
        .expect("outbound");
    assert_eq!(outbound.content, "ok");

    let records = records.lock().await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].provider, "openrouter");
    assert_eq!(records[0].model, "deepseek/deepseek-r1");

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn agent_preserves_assistant_extra_fields_across_tool_continuation() {
    let dir = tempdir().expect("tempdir");
    let bus = MessageBus::new(32);
    let provider: Arc<dyn LlmProvider> = Arc::new(ReplayAwareProvider {
        call_count: Arc::new(Mutex::new(0)),
    });
    let agent = AgentLoop::from_config(bus, provider, multi_profile_config(dir.path()))
        .await
        .expect("agent");

    let result = agent
        .process_direct("inspect", "cli:test", "cli", "test")
        .await
        .expect("process");

    assert_eq!(result, "done");
}

#[tokio::test]
async fn session_burst_messages_are_merged_into_one_turn() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(DebounceProviderState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(DebounceRecordingProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, debounce_config(dir.path(), 50))
        .await
        .expect("agent");
    let runner = {
        let agent = agent.clone();
        tokio::spawn(async move { agent.run().await })
    };

    for content in ["first", "second", "third"] {
        bus.publish_inbound(InboundMessage {
            channel: "cli".to_string(),
            sender_id: "user".to_string(),
            chat_id: "burst".to_string(),
            content: content.to_string(),
            timestamp: chrono::Utc::now(),
            metadata: Default::default(),
            session_key_override: Some("cli:burst".to_string()),
        })
        .await
        .expect("publish burst");
    }

    let outbound = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
        .await
        .expect("timely outbound")
        .expect("outbound");
    assert!(outbound.content.contains("[Compressed user burst]"));

    let contents = state.contents.lock().await.clone();
    assert_eq!(contents.len(), 1);
    assert!(contents[0].contains("[Compressed user burst]"));
    assert!(contents[0].contains("1. first"));
    assert!(contents[0].contains("2. second"));
    assert!(contents[0].contains("3. third"));

    let session = agent
        .load_session_by_key("cli:burst")
        .expect("load session")
        .expect("session exists");
    let user_turns = session
        .messages
        .iter()
        .filter(|message| message.role == "user")
        .collect::<Vec<_>>();
    assert_eq!(user_turns.len(), 1);
    assert!(user_turns[0]
        .content
        .as_str()
        .is_some_and(|value| value.contains("[Compressed user burst]")));

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn messages_outside_the_debounce_window_are_processed_separately() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(DebounceProviderState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(DebounceRecordingProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, debounce_config(dir.path(), 40))
        .await
        .expect("agent");
    let runner = {
        let agent = agent.clone();
        tokio::spawn(async move { agent.run().await })
    };

    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "split".to_string(),
        content: "one".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:split".to_string()),
    })
    .await
    .expect("publish first");

    tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
        .await
        .expect("timely first outbound")
        .expect("first outbound");

    tokio::time::sleep(Duration::from_millis(80)).await;

    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "split".to_string(),
        content: "two".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:split".to_string()),
    })
    .await
    .expect("publish second");

    tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
        .await
        .expect("timely second outbound")
        .expect("second outbound");

    let contents = state.contents.lock().await.clone();
    assert_eq!(contents, vec!["one".to_string(), "two".to_string()]);

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn models_command_bypasses_pending_debounce() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(DebounceProviderState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(DebounceRecordingProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, debounce_config(dir.path(), 200))
        .await
        .expect("agent");
    let runner = {
        let agent = agent.clone();
        tokio::spawn(async move { agent.run().await })
    };

    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "models".to_string(),
        content: "hello".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:models".to_string()),
    })
    .await
    .expect("publish normal");
    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "models".to_string(),
        content: "/models".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:models".to_string()),
    })
    .await
    .expect("publish command");

    let first = tokio::time::timeout(Duration::from_millis(100), bus.consume_outbound())
        .await
        .expect("models should bypass debounce")
        .expect("first outbound");
    assert!(
        first.content.contains("Available model profiles:"),
        "{:?}",
        first.content
    );

    let second = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
        .await
        .expect("debounced burst should still flush")
        .expect("second outbound");
    assert_eq!(second.content, "processed: hello");
    assert_eq!(
        state.contents.lock().await.clone(),
        vec!["hello".to_string()]
    );

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn help_command_bypasses_pending_debounce() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(DebounceProviderState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(DebounceRecordingProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, debounce_config(dir.path(), 200))
        .await
        .expect("agent");
    let runner = {
        let agent = agent.clone();
        tokio::spawn(async move { agent.run().await })
    };

    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "help".to_string(),
        content: "hello".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:help".to_string()),
    })
    .await
    .expect("publish normal");
    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "help".to_string(),
        content: "/help".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:help".to_string()),
    })
    .await
    .expect("publish help");

    let first = tokio::time::timeout(Duration::from_millis(100), bus.consume_outbound())
        .await
        .expect("help should bypass debounce")
        .expect("first outbound");
    assert!(
        first.content.contains("nanobot-rs commands:"),
        "{}",
        first.content
    );

    let second = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
        .await
        .expect("debounced burst should flush")
        .expect("second outbound");
    assert_eq!(second.content, "processed: hello");
    assert_eq!(
        state.contents.lock().await.clone(),
        vec!["hello".to_string()]
    );

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn model_command_bypasses_pending_debounce_and_updates_the_flushed_burst_profile() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(DebounceProviderState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(DebounceRecordingProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, debounce_config(dir.path(), 200))
        .await
        .expect("agent");
    let runner = {
        let agent = agent.clone();
        tokio::spawn(async move { agent.run().await })
    };

    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "model".to_string(),
        content: "hello".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:model".to_string()),
    })
    .await
    .expect("publish normal");
    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "model".to_string(),
        content: "/model openrouter:deepseek-r1".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:model".to_string()),
    })
    .await
    .expect("publish model command");

    let first = tokio::time::timeout(Duration::from_millis(100), bus.consume_outbound())
        .await
        .expect("model should bypass debounce")
        .expect("first outbound");
    assert!(
        first
            .content
            .contains("Switched this session to openrouter:deepseek-r1."),
        "{}",
        first.content
    );

    tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
        .await
        .expect("debounced burst should flush")
        .expect("second outbound");

    let requests = state.requests.lock().await.clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].provider, "openrouter");
    assert_eq!(requests[0].model, "deepseek/deepseek-r1");

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn new_clears_any_pending_burst_before_resetting_the_session() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(DebounceProviderState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(DebounceRecordingProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, debounce_config(dir.path(), 200))
        .await
        .expect("agent");
    let runner = {
        let agent = agent.clone();
        tokio::spawn(async move { agent.run().await })
    };

    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "new".to_string(),
        content: "hello".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:new".to_string()),
    })
    .await
    .expect("publish normal");
    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "new".to_string(),
        content: "/new".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:new".to_string()),
    })
    .await
    .expect("publish new");

    let first = tokio::time::timeout(Duration::from_millis(100), bus.consume_outbound())
        .await
        .expect("new should bypass debounce")
        .expect("first outbound");
    assert_eq!(first.content, "New session started.");

    assert!(
        tokio::time::timeout(Duration::from_millis(300), bus.consume_outbound())
            .await
            .is_err(),
        "pending burst should be cleared by /new"
    );
    assert!(state.contents.lock().await.is_empty());

    let session = agent
        .load_session_by_key("cli:new")
        .expect("load session")
        .expect("session exists");
    assert!(session.messages.is_empty());

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn stop_bypasses_debounce_cancels_the_running_task_and_clears_pending_burst() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(DebounceProviderState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(DebounceRecordingProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, debounce_config(dir.path(), 20))
        .await
        .expect("agent");
    let runner = {
        let agent = agent.clone();
        tokio::spawn(async move { agent.run().await })
    };

    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "stop".to_string(),
        content: "block".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:stop".to_string()),
    })
    .await
    .expect("publish blocking");

    while !state.block_started.load(Ordering::SeqCst) {
        tokio::time::timeout(
            Duration::from_secs(1),
            state.block_started_notify.notified(),
        )
        .await
        .expect("blocking task should start");
    }

    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "stop".to_string(),
        content: "later".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:stop".to_string()),
    })
    .await
    .expect("publish buffered");
    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "stop".to_string(),
        content: "/stop".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:stop".to_string()),
    })
    .await
    .expect("publish stop");

    let outbound = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
        .await
        .expect("stop should produce outbound")
        .expect("outbound");
    assert!(
        outbound.content.starts_with("Stopped "),
        "{}",
        outbound.content
    );

    assert!(
        tokio::time::timeout(Duration::from_millis(300), bus.consume_outbound())
            .await
            .is_err(),
        "pending burst should be cleared by /stop"
    );
    assert_eq!(
        state.contents.lock().await.clone(),
        vec!["block".to_string()]
    );

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn bursts_are_never_merged_across_different_sessions_in_the_same_channel() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(DebounceProviderState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(DebounceRecordingProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, debounce_config(dir.path(), 50))
        .await
        .expect("agent");
    let runner = {
        let agent = agent.clone();
        tokio::spawn(async move { agent.run().await })
    };

    for (session_key, chat_id, content) in [
        ("telegram:chat-a", "chat-a", "alpha-1"),
        ("telegram:chat-b", "chat-b", "beta-1"),
        ("telegram:chat-a", "chat-a", "alpha-2"),
        ("telegram:chat-b", "chat-b", "beta-2"),
    ] {
        bus.publish_inbound(InboundMessage {
            channel: "telegram".to_string(),
            sender_id: "user".to_string(),
            chat_id: chat_id.to_string(),
            content: content.to_string(),
            timestamp: chrono::Utc::now(),
            metadata: Default::default(),
            session_key_override: Some(session_key.to_string()),
        })
        .await
        .expect("publish");
    }

    for _ in 0..2 {
        tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
            .await
            .expect("timely outbound")
            .expect("outbound");
    }

    let mut contents = state.contents.lock().await.clone();
    contents.sort();
    assert_eq!(
        contents,
        vec![
            "[Compressed user burst]\n1. alpha-1\n2. alpha-2".to_string(),
            "[Compressed user burst]\n1. beta-1\n2. beta-2".to_string()
        ]
    );

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn bus_requests_for_different_sessions_can_complete_in_parallel() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(SessionConcurrencyState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(CrossSessionBusProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus.clone(),
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    )
    .await
    .expect("agent");
    let runner = {
        let agent = agent.clone();
        tokio::spawn(async move { agent.run().await })
    };

    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "a".to_string(),
        content: "hold-a".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:a".to_string()),
    })
    .await
    .expect("publish a");

    while !state.first_started.load(Ordering::SeqCst) {
        tokio::time::timeout(
            Duration::from_secs(1),
            state.first_started_notify.notified(),
        )
        .await
        .expect("first session should start");
    }

    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "b".to_string(),
        content: "fast-b".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:b".to_string()),
    })
    .await
    .expect("publish b");

    let early_outbound = tokio::time::timeout(Duration::from_millis(200), bus.consume_outbound())
        .await
        .ok()
        .flatten();

    state.first_released.store(true, Ordering::SeqCst);
    state.first_release_notify.notify_waiters();

    let mut contents = Vec::new();
    if let Some(outbound) = early_outbound {
        contents.push(outbound.content);
    }
    while contents.len() < 2 {
        let outbound = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
            .await
            .expect("timely outbound")
            .expect("outbound");
        contents.push(outbound.content);
    }

    assert_eq!(contents.first().map(String::as_str), Some("session-b done"));
    assert!(contents.iter().any(|content| content == "session-a done"));

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn system_messages_share_the_same_session_lock_as_user_messages() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(SystemSessionState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(SystemSessionProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus.clone(),
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    )
    .await
    .expect("agent");
    let runner = {
        let agent = agent.clone();
        tokio::spawn(async move { agent.run().await })
    };

    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "shared".to_string(),
        content: "hold-user".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:shared".to_string()),
    })
    .await
    .expect("publish user");

    while !state.user_started.load(Ordering::SeqCst) {
        tokio::time::timeout(Duration::from_secs(1), state.user_started_notify.notified())
            .await
            .expect("user task should start");
    }

    bus.publish_inbound(InboundMessage {
        channel: "system".to_string(),
        sender_id: "subagent".to_string(),
        chat_id: "cli:shared".to_string(),
        content: "system-hold".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: None,
    })
    .await
    .expect("publish system");

    let system_started_early = tokio::time::timeout(Duration::from_millis(200), async {
        while !state.system_started.load(Ordering::SeqCst) {
            state.system_started_notify.notified().await;
        }
    })
    .await
    .is_ok();

    state.user_released.store(true, Ordering::SeqCst);
    state.user_release_notify.notify_waiters();

    while !state.system_started.load(Ordering::SeqCst) {
        tokio::time::timeout(
            Duration::from_secs(1),
            state.system_started_notify.notified(),
        )
        .await
        .expect("system task should start after user release");
    }

    state.system_released.store(true, Ordering::SeqCst);
    state.system_release_notify.notify_waiters();

    let mut contents = Vec::new();
    while contents.len() < 2 {
        let outbound = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
            .await
            .expect("timely outbound")
            .expect("outbound");
        contents.push(outbound.content);
    }

    assert!(
        !system_started_early,
        "system message overlapped its target user session"
    );
    assert!(contents.iter().any(|content| content == "user done"));
    assert!(contents.iter().any(|content| content == "system done"));

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn stop_cancels_system_work_for_the_target_session() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(SystemSessionState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(SystemSessionProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus.clone(),
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    )
    .await
    .expect("agent");
    let runner = {
        let agent = agent.clone();
        tokio::spawn(async move { agent.run().await })
    };

    bus.publish_inbound(InboundMessage {
        channel: "system".to_string(),
        sender_id: "subagent".to_string(),
        chat_id: "cli:shared".to_string(),
        content: "system-hold".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: None,
    })
    .await
    .expect("publish system");

    while !state.system_started.load(Ordering::SeqCst) {
        tokio::time::timeout(
            Duration::from_secs(1),
            state.system_started_notify.notified(),
        )
        .await
        .expect("system task should start");
    }

    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "shared".to_string(),
        content: "/stop".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:shared".to_string()),
    })
    .await
    .expect("publish stop");

    let outbound = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
        .await
        .expect("stop should produce outbound")
        .expect("outbound");
    assert!(
        outbound.content.starts_with("Stopped "),
        "{}",
        outbound.content
    );

    assert!(
        tokio::time::timeout(Duration::from_millis(300), bus.consume_outbound())
            .await
            .is_err(),
        "stopped system task should not emit a completion"
    );

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn direct_requests_for_the_same_session_remain_serialized() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(SessionConcurrencyState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(SameSessionDirectProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus,
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    )
    .await
    .expect("agent");

    let first_agent = agent.clone();
    let first_task = tokio::spawn(async move {
        first_agent
            .process_direct("first", "cli:shared", "cli", "shared")
            .await
    });

    while !state.first_started.load(Ordering::SeqCst) {
        tokio::time::timeout(
            Duration::from_secs(1),
            state.first_started_notify.notified(),
        )
        .await
        .expect("first direct request should start");
    }

    let second_agent = agent.clone();
    let second_task = tokio::spawn(async move {
        second_agent
            .process_direct("second", "cli:shared", "cli", "shared")
            .await
    });

    let second_started_early = tokio::time::timeout(Duration::from_millis(200), async {
        while !state.second_started.load(Ordering::SeqCst) {
            state.second_started_notify.notified().await;
        }
    })
    .await
    .is_ok();

    state.first_released.store(true, Ordering::SeqCst);
    state.first_release_notify.notify_waiters();

    let first_result = first_task.await.expect("first join").expect("first result");
    let second_result = second_task
        .await
        .expect("second join")
        .expect("second result");

    assert!(
        !second_started_early,
        "second same-session request overlapped the first"
    );
    assert_eq!(first_result, "first done");
    assert_eq!(second_result, "second done");
}

#[tokio::test]
async fn direct_requests_for_different_sessions_can_run_in_parallel() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(SessionConcurrencyState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(CrossSessionBusProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus,
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    )
    .await
    .expect("agent");

    let first_agent = agent.clone();
    let first_task = tokio::spawn(async move {
        first_agent
            .process_direct("hold-a", "web:a", "web", "a")
            .await
    });

    while !state.first_started.load(Ordering::SeqCst) {
        tokio::time::timeout(
            Duration::from_secs(1),
            state.first_started_notify.notified(),
        )
        .await
        .expect("first direct session should start");
    }

    let second_agent = agent.clone();
    let second_task = tokio::spawn(async move {
        second_agent
            .process_direct("fast-b", "web:b", "web", "b")
            .await
    });

    let second_result = tokio::time::timeout(Duration::from_millis(200), second_task)
        .await
        .expect("second direct session should not be blocked")
        .expect("second join")
        .expect("second result");

    state.first_released.store(true, Ordering::SeqCst);
    state.first_release_notify.notify_waiters();

    let first_result = first_task.await.expect("first join").expect("first result");

    assert_eq!(second_result, "session-b done");
    assert_eq!(first_result, "session-a done");
}

#[tokio::test]
async fn btw_replies_while_main_lane_keeps_running() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(BtwState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(BtwTestProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, multi_profile_config(dir.path()))
        .await
        .expect("agent");
    let runner = spawn_runner(agent.clone());

    bus.publish_inbound(inbound_message("cli", "btw", "cli:btw", "main-hold"))
        .await
        .expect("publish main");
    wait_for_flag(&state.main_started, &state.main_started_notify).await;

    bus.publish_inbound(inbound_message("cli", "btw", "cli:btw", "/btw status?"))
        .await
        .expect("publish btw");

    let btw_outbound = tokio::time::timeout(Duration::from_millis(200), bus.consume_outbound())
        .await
        .expect("btw reply should arrive before the main turn completes")
        .expect("btw outbound");
    assert_eq!(btw_outbound.content, "btw reply");

    state.main_released.store(true, Ordering::SeqCst);
    state.main_release_notify.notify_waiters();
    let main_outbound = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
        .await
        .expect("main outbound")
        .expect("main outbound");
    assert_eq!(main_outbound.content, "main final");

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn btw_persists_to_timeline_but_not_model_history() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(BtwState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(BtwTestProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, multi_profile_config(dir.path()))
        .await
        .expect("agent");
    let runner = spawn_runner(agent.clone());

    bus.publish_inbound(inbound_message(
        "cli",
        "history",
        "cli:history",
        "main-hold",
    ))
    .await
    .expect("publish main");
    wait_for_flag(&state.main_started, &state.main_started_notify).await;

    bus.publish_inbound(inbound_message(
        "cli",
        "history",
        "cli:history",
        "/btw history-check",
    ))
    .await
    .expect("publish btw");

    state.main_released.store(true, Ordering::SeqCst);
    state.main_release_notify.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
        .await
        .expect("main outbound")
        .expect("main outbound");

    wait_for_flag(&state.btw_completed, &state.btw_completed_notify).await;

    let session = agent
        .load_session_by_key("cli:history")
        .expect("load session")
        .expect("session exists");
    let turn_texts = session
        .messages
        .iter()
        .map(|message| {
            message
                .content
                .as_str()
                .map(ToString::to_string)
                .unwrap_or_else(|| message.content.to_string())
        })
        .collect::<Vec<_>>();
    assert!(
        turn_texts
            .iter()
            .any(|content| content.contains("history-check")),
        "{turn_texts:?}"
    );
    assert!(
        turn_texts
            .iter()
            .all(|content| !content.contains("/btw history-check")),
        "{turn_texts:?}"
    );
    assert!(
        turn_texts
            .iter()
            .any(|content| content.contains("btw reply")),
        "{turn_texts:?}"
    );
    let history_texts = session
        .get_history(100)
        .into_iter()
        .map(|message| {
            message
                .get("content")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .unwrap_or_else(|| message["content"].to_string())
        })
        .collect::<Vec<_>>();
    assert!(
        history_texts
            .iter()
            .all(|content| !content.contains("/btw history-check")),
        "{history_texts:?}"
    );
    assert!(
        history_texts
            .iter()
            .all(|content| !content.contains("btw reply")),
        "{history_texts:?}"
    );

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn btw_requires_an_active_main_task() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(BtwState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(BtwTestProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, multi_profile_config(dir.path()))
        .await
        .expect("agent");
    let runner = spawn_runner(agent.clone());

    bus.publish_inbound(inbound_message(
        "cli",
        "inactive",
        "cli:inactive",
        "/btw hello",
    ))
    .await
    .expect("publish btw");

    let outbound = tokio::time::timeout(Duration::from_millis(200), bus.consume_outbound())
        .await
        .expect("btw should return a user-visible rejection when no main task is active")
        .expect("outbound");
    assert!(
        outbound.content.contains("active main task") || outbound.content.contains("running main"),
        "{}",
        outbound.content
    );

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn btw_uses_the_session_active_profile_snapshot() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(BtwState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(BtwTestProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, multi_profile_config(dir.path()))
        .await
        .expect("agent");
    let runner = spawn_runner(agent.clone());

    agent
        .process_direct(
            "/model openrouter:deepseek-r1",
            "cli:profile",
            "cli",
            "profile",
        )
        .await
        .expect("switch profile");
    assert_eq!(
        agent
            .current_profile_for_session("cli:profile")
            .expect("profile"),
        "openrouter:deepseek-r1"
    );

    bus.publish_inbound(inbound_message(
        "cli",
        "profile",
        "cli:profile",
        "main-hold",
    ))
    .await
    .expect("publish main");
    wait_for_flag(&state.main_started, &state.main_started_notify).await;

    bus.publish_inbound(inbound_message(
        "cli",
        "profile",
        "cli:profile",
        "/btw profile-check",
    ))
    .await
    .expect("publish btw");

    wait_for_flag(&state.btw_started, &state.btw_started_notify).await;
    let records = state.records.lock().await.clone();
    assert!(records.len() >= 2, "{records:?}");
    let btw_request = records.last().expect("btw request");
    assert_eq!(btw_request.provider, "openrouter");
    assert_eq!(btw_request.model, "deepseek/deepseek-r1");
    assert_eq!(btw_request.extras.get("temperature"), Some(&json!(0.1)));
    assert_eq!(
        btw_request.extras.get("reasoning"),
        Some(&json!({"enabled": true}))
    );

    state.main_released.store(true, Ordering::SeqCst);
    state.main_release_notify.notify_waiters();
    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn btw_rejects_when_the_bound_main_generation_changes_before_start() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(BtwState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(BtwTestProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, multi_profile_config(dir.path()))
        .await
        .expect("agent");
    let runner = spawn_runner(agent.clone());

    bus.publish_inbound(inbound_message("cli", "stale", "cli:stale", "main-hold"))
        .await
        .expect("publish main");
    wait_for_flag(&state.main_started, &state.main_started_notify).await;

    bus.publish_inbound(inbound_message(
        "cli",
        "stale",
        "cli:stale",
        "/btw stale-generation",
    ))
    .await
    .expect("publish btw");

    bus.publish_inbound(inbound_message("cli", "stale", "cli:stale", "/stop"))
        .await
        .expect("publish stop for first generation");
    let first_outbound = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
        .await
        .expect("stop should cancel the first generation")
        .expect("stop outbound");
    let mut saw_stale = first_outbound.content.contains("stale")
        || first_outbound.content.contains("generation");

    state.main_released.store(true, Ordering::SeqCst);
    state.main_release_notify.notify_waiters();
    state.main_released.store(false, Ordering::SeqCst);
    bus.publish_inbound(inbound_message("cli", "stale", "cli:stale", "main-hold"))
        .await
        .expect("publish second main");
    tokio::time::timeout(
        Duration::from_secs(1),
        wait_for_count(&state.main_start_count, &state.main_start_count_notify, 2),
    )
    .await
    .expect("second generation should start before BTW is allowed to continue");
    state.btw_ready.store(true, Ordering::SeqCst);
    state.btw_ready_notify.notify_waiters();

    if !saw_stale {
        let outbound = tokio::time::timeout(Duration::from_millis(200), bus.consume_outbound())
            .await
            .expect("btw should reject stale generation changes")
            .expect("outbound");
        saw_stale = outbound.content.contains("stale") || outbound.content.contains("generation");
        assert!(saw_stale, "{}", outbound.content);
    }

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn btw_rejects_a_second_active_btw_for_the_same_session() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(BtwState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(BtwTestProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, multi_profile_config(dir.path()))
        .await
        .expect("agent");
    let runner = spawn_runner(agent.clone());

    bus.publish_inbound(inbound_message("cli", "slot", "cli:slot", "main-hold"))
        .await
        .expect("publish main");
    wait_for_flag(&state.main_started, &state.main_started_notify).await;

    bus.publish_inbound(inbound_message("cli", "slot", "cli:slot", "/btw hold-btw"))
        .await
        .expect("publish first btw");
    wait_for_flag(&state.btw_started, &state.btw_started_notify).await;

    bus.publish_inbound(inbound_message(
        "cli",
        "slot",
        "cli:slot",
        "/btw second-btw",
    ))
    .await
    .expect("publish second btw");

    let outbound = tokio::time::timeout(Duration::from_millis(200), bus.consume_outbound())
        .await
        .expect("second btw should be rejected immediately")
        .expect("outbound");
    assert!(
        outbound.content.contains("already") || outbound.content.contains("another"),
        "{}",
        outbound.content
    );

    state.btw_released.store(true, Ordering::SeqCst);
    state.btw_release_notify.notify_waiters();
    state.main_released.store(true, Ordering::SeqCst);
    state.main_release_notify.notify_waiters();
    let _ = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound()).await;

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn stop_cancels_active_btw_tasks_for_the_session() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(BtwState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(BtwTestProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, multi_profile_config(dir.path()))
        .await
        .expect("agent");
    let runner = spawn_runner(agent.clone());

    bus.publish_inbound(inbound_message("cli", "stop", "cli:stop", "main-hold"))
        .await
        .expect("publish main");
    wait_for_flag(&state.main_started, &state.main_started_notify).await;

    bus.publish_inbound(inbound_message("cli", "stop", "cli:stop", "/btw hold-btw"))
        .await
        .expect("publish btw");
    wait_for_flag(&state.btw_started, &state.btw_started_notify).await;

    bus.publish_inbound(inbound_message("cli", "stop", "cli:stop", "/stop"))
        .await
        .expect("publish stop");

    let outbound = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
        .await
        .expect("stop should emit a cancellation reply")
        .expect("outbound");
    assert!(
        outbound.content.starts_with("Stopped "),
        "{}",
        outbound.content
    );

    assert!(
        tokio::time::timeout(Duration::from_millis(300), bus.consume_outbound())
            .await
            .is_err(),
        "stopping the session should cancel the btw lane before it completes"
    );

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn stop_releases_the_btw_slot_for_the_next_generation() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(BtwState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(BtwTestProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, multi_profile_config(dir.path()))
        .await
        .expect("agent");
    let runner = spawn_runner(agent.clone());

    bus.publish_inbound(inbound_message("cli", "slot2", "cli:slot2", "main-hold"))
        .await
        .expect("publish first main");
    wait_for_flag(&state.main_started, &state.main_started_notify).await;

    bus.publish_inbound(inbound_message("cli", "slot2", "cli:slot2", "/btw hold-btw"))
        .await
        .expect("publish first btw");
    wait_for_flag(&state.btw_started, &state.btw_started_notify).await;

    bus.publish_inbound(inbound_message("cli", "slot2", "cli:slot2", "/stop"))
        .await
        .expect("publish stop");
    let stop_outbound = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
        .await
        .expect("stop outbound")
        .expect("stop outbound");
    assert!(stop_outbound.content.starts_with("Stopped "), "{}", stop_outbound.content);

    state.main_released.store(false, Ordering::SeqCst);
    bus.publish_inbound(inbound_message("cli", "slot2", "cli:slot2", "main-hold"))
        .await
        .expect("publish second main");
    tokio::time::timeout(
        Duration::from_secs(1),
        wait_for_count(&state.main_start_count, &state.main_start_count_notify, 2),
    )
    .await
    .expect("second main should start");

    let stale_outbound = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
        .await
        .expect("stale btw outbound")
        .expect("stale btw outbound");
    assert!(
        stale_outbound.content.contains("stale") || stale_outbound.content.contains("generation"),
        "{}",
        stale_outbound.content
    );

    bus.publish_inbound(inbound_message(
        "cli",
        "slot2",
        "cli:slot2",
        "/btw second-btw",
    ))
    .await
    .expect("publish second btw");
    let btw_outbound = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
        .await
        .expect("second btw reply")
        .expect("second btw outbound");
    assert_eq!(btw_outbound.content, "btw reply");

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn btw_bypasses_debounce_and_never_merges_into_user_bursts() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(BtwState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(BtwTestProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, debounce_config(dir.path(), 200))
        .await
        .expect("agent");
    let runner = spawn_runner(agent.clone());

    bus.publish_inbound(inbound_message("cli", "burst", "cli:burst", "main-hold"))
        .await
        .expect("publish main");
    wait_for_flag(&state.main_started, &state.main_started_notify).await;

    for content in ["first", "second"] {
        bus.publish_inbound(inbound_message("cli", "burst", "cli:burst", content))
            .await
            .expect("publish burst message");
    }
    bus.publish_inbound(inbound_message("cli", "burst", "cli:burst", "/btw status?"))
        .await
        .expect("publish btw");

    let btw_outbound = tokio::time::timeout(Duration::from_millis(100), bus.consume_outbound())
        .await
        .expect("btw should bypass the debounce window")
        .expect("outbound");
    assert_eq!(btw_outbound.content, "btw reply");

    state.main_released.store(true, Ordering::SeqCst);
    state.main_release_notify.notify_waiters();
    let mut burst_outbound = None;
    for _ in 0..2 {
        let outbound = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
            .await
            .expect("debounced burst should still flush")
            .expect("outbound");
        if outbound.content.contains("[Compressed user burst]") {
            burst_outbound = Some(outbound);
            break;
        }
    }
    let burst_outbound = burst_outbound.expect("merged burst outbound");
    assert!(burst_outbound.content.contains("[Compressed user burst]"));
    assert!(
        !burst_outbound.content.contains("status?"),
        "{}",
        burst_outbound.content
    );

    let histories = state.histories.lock().await.clone();
    let seen_contents = histories
        .iter()
        .map(|messages| BtwTestProvider::user_content(messages))
        .collect::<Vec<_>>();
    assert!(
        seen_contents
            .iter()
            .any(|content| content.contains("[Compressed user burst]")),
        "{seen_contents:?}"
    );
    assert!(
        seen_contents
            .iter()
            .any(|content| content.contains("status?")),
        "{seen_contents:?}"
    );
    assert!(
        seen_contents
            .iter()
            .all(|content| !(content.contains("[Compressed user burst]")
                && content.contains("status?"))),
        "{seen_contents:?}"
    );

    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn btw_cannot_see_unsaved_inflight_main_lane_messages() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(BtwState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(BtwTestProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, multi_profile_config(dir.path()))
        .await
        .expect("agent");
    let runner = spawn_runner(agent.clone());

    bus.publish_inbound(inbound_message(
        "cli",
        "inflight",
        "cli:inflight",
        "inflight-shadow",
    ))
    .await
    .expect("publish inflight main");
    wait_for_flag(&state.main_started, &state.main_started_notify).await;

    bus.publish_inbound(inbound_message(
        "cli",
        "inflight",
        "cli:inflight",
        "/btw inflight-check",
    ))
    .await
    .expect("publish btw");

    wait_for_flag(&state.btw_started, &state.btw_started_notify).await;
    let histories = state.histories.lock().await.clone();
    let btw_history = histories
        .iter()
        .rev()
        .find(|messages| BtwTestProvider::user_content(messages).contains("inflight-check"))
        .expect("btw history");
    let saw_shadow = btw_history.iter().any(|message| {
        message.get("role").and_then(Value::as_str) == Some("assistant")
            && message
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|content| content.contains("inflight-shadow"))
    });
    assert!(!saw_shadow, "{btw_history:?}");

    state.main_released.store(true, Ordering::SeqCst);
    state.main_release_notify.notify_waiters();
    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn btw_snapshot_load_failures_return_a_user_visible_reply() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(BtwState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(BtwTestProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, multi_profile_config(dir.path()))
        .await
        .expect("agent");
    let runner = spawn_runner(agent.clone());

    bus.publish_inbound(inbound_message(
        "cli",
        "corrupt",
        "cli:corrupt",
        "main-hold",
    ))
    .await
    .expect("publish main");
    wait_for_flag(&state.main_started, &state.main_started_notify).await;

    let session_path = dir.path().join("sessions").join("cli_corrupt.jsonl");
    std::fs::write(&session_path, "not-json").expect("corrupt session after main started");

    bus.publish_inbound(inbound_message(
        "cli",
        "corrupt",
        "cli:corrupt",
        "/btw snapshot-profile-read",
    ))
    .await
    .expect("publish btw");

    let outbound = tokio::time::timeout(Duration::from_millis(200), bus.consume_outbound())
        .await
        .expect("btw should return a visible snapshot failure reply")
        .expect("outbound");
    assert!(
        outbound.content.contains("snapshot") || outbound.content.contains("failed"),
        "{}",
        outbound.content
    );

    state.main_released.store(true, Ordering::SeqCst);
    state.main_release_notify.notify_waiters();
    let main_outbound = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
        .await
        .expect("main should still complete after BTW failure")
        .expect("main outbound");
    assert_eq!(main_outbound.content, "main final");
    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn btw_provider_or_tool_failures_do_not_affect_the_main_lane() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(BtwState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(BtwTestProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::from_config(bus.clone(), provider, multi_profile_config(dir.path()))
        .await
        .expect("agent");
    let runner = spawn_runner(agent.clone());

    bus.publish_inbound(inbound_message("cli", "fail", "cli:fail", "main-hold"))
        .await
        .expect("publish main");
    wait_for_flag(&state.main_started, &state.main_started_notify).await;

    bus.publish_inbound(inbound_message("cli", "fail", "cli:fail", "/btw tool-fail"))
        .await
        .expect("publish btw");

    state.main_released.store(true, Ordering::SeqCst);
    state.main_release_notify.notify_waiters();

    let mut contents = Vec::new();
    while contents.len() < 2 {
        let outbound = tokio::time::timeout(Duration::from_secs(1), bus.consume_outbound())
            .await
            .expect("both lanes should resolve independently")
            .expect("outbound");
        contents.push(outbound.content);
    }
    assert!(
        contents.iter().any(|content| content == "main final"),
        "{contents:?}"
    );
    assert!(
        contents
            .iter()
            .any(|content| content.contains("failed") || content.contains("tool")),
        "{contents:?}"
    );

    agent.stop();
    runner.abort();
}
