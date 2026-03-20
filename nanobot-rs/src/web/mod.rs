pub mod api;
pub mod page;

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use axum::{Router, routing::{get, post}};
use tokio::net::TcpListener;

use crate::agent::AgentLoop;

#[async_trait]
pub trait ChatService: Send + Sync {
    async fn chat(&self, message: &str, session_id: &str) -> Result<String>;
}

#[derive(Clone)]
pub struct AppState {
    pub(crate) chat: Arc<dyn ChatService>,
}

impl AppState {
    pub fn new(chat: Arc<dyn ChatService>) -> Self {
        Self { chat }
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(page::index))
        .route("/healthz", get(api::healthz))
        .route("/api/chat", post(api::chat))
        .with_state(state)
}

#[derive(Clone)]
pub struct AgentChatService {
    agent: AgentLoop,
}

impl AgentChatService {
    pub fn new(agent: AgentLoop) -> Self {
        Self { agent }
    }
}

#[async_trait]
impl ChatService for AgentChatService {
    async fn chat(&self, message: &str, session_id: &str) -> Result<String> {
        self.agent
            .process_direct(message, &format!("web:{session_id}"), "web", session_id)
            .await
    }
}

pub async fn serve(agent: AgentLoop, host: &str, port: u16) -> Result<()> {
    let state = AppState::new(Arc::new(AgentChatService::new(agent)));
    let listener = TcpListener::bind(format!("{host}:{port}")).await?;
    axum::serve(listener, build_router(state)).await?;
    Ok(())
}
