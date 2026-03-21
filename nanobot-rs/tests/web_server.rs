use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use axum::Router;
use nanobot_rs::agent::AgentLoop;
use nanobot_rs::bus::MessageBus;
use nanobot_rs::config::WebToolsConfig;
use nanobot_rs::providers::{LlmProvider, LlmResponse, ToolCall};
use nanobot_rs::web::{self, AgentChatService, AppState, ChatService};
use serde_json::json;
use std::collections::VecDeque;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

#[derive(Clone, Default)]
struct StaticChatService;

#[async_trait]
impl ChatService for StaticChatService {
    async fn chat(&self, _message: &str, _session_id: &str) -> Result<String> {
        Ok("unused".to_string())
    }
}

fn test_state() -> AppState {
    AppState::new(Arc::new(StaticChatService))
}

#[derive(Clone)]
struct ReplyChatService {
    reply: String,
}

#[async_trait]
impl ChatService for ReplyChatService {
    async fn chat(&self, _message: &str, _session_id: &str) -> Result<String> {
        Ok(self.reply.clone())
    }
}

fn test_state_with_reply(reply: &str) -> AppState {
    AppState::new(Arc::new(ReplyChatService {
        reply: reply.to_string(),
    }))
}

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

async fn spawn_test_server(app: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    addr
}

#[tokio::test]
async fn root_and_health_routes_respond() {
    let app = web::build_router(test_state());
    let addr = spawn_test_server(app).await;

    let html = reqwest::get(format!("http://{addr}/"))
        .await
        .expect("fetch root")
        .text()
        .await
        .expect("root body");
    let health = reqwest::get(format!("http://{addr}/healthz"))
        .await
        .expect("fetch health")
        .text()
        .await
        .expect("health body");

    assert!(html.to_ascii_lowercase().contains("<!doctype html>"));
    assert_eq!(health, "ok");
}

#[tokio::test]
async fn chat_endpoint_returns_agent_reply() {
    let app = web::build_router(test_state_with_reply("hello from agent"));
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/chat"))
        .json(&serde_json::json!({
            "message": "hi",
            "sessionId": "browser-session-1"
        }))
        .send()
        .await
        .expect("send chat request")
        .json()
        .await
        .expect("chat response body");

    assert_eq!(response["reply"], "hello from agent");
    assert_eq!(response["sessionId"], "browser-session-1");
}

#[tokio::test]
async fn chat_endpoint_rejects_blank_messages() {
    let app = web::build_router(test_state_with_reply("should not be used"));
    let addr = spawn_test_server(app).await;

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/chat"))
        .json(&serde_json::json!({
            "message": "   ",
            "sessionId": "browser-session-2"
        }))
        .send()
        .await
        .expect("send blank chat request");
    let status = response.status();
    let payload: serde_json::Value = response.json().await.expect("blank chat response");

    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST);
    assert!(
        payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("message must not be empty")
    );
}

#[tokio::test]
async fn chat_endpoint_returns_message_tool_reply() {
    let dir = tempdir().expect("tempdir");
    let provider = mock_provider(vec![
        LlmResponse {
            content: Some("sending".to_string()),
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "message".to_string(),
                arguments: json!({
                    "content": "Hi from the message tool"
                }),
            }],
            finish_reason: "tool_calls".to_string(),
        },
        LlmResponse {
            content: Some("done".to_string()),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
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
    let app = web::build_router(AppState::new(Arc::new(AgentChatService::new(agent))));
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/chat"))
        .json(&serde_json::json!({
            "message": "hi",
            "sessionId": "browser-session-message"
        }))
        .send()
        .await
        .expect("send chat request")
        .json()
        .await
        .expect("chat response body");

    assert_eq!(response["reply"], "Hi from the message tool");
}
