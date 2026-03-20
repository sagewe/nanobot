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
