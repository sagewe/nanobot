use std::collections::VecDeque;
use std::env;
use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::IntoResponse;
use axum::routing::{any, post};
use axum::{Json, Router};
use nanobot_rs::providers::{
    CodexProvider, CodexProviderConfig, LlmProvider, ProviderError, ProviderRequestDescriptor,
};
use serde_json::{Map, Value, json};
use tempfile::tempdir;
use tokio::net::TcpListener;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn with_openai_api_key<F>(value: &str, f: F)
where
    F: FnOnce(),
{
    let _guard = env_lock().lock().expect("env lock");
    let previous = env::var_os("OPENAI_API_KEY");

    unsafe {
        env::set_var("OPENAI_API_KEY", value);
    }

    f();

    match previous {
        Some(previous) => unsafe {
            env::set_var("OPENAI_API_KEY", previous);
        },
        None => unsafe {
            env::remove_var("OPENAI_API_KEY");
        },
    }
}

fn write_auth_file(dir: &tempfile::TempDir, content: &str) -> String {
    let path = dir.path().join("auth.json");
    fs::write(&path, content).expect("write auth file");
    path.display().to_string()
}

fn valid_auth_json() -> &'static str {
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

fn with_home_dir<F>(home_dir: &str, f: F)
where
    F: FnOnce(),
{
    let _guard = env_lock().lock().expect("env lock");
    let previous_home = env::var_os("HOME");

    unsafe {
        env::set_var("HOME", home_dir);
    }

    f();

    match previous_home {
        Some(previous) => unsafe {
            env::set_var("HOME", previous);
        },
        None => unsafe {
            env::remove_var("HOME");
        },
    }
}

fn build_provider(auth_file: String, addr: SocketAddr) -> CodexProvider {
    CodexProvider::from_config(CodexProviderConfig {
        auth_file,
        api_base: format!("http://{addr}/backend-api/"),
    })
    .expect("provider")
}

fn request_descriptor(extras: Map<String, Value>) -> ProviderRequestDescriptor {
    ProviderRequestDescriptor::new("codex", "gpt-5.4", extras)
}

#[derive(Clone, Debug)]
struct CapturedCodexRequest {
    path: String,
    authorization: Option<String>,
    account_id: Option<String>,
    body: Value,
}

#[derive(Clone)]
struct CodexCaptureState {
    requests: Arc<tokio::sync::Mutex<Vec<CapturedCodexRequest>>>,
    responses: Arc<tokio::sync::Mutex<VecDeque<(StatusCode, Value)>>>,
}

#[derive(Clone, Debug)]
struct CapturedLiveCodexRequest {
    method: String,
    path: String,
    authorization: Option<String>,
    account_id: Option<String>,
    accept: Option<String>,
    body: Value,
}

#[derive(Clone)]
struct LiveCodexCaptureState {
    requests: Arc<tokio::sync::Mutex<Vec<CapturedLiveCodexRequest>>>,
    response_status: StatusCode,
    response_body: String,
}

