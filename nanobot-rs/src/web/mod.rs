pub mod api;
pub mod page;

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use axum::{Router, routing::get};

#[async_trait]
pub trait ChatService: Send + Sync {
    async fn chat(&self, message: &str, session_id: &str) -> Result<String>;
}

#[derive(Clone)]
pub struct AppState {
    pub(crate) _chat: Arc<dyn ChatService>,
}

impl AppState {
    pub fn new(chat: Arc<dyn ChatService>) -> Self {
        Self { _chat: chat }
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(page::index))
        .route("/healthz", get(api::healthz))
        .with_state(state)
}
