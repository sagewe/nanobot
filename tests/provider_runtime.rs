use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Map, Value, json};
use sidekick::providers::{LlmProvider, OpenAICompatibleProvider, ProviderRequestDescriptor};
use sidekick::session::SessionMessage;
use tokio::net::TcpListener;

#[derive(Clone)]
struct ProviderState {
    calls: Arc<AtomicUsize>,
    scenario: Scenario,
    payloads: Arc<tokio::sync::Mutex<Vec<Value>>>,
}

#[derive(Clone, Copy)]
enum Scenario {
    RetryThenSuccess,
    AuthFailure,
    EmptyChoices,
    CaptureRequestBody,
    PreserveAssistantExtras,
}

async fn chat_completions(
    State(state): State<ProviderState>,
    Json(payload): Json<Value>,
) -> (axum::http::StatusCode, Json<Value>) {
    let call_index = state.calls.fetch_add(1, Ordering::SeqCst);
    state.payloads.lock().await.push(payload);
    match state.scenario {
        Scenario::RetryThenSuccess if call_index < 2 => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": "model overloaded"}})),
        ),
        Scenario::RetryThenSuccess => (
            axum::http::StatusCode::OK,
            Json(json!({
                "choices": [{
                    "message": {"content": "eventual success"},
                    "finish_reason": "stop"
                }]
            })),
        ),
        Scenario::AuthFailure => (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"error": {"message": "invalid api key"}})),
        ),
        Scenario::EmptyChoices => (axum::http::StatusCode::OK, Json(json!({"choices": []}))),
        Scenario::CaptureRequestBody => (
            axum::http::StatusCode::OK,
            Json(json!({
                "choices": [{
                    "message": {"content": "captured"},
                    "finish_reason": "stop"
                }]
            })),
        ),
        Scenario::PreserveAssistantExtras => (
            axum::http::StatusCode::OK,
            Json(json!({
                "choices": [{
                    "message": {
                        "content": null,
                        "tool_calls": [{
                            "id": "call_1",
                            "type": "function",
                            "function": {"name": "search", "arguments": "{}"}
                        }],
                        "reasoning_content": "chain"
                    },
                    "finish_reason": "tool_calls"
                }]
            })),
        ),
    }
}

async fn start_server(state: ProviderState) -> SocketAddr {
    let app = Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    addr
}