async fn capture_codex_responses_request(
    State(state): State<CodexCaptureState>,
    headers: HeaderMap,
    uri: Uri,
    Json(payload): Json<Value>,
) -> (StatusCode, Json<Value>) {
    state.requests.lock().await.push(CapturedCodexRequest {
        path: uri.path().to_string(),
        authorization: headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(ToString::to_string),
        account_id: headers
            .get("ChatGPT-Account-Id")
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
    (status, Json(body))
}

async fn start_codex_capture_server(
    responses: Vec<(StatusCode, Value)>,
) -> (
    SocketAddr,
    Arc<tokio::sync::Mutex<Vec<CapturedCodexRequest>>>,
) {
    let requests = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let app = Router::new()
        .route(
            "/backend-api/responses",
            post(capture_codex_responses_request),
        )
        .with_state(CodexCaptureState {
            requests: requests.clone(),
            responses: Arc::new(tokio::sync::Mutex::new(responses.into_iter().collect())),
        });
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (addr, requests)
}

fn mock_codex_sse_body() -> String {
    [
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"streamed \"}",
        "",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"content\"}",
        "",
        "data: {\"type\":\"response.completed\"}",
        "",
        "data: [DONE]",
        "",
    ]
    .join("\n")
}

fn response_output_item_done(item: Value) -> Value {
    json!({
        "type": "response.output_item.done",
        "item": item,
    })
}

fn response_function_call_arguments_delta(item_id: &str, delta: &str) -> Value {
    json!({
        "type": "response.function_call_arguments.delta",
        "item_id": item_id,
        "delta": delta,
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

fn codex_event_response(output: Vec<Value>) -> Value {
    json!({
        "id": "resp_event_1",
        "status": "completed",
        "output": output,
    })
}

async fn capture_codex_live_request(
    State(state): State<LiveCodexCaptureState>,
    method: Method,
    headers: HeaderMap,
    uri: Uri,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    state.requests.lock().await.push(CapturedLiveCodexRequest {
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

    (
        state.response_status,
        [
            (header::CONTENT_TYPE, "text/event-stream"),
            (header::CACHE_CONTROL, "no-cache"),
            (header::CONNECTION, "keep-alive"),
        ],
        state.response_body,
    )
}

async fn start_live_codex_capture_server() -> (
    SocketAddr,
    Arc<tokio::sync::Mutex<Vec<CapturedLiveCodexRequest>>>,
) {
    start_live_codex_capture_server_with_response(StatusCode::OK, mock_codex_sse_body()).await
}

async fn start_live_codex_capture_server_with_response(
    response_status: StatusCode,
    response_body: impl Into<String>,
) -> (
    SocketAddr,
    Arc<tokio::sync::Mutex<Vec<CapturedLiveCodexRequest>>>,
) {
    let requests = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/backend-api/{segment}", any(capture_codex_live_request))
        .with_state(LiveCodexCaptureState {
            requests: requests.clone(),
            response_status,
            response_body: response_body.into(),
        });
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    (addr, requests)
}

#[tokio::test]
async fn codex_provider_aggregates_completed_assistant_text_events() {
    let dir = tempdir().expect("tempdir");
    let auth_file = write_auth_file(&dir, valid_auth_json());
    let (addr, _) = start_codex_capture_server(vec![(
        StatusCode::OK,
        codex_event_response(vec![
            response_output_item_done(json!({
                "type": "message",
                "role": "assistant",
                "id": "msg_1",
                "status": "completed",
                "content": [
                    {"type": "output_text", "text": "hello from codex"}
                ]
            })),
            response_completed(),
        ]),
    )])
    .await;
    let provider = build_provider(auth_file, addr);

    let response = provider
        .chat_with_request(vec![], vec![], &request_descriptor(Map::new()))
        .await
        .expect("assistant text response");

    assert_eq!(response.content.as_deref(), Some("hello from codex"));
    assert!(response.tool_calls.is_empty());
    assert_eq!(response.finish_reason, "stop");
}

#[tokio::test]
async fn codex_provider_aggregates_completed_function_call_events() {
    let dir = tempdir().expect("tempdir");
    let auth_file = write_auth_file(&dir, valid_auth_json());
    let (addr, _) = start_codex_capture_server(vec![(
        StatusCode::OK,
        codex_event_response(vec![
            response_output_item_done(json!({
                "type": "function_call",
                "call_id": "call_1",
                "id": "fallback_id",
                "name": "read_file",
                "arguments": "{\"path\":\"src/main.rs\"}",
                "status": "completed"
            })),
            response_completed(),
        ]),
    )])
    .await;
    let provider = build_provider(auth_file, addr);

    let response = provider
        .chat_with_request(vec![], vec![], &request_descriptor(Map::new()))
        .await
        .expect("function call response");

    assert_eq!(response.content, None);
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].name, "read_file");
    assert_eq!(response.finish_reason, "tool_calls");
}

#[tokio::test]
async fn codex_provider_assembles_incremental_function_call_arguments() {
    let dir = tempdir().expect("tempdir");
    let auth_file = write_auth_file(&dir, valid_auth_json());
    let (addr, _) = start_codex_capture_server(vec![(
        StatusCode::OK,
        codex_event_response(vec![
            response_output_item_done(json!({
                "type": "function_call",
                "call_id": "call_2",
                "id": "fallback_id",
                "name": "read_file",
                "status": "in_progress"
            })),
            response_function_call_arguments_delta("call_2", "{\"path\":\"src/"),
            response_function_call_arguments_delta("call_2", "main.rs\"}"),
            response_function_call_arguments_done("call_2", ""),
            response_completed(),
        ]),
    )])
    .await;
    let provider = build_provider(auth_file, addr);

    let response = provider
        .chat_with_request(vec![], vec![], &request_descriptor(Map::new()))
        .await
        .expect("incremental function call response");

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].name, "read_file");
    assert_eq!(
        response.tool_calls[0].arguments,
        json!({"path":"src/main.rs"})
    );
    assert_eq!(response.finish_reason, "tool_calls");
}

#[tokio::test]
async fn codex_provider_rejects_malformed_event_sequences() {
    let dir = tempdir().expect("tempdir");
    let auth_file = write_auth_file(&dir, valid_auth_json());
    let (addr, _) = start_codex_capture_server(vec![(
        StatusCode::OK,
        codex_event_response(vec![
            response_function_call_arguments_delta("missing_call", "{\"path\":\"src/main.rs\"}"),
            response_completed(),
        ]),
    )])
    .await;
    let provider = build_provider(auth_file, addr);

    let err = provider
        .chat_with_request(vec![], vec![], &request_descriptor(Map::new()))
        .await
        .expect_err("malformed event stream should fail");

    let message = err.to_string();
    assert!(message.contains("malformed"), "{message}");
}

#[tokio::test]
async fn codex_provider_live_sse_contract_hits_codex_rooted_endpoint_and_aggregates_streamed_content()
 {
    let dir = tempdir().expect("tempdir");
    let auth_file = write_auth_file(&dir, valid_auth_json());
    let (addr, requests) = start_live_codex_capture_server().await;
    let provider = build_provider(auth_file, addr);
    let request = ProviderRequestDescriptor::new(
        "codex",
        "gpt-5.4",
        [
            ("model".to_string(), json!("wrong-model")),
            ("input".to_string(), json!(["wrong-input"])),
            ("tools".to_string(), json!(["wrong-tool"])),
        ]
        .into_iter()
        .collect::<Map<String, Value>>(),
    );
    let messages = vec![
        json!({"role": "user", "content": "hello"}),
        json!({"role": "assistant", "content": "working"}),
    ];
    let tools = vec![json!({
        "type": "function",
        "name": "search",
        "description": "Search docs",
        "parameters": {"type": "object"}
    })];

    let result = provider
        .chat_with_request(messages, tools.clone(), &request)
        .await;

    let captured = requests.lock().await;
    assert_eq!(captured.len(), 1);
    let sent = captured.last().expect("captured request");
    assert_eq!(sent.method, "POST");
    assert_eq!(sent.path, "/backend-api/codex");
    assert_eq!(sent.authorization.as_deref(), Some("Bearer access-token"));
    assert_eq!(sent.account_id.as_deref(), Some("account-id"));
    assert!(
        sent.accept
            .as_deref()
            .is_some_and(|value| value.contains("text/event-stream"))
    );
    assert_eq!(sent.body["model"], json!("gpt-5.4"));
    assert_eq!(
        sent.body["input"],
        json!([
            {
                "role": "user",
                "content": [{"type": "input_text", "text": "hello"}]
            },
            {
                "role": "assistant",
                "content": [{"type": "input_text", "text": "working"}]
            }
        ])
    );
    assert_eq!(sent.body["tools"], json!(tools));

    let response = result.expect("live SSE response");
    assert_eq!(response.content.as_deref(), Some("streamed content"));
    assert!(response.tool_calls.is_empty());
    assert_eq!(response.finish_reason, "stop");
}

#[tokio::test]
async fn codex_provider_normalizes_plain_text_response_and_sends_bearer_and_account_headers() {
    let dir = tempdir().expect("tempdir");
    let auth_file = write_auth_file(&dir, valid_auth_json());
    let (addr, requests) = start_codex_capture_server(vec![(
        StatusCode::OK,
        json!({
            "id": "resp_123",
            "status": "completed",
            "output": [{
                "type": "message",
                "role": "assistant",
                "id": "msg_123",
                "status": "completed",
                "content": [
                    {"type": "output_text", "text": "captured"}
                ]
            }]
        }),
    )])
    .await;
    let provider = build_provider(auth_file, addr);
    let request = ProviderRequestDescriptor::new(
        "codex",
        "gpt-5.4",
        [
            ("reasoning_effort".to_string(), json!("high")),
            ("store".to_string(), json!(false)),
            ("model".to_string(), json!("wrong-model")),
            ("input".to_string(), json!(["wrong-input"])),
            ("tools".to_string(), json!(["wrong-tool"])),
        ]
        .into_iter()
        .collect::<Map<String, Value>>(),
    );
    let messages = vec![
        json!({"role": "user", "content": "hello"}),
        json!({"role": "assistant", "content": "working"}),
    ];
    let tools = vec![json!({
        "type": "function",
        "name": "search",
        "description": "Search docs",
        "parameters": {"type": "object"}
    })];

    let response = provider
        .chat_with_request(messages, tools.clone(), &request)
        .await
        .expect("response");

    assert_eq!(response.content.as_deref(), Some("captured"));
    assert!(response.tool_calls.is_empty());
    assert_eq!(response.finish_reason, "stop");
    assert_eq!(
        response.extra.get("id").and_then(Value::as_str),
        Some("msg_123")
    );
    assert_eq!(
        response.extra.get("status").and_then(Value::as_str),
        Some("completed")
    );

    let captured = requests.lock().await;
    let sent = captured.last().expect("captured request");
    assert_eq!(sent.path, "/backend-api/responses");
    assert_eq!(sent.authorization.as_deref(), Some("Bearer access-token"));
    assert_eq!(sent.account_id.as_deref(), Some("account-id"));
    assert_eq!(sent.body["model"], json!("gpt-5.4"));
    assert_eq!(sent.body["reasoning_effort"], json!("high"));
    assert_eq!(sent.body["store"], json!(false));
    assert_eq!(
        sent.body["input"],
        json!([
            {
                "role": "user",
                "content": [{"type": "input_text", "text": "hello"}]
            },
            {
                "role": "assistant",
                "content": [{"type": "input_text", "text": "working"}]
            }
        ])
    );
    assert_eq!(sent.body["tools"], json!(tools));
}

#[tokio::test]
async fn codex_provider_normalizes_tool_call_response() {
    let dir = tempdir().expect("tempdir");
    let auth_file = write_auth_file(&dir, valid_auth_json());
    let (addr, requests) = start_codex_capture_server(vec![(
        StatusCode::OK,
        json!({
            "id": "resp_456",
            "status": "completed",
            "output": [{
                "type": "function_call",
                "call_id": "call_1",
                "id": "fallback_id",
                "name": "search",
                "arguments": "{\"query\":\"rust\"}",
                "status": "completed"
            }]
        }),
    )])
    .await;
    let provider = build_provider(auth_file, addr);

    let response = provider
        .chat_with_request(vec![], vec![], &request_descriptor(Map::new()))
        .await
        .expect("tool-call response");

    assert_eq!(response.content, None);
    assert_eq!(response.tool_calls.len(), 1);
    let tool_call = &response.tool_calls[0];
    assert_eq!(tool_call.id, "call_1");
    assert_eq!(tool_call.name, "search");
    assert_eq!(tool_call.arguments, json!({"query":"rust"}));
    assert_eq!(response.finish_reason, "tool_calls");
    assert_eq!(
        response.extra.get("status").and_then(Value::as_str),
        Some("completed")
    );

    let captured = requests.lock().await;
    assert_eq!(captured.len(), 1);
}

#[tokio::test]
async fn codex_provider_does_not_retry_auth_failures() {
    let dir = tempdir().expect("tempdir");
    let auth_file = write_auth_file(&dir, valid_auth_json());
    let (addr, requests) = start_codex_capture_server(vec![(
        StatusCode::UNAUTHORIZED,
        json!({"error": {"message": "invalid auth"}}),
    )])
    .await;
    let provider = build_provider(auth_file, addr);

    let err = provider
        .chat_with_retry(vec![], vec![], "gpt-5.4")
        .await
        .expect_err("auth failure should not retry");

    assert!(err.to_string().contains("invalid auth") || err.to_string().contains("401"));
    let captured = requests.lock().await;
    assert_eq!(captured.len(), 1);
}

#[tokio::test]
async fn codex_provider_retries_transient_5xx_failures_then_succeeds() {
    let dir = tempdir().expect("tempdir");
    let auth_file = write_auth_file(&dir, valid_auth_json());
    let (addr, requests) = start_codex_capture_server(vec![
        (
            StatusCode::TOO_MANY_REQUESTS,
            json!({"error": {"message": "rate limited"}}),
        ),
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error": {"message": "model overloaded"}}),
        ),
        (
            StatusCode::OK,
            json!({
                "id": "resp_789",
                "status": "completed",
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {"type": "output_text", "text": "eventual success"}
                    ]
                }]
            }),
        ),
    ])
    .await;
    let provider = build_provider(auth_file, addr);

    let response = provider
        .chat_with_retry(vec![], vec![], "gpt-5.4")
        .await
        .expect("retry then succeed");

    assert_eq!(response.content.as_deref(), Some("eventual success"));
    assert_eq!(response.finish_reason, "stop");
    let captured = requests.lock().await;
    assert_eq!(captured.len(), 3);
}

