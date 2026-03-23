use std::collections::VecDeque;
use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::IntoResponse;
use axum::routing::any;
use axum::{Json, Router};
use nanobot_rs::agent::{AgentLoop, SubagentManager};
use nanobot_rs::bus::{InboundMessage, MessageBus};
use nanobot_rs::config::{AgentProfileConfig, Config, WebToolsConfig};
use nanobot_rs::providers::{
    LlmProvider, LlmResponse, ProviderPool, ProviderRequestDescriptor, ToolCall,
};
use serde_json::{Map, Value, json};
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
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            bus.consume_outbound()
        )
        .await
        .is_err()
    );
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
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            bus.consume_outbound()
        )
        .await
        .is_err()
    );
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
    assert!(
        requests[0]
            .accept
            .as_deref()
            .is_some_and(|value| value.contains("text/event-stream"))
    );
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
    assert!(
        requests[1]
            .accept
            .as_deref()
            .is_some_and(|value| value.contains("text/event-stream"))
    );

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
