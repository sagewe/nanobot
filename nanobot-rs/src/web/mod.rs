pub mod api;
pub mod page;

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use axum::{
    Router,
    routing::{get, post},
};
use tokio::net::TcpListener;
use tracing::{error, info};

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
        info!(
            session = %session_id,
            preview = %preview(message),
            "web session {session_id} started"
        );
        let result = self
            .agent
            .process_direct_logged(message, &format!("web:{session_id}"), "web", session_id)
            .await;
        match &result {
            Ok(reply) => {
                info!(
                    session = %session_id,
                    preview = %preview(reply),
                    "web session {session_id} completed"
                );
            }
            Err(error) => {
                error!(
                    session = %session_id,
                    error = %error,
                    "web session {session_id} failed"
                );
            }
        }
        result
    }
}

pub async fn serve(agent: AgentLoop, host: &str, port: u16) -> Result<()> {
    let state = AppState::new(Arc::new(AgentChatService::new(agent)));
    let listener = TcpListener::bind(format!("{host}:{port}")).await?;
    axum::serve(listener, build_router(state)).await?;
    Ok(())
}

fn preview(text: &str) -> String {
    const LIMIT: usize = 80;
    let trimmed = text.trim();
    let chars = trimmed.chars().collect::<Vec<_>>();
    if chars.len() <= LIMIT {
        return trimmed.to_string();
    }
    format!("{}…", chars[..LIMIT].iter().collect::<String>())
}