#[tokio::test]
async fn codex_provider_fatalizes_malformed_successful_responses() {
    let dir = tempdir().expect("tempdir");
    let auth_file = write_auth_file(&dir, valid_auth_json());
    let (addr, requests) = start_codex_capture_server(vec![(
        StatusCode::OK,
        json!({
            "id": "resp_bad"
        }),
    )])
    .await;
    let provider = build_provider(auth_file, addr);

    let err = provider
        .chat_with_request(vec![], vec![], &request_descriptor(Map::new()))
        .await
        .expect_err("missing output array should fail");

    assert!(
        err.downcast_ref::<ProviderError>().is_some(),
        "malformed successful responses should be converted into provider errors"
    );
    assert!(!nanobot_rs::providers::should_retry(&err));
    let message = err.to_string();
    assert!(message.contains("missing output array"), "{message}");

    let captured = requests.lock().await;
    assert_eq!(captured.len(), 1);
}

#[test]
fn codex_provider_rejects_missing_auth_file_and_ignores_openai_api_key() {
    with_openai_api_key("sk-test-openai-key", || {
        let err = CodexProvider::from_config(CodexProviderConfig {
            auth_file: "/tmp/does-not-exist-codex-auth.json".to_string(),
            api_base: "https://chatgpt.com/backend-api".to_string(),
        })
        .expect_err("missing auth file should fail");

        assert!(err.to_string().contains("auth file"));
    });
}

