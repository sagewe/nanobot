use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use axum::Router;
use nanobot_rs::web::{self, AppState, ChatService};
use tokio::net::TcpListener;

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