#[tokio::test]
async fn provider_retries_transient_errors_only() {
    let calls = Arc::new(AtomicUsize::new(0));
    let addr = start_server(ProviderState {
        calls: calls.clone(),
        scenario: Scenario::RetryThenSuccess,
        payloads: Arc::new(tokio::sync::Mutex::new(Vec::new())),
    })
    .await;
    let provider = OpenAICompatibleProvider::new(
        "secret".to_string(),
        format!("http://{addr}/v1"),
        "demo-model".to_string(),
    );

    let response = provider
        .chat_with_retry(vec![], vec![], "demo-model")
        .await
        .expect("chat_with_retry");

    assert_eq!(response.content.as_deref(), Some("eventual success"));
    assert_eq!(response.finish_reason, "stop");
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn provider_propagates_auth_errors_from_chat_with_retry() {
    let calls = Arc::new(AtomicUsize::new(0));
    let addr = start_server(ProviderState {
        calls: calls.clone(),
        scenario: Scenario::AuthFailure,
        payloads: Arc::new(tokio::sync::Mutex::new(Vec::new())),
    })
    .await;
    let provider = OpenAICompatibleProvider::new(
        "secret".to_string(),
        format!("http://{addr}/v1"),
        "demo-model".to_string(),
    );

    let err = provider
        .chat_with_retry(vec![], vec![], "demo-model")
        .await
        .expect_err("chat_with_retry");

    assert!(err.to_string().contains("invalid api key"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn provider_propagates_empty_choices_from_chat_with_retry() {
    let calls = Arc::new(AtomicUsize::new(0));
    let addr = start_server(ProviderState {
        calls: calls.clone(),
        scenario: Scenario::EmptyChoices,
        payloads: Arc::new(tokio::sync::Mutex::new(Vec::new())),
    })
    .await;
    let provider = OpenAICompatibleProvider::new(
        String::new(),
        format!("http://{addr}/v1"),
        "demo-model".to_string(),
    );

    let err = provider
        .chat_with_retry(vec![], vec![], "demo-model")
        .await
        .expect_err("chat_with_retry");

    assert!(err.to_string().contains("provider returned no choices"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn stored_assistant_tool_calls_replay_reasoning_content_from_extra() {
    let mut extra = Map::new();
    extra.insert("reasoning_content".to_string(), json!("chain-of-thought"));
    let stored = SessionMessage {
        role: "assistant".to_string(),
        content: Value::Null,
        timestamp: None,
        tool_calls: Some(vec![json!({
            "id": "call_1",
            "type": "function",
            "function": {"name": "search", "arguments": "{}"}
        })]),
        tool_call_id: None,
        name: None,
        extra,
    };

    let replay = stored.to_llm_message();

    assert_eq!(
        replay.get("reasoning_content").and_then(Value::as_str),
        Some("chain-of-thought")
    );
    assert!(replay.get("tool_calls").is_some());
}

#[tokio::test]
async fn provider_request_extras_merge_into_body_and_runtime_fields_win() {
    let calls = Arc::new(AtomicUsize::new(0));
    let payloads = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let addr = start_server(ProviderState {
        calls: calls.clone(),
        scenario: Scenario::CaptureRequestBody,
        payloads: payloads.clone(),
    })
    .await;
    let provider = OpenAICompatibleProvider::new(
        String::new(),
        format!("http://{addr}/v1"),
        "demo-model".to_string(),
    );
    let request = ProviderRequestDescriptor::new(
        "openai",
        "real-model",
        [
            ("temperature".to_string(), json!(0.2)),
            ("reasoning".to_string(), json!({"enabled": true})),
            ("model".to_string(), json!("wrong-model")),
            ("messages".to_string(), json!(["wrong"])),
            ("tools".to_string(), json!(["wrong"])),
        ]
        .into_iter()
        .collect(),
    );
    let messages = vec![json!({"role": "user", "content": "hello"})];
    let tools = vec![json!({
        "type": "function",
        "function": {"name": "noop", "parameters": {"type": "object"}}
    })];

    let response = provider
        .chat_with_request(messages.clone(), tools.clone(), &request)
        .await
        .expect("chat_with_request");

    assert_eq!(response.content.as_deref(), Some("captured"));
    let payload = payloads.lock().await;
    let sent = payload.last().expect("captured payload");
    assert_eq!(sent["model"], json!("real-model"));
    assert_eq!(sent["messages"], json!(messages));
    assert_eq!(sent["tools"], json!(tools));
    assert_eq!(sent["temperature"], json!(0.2));
    assert_eq!(sent["reasoning"], json!({"enabled": true}));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn provider_preserves_unknown_assistant_fields_in_response_extra() {
    let calls = Arc::new(AtomicUsize::new(0));
    let addr = start_server(ProviderState {
        calls: calls.clone(),
        scenario: Scenario::PreserveAssistantExtras,
        payloads: Arc::new(tokio::sync::Mutex::new(Vec::new())),
    })
    .await;
    let provider = OpenAICompatibleProvider::new(
        String::new(),
        format!("http://{addr}/v1"),
        "demo-model".to_string(),
    );
    let request = ProviderRequestDescriptor::new("openai", "demo-model", Map::new());

    let response = provider
        .chat_with_request(vec![], vec![], &request)
        .await
        .expect("chat_with_request");

    assert_eq!(
        response
            .extra
            .get("reasoning_content")
            .and_then(Value::as_str),
        Some("chain")
    );
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