#[test]
fn codex_provider_rejects_malformed_json() {
    let dir = tempdir().expect("tempdir");
    let auth_file = write_auth_file(&dir, "{");

    with_openai_api_key("sk-test-openai-key", || {
        let err = CodexProvider::from_config(CodexProviderConfig {
            auth_file,
            api_base: "https://chatgpt.com/backend-api".to_string(),
        })
        .expect_err("malformed auth json should fail");

        let message = err.to_string();
        assert!(message.contains("parse"));
        assert!(!message.contains("OPENAI_API_KEY"));
    });
}

#[test]
fn codex_provider_rejects_unreadable_existing_auth_path() {
    let dir = tempdir().expect("tempdir");
    with_openai_api_key("sk-test-openai-key", || {
        let err = CodexProvider::from_config(CodexProviderConfig {
            auth_file: dir.path().display().to_string(),
            api_base: "https://chatgpt.com/backend-api".to_string(),
        })
        .expect_err("directory path should fail to read");

        assert!(err.to_string().contains("read"));
    });
}

#[test]
fn codex_provider_expands_home_directory_in_auth_file_path() {
    let home = tempdir().expect("tempdir");
    let auth_path = home.path().join("auth.json");
    fs::write(
        &auth_path,
        r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "access-token",
    "refresh_token": "refresh-token",
    "id_token": "id-token",
    "account_id": "account-id"
  }
}"#,
    )
    .expect("write auth file");

    with_home_dir(home.path().to_str().expect("home dir path"), || {
        let provider = CodexProvider::from_config(CodexProviderConfig {
            auth_file: "~/auth.json".to_string(),
            api_base: "https://chatgpt.com/backend-api".to_string(),
        })
        .expect("home directory auth path should load");

        assert_eq!(provider.auth_path(), auth_path.as_path());
        assert_eq!(provider.api_base(), "https://chatgpt.com/backend-api");
    });
}

