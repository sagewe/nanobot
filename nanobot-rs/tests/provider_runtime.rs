use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use nanobot_rs::providers::{LlmProvider, OpenAICompatibleProvider};
use serde_json::{Value, json};
use tokio::net::TcpListener;

#[derive(Clone)]
struct ProviderState {
    calls: Arc<AtomicUsize>,
    scenario: Scenario,
}

#[derive(Clone, Copy)]
enum Scenario {
    RetryThenSuccess,
    AuthFailure,
    EmptyChoices,
}

async fn chat_completions(
    State(state): State<ProviderState>,
    Json(_payload): Json<Value>,
) -> (axum::http::StatusCode, Json<Value>) {
    let call_index = state.calls.fetch_add(1, Ordering::SeqCst);
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

    assert!(
        err.to_string().contains("invalid api key")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn provider_turns_empty_choices_into_error_response() {
    let calls = Arc::new(AtomicUsize::new(0));
    let addr = start_server(ProviderState {
        calls: calls.clone(),
        scenario: Scenario::EmptyChoices,
    })
    .await;
    let provider = OpenAICompatibleProvider::new(
        String::new(),
        format!("http://{addr}/v1"),
        "demo-model".to_string(),
    );

    let response = provider
        .chat_with_retry(vec![], vec![], "demo-model")
        .await
        .expect("chat_with_retry");

    assert_eq!(response.finish_reason, "error");
    assert!(
        response
            .content
            .as_deref()
            .unwrap_or_default()
            .contains("provider returned no choices")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
