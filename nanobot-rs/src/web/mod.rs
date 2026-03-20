pub mod api;
pub mod page;

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use axum::{Router, routing::{get, post}};

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