#[test]
fn codex_provider_rejects_non_chatgpt_auth_mode_and_ignores_openai_api_key() {
    let dir = tempdir().expect("tempdir");
    let auth_file = write_auth_file(
        &dir,
        r#"{
  "auth_mode": "api_key",
  "tokens": {
    "access_token": "access-token",
    "refresh_token": "refresh-token",
    "id_token": "id-token",
    "account_id": "account-id"
  }
}"#,
    );

    with_openai_api_key("sk-test-openai-key", || {
        let err = CodexProvider::from_config(CodexProviderConfig {
            auth_file,
            api_base: "https://chatgpt.com/backend-api".to_string(),
        })
        .expect_err("invalid auth mode should fail");

        let message = err.to_string();
        assert!(message.contains("auth_mode"));
        assert!(message.contains("chatgpt"));
        assert!(!message.contains("OPENAI_API_KEY"));
    });
}

#[test]
fn codex_provider_rejects_missing_required_token_fields() {
    let cases = [
        (
            "access_token",
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "refresh_token": "refresh-token",
    "id_token": "id-token",
    "account_id": "account-id"
  }
}"#,
        ),
        (
            "refresh_token",
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "access-token",
    "id_token": "id-token",
    "account_id": "account-id"
  }
}"#,
        ),
        (
            "id_token",
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "access-token",
    "refresh_token": "refresh-token",
    "account_id": "account-id"
  }
}"#,
        ),
        (
            "account_id",
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "access-token",
    "refresh_token": "refresh-token",
    "id_token": "id-token"
  }
}"#,
        ),
    ];

    for (missing_field, content) in cases {
        let dir = tempdir().expect("tempdir");
        let auth_file = write_auth_file(&dir, content);
        let err = CodexProvider::from_config(CodexProviderConfig {
            auth_file,
            api_base: "https://chatgpt.com/backend-api".to_string(),
        })
        .expect_err("missing token field should fail");

        let message = err.to_string();
        assert!(
            message.contains(missing_field),
            "expected error to mention missing field {missing_field}, got: {message}"
        );
    }
}

#[test]
fn codex_provider_rejects_empty_required_token_fields() {
    let cases = [
        (
            "access_token",
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "",
    "refresh_token": "refresh-token",
    "id_token": "id-token",
    "account_id": "account-id"
  }
}"#,
        ),
        (
            "refresh_token",
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "access-token",
    "refresh_token": "",
    "id_token": "id-token",
    "account_id": "account-id"
  }
}"#,
        ),
        (
            "id_token",
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "access-token",
    "refresh_token": "refresh-token",
    "id_token": "",
    "account_id": "account-id"
  }
}"#,
        ),
        (
            "account_id",
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "access-token",
    "refresh_token": "refresh-token",
    "id_token": "id-token",
    "account_id": ""
  }
}"#,
        ),
    ];

    for (field, content) in cases {
        let dir = tempdir().expect("tempdir");
        let auth_file = write_auth_file(&dir, content);
        let err = CodexProvider::from_config(CodexProviderConfig {
            auth_file,
            api_base: "https://chatgpt.com/backend-api".to_string(),
        })
        .expect_err("empty token field should fail");

        let message = err.to_string();
        assert!(message.contains(field));
        assert!(message.contains("empty"));
    }
}

#[test]
fn codex_provider_loads_valid_auth_file() {
    let dir = tempdir().expect("tempdir");
    let auth_file = write_auth_file(
        &dir,
        r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "access-token",
    "refresh_token": "refresh-token",
    "id_token": "id-token",
    "account_id": "account-id"
  }
}"#,
    );

    let provider = CodexProvider::from_config(CodexProviderConfig {
        auth_file,
        api_base: "https://chatgpt.com/backend-api".to_string(),
    })
    .expect("valid auth file should load");

    assert_eq!(provider.api_base(), "https://chatgpt.com/backend-api");
    assert_eq!(provider.auth_path(), dir.path().join("auth.json").as_path());
}
