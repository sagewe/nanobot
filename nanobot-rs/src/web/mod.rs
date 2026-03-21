pub mod api;
pub mod page;

use std::sync::Arc;

use anyhow::{Result, bail};
use async_trait::async_trait;
use axum::{
    Router,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tracing::{error, info};
use uuid::Uuid;

use crate::agent::AgentLoop;
use crate::presentation::render_web_html;
use crate::session::{Session, SessionMessage, SessionSummary};

const WEB_NAMESPACE: &str = "web";

#[async_trait]
pub trait ChatService: Send + Sync {
    async fn chat(&self, message: &str, session_id: &str) -> Result<WebChatReply>;

    async fn list_sessions(&self) -> Result<Vec<WebSessionSummary>> {
        bail!("session listing is not implemented for this service")
    }

    async fn get_session(&self, _session_id: &str) -> Result<Option<WebSessionDetail>> {
        bail!("session detail is not implemented for this service")
    }

    async fn create_session(&self) -> Result<WebSessionSummary> {
        bail!("session creation is not implemented for this service")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebChatReply {
    pub reply: String,
    pub active_profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebSessionSummary {
    pub session_id: String,
    pub updated_at: DateTime<Utc>,
    pub active_profile: String,
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebTranscriptMessage {
    pub role: String,
    pub content: String,
    pub timestamp: Option<DateTime<Utc>>,
    pub content_html: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebSessionDetail {
    pub session_id: String,
    pub updated_at: DateTime<Utc>,
    pub active_profile: String,
    pub messages: Vec<WebTranscriptMessage>,
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
        .route(
            "/api/sessions",
            get(api::list_sessions).post(api::create_session),
        )
        .route("/api/sessions/{id}", get(api::get_session))
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
    async fn chat(&self, message: &str, session_id: &str) -> Result<WebChatReply> {
        let session_key = web_session_key(session_id);
        info!(
            session = %session_id,
            preview = %preview(message),
            "web session {session_id} started"
        );
        let result = self
            .agent
            .process_direct_logged(message, &session_key, "web", session_id)
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
        let reply = result?;
        let active_profile = self.agent.current_profile_for_session(&session_key)?;
        Ok(WebChatReply {
            reply,
            active_profile,
        })
    }

    async fn list_sessions(&self) -> Result<Vec<WebSessionSummary>> {
        Ok(self
            .agent
            .list_sessions_in_namespace(WEB_NAMESPACE)?
            .into_iter()
            .map(|summary| summary_from_session(self, summary))
            .collect())
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<WebSessionDetail>> {
        let session = self.agent.load_session(&web_session_key(session_id))?;
        Ok(session.map(|session| detail_from_session(self, session)))
    }

    async fn create_session(&self) -> Result<WebSessionSummary> {
        let session_id = Uuid::new_v4().to_string();
        let session = self.agent.create_session(&web_session_key(&session_id))?;
        Ok(summary_from_full_session(self, session))
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

fn web_session_key(session_id: &str) -> String {
    format!("{WEB_NAMESPACE}:{session_id}")
}

fn summary_from_session(service: &AgentChatService, summary: SessionSummary) -> WebSessionSummary {
    WebSessionSummary {
        session_id: public_session_id(&summary.key),
        updated_at: summary.updated_at,
        active_profile: effective_profile(service, summary.active_profile.as_deref()),
        preview: summary.preview,
    }
}

fn summary_from_full_session(service: &AgentChatService, session: Session) -> WebSessionSummary {
    WebSessionSummary {
        session_id: public_session_id(&session.key),
        updated_at: session.updated_at,
        active_profile: effective_profile(service, session.active_profile.as_deref()),
        preview: session_preview(&session.messages),
    }
}

fn detail_from_session(service: &AgentChatService, session: Session) -> WebSessionDetail {
    WebSessionDetail {
        session_id: public_session_id(&session.key),
        updated_at: session.updated_at,
        active_profile: effective_profile(service, session.active_profile.as_deref()),
        messages: session
            .messages
            .iter()
            .filter_map(transcript_message)
            .collect(),
    }
}

fn transcript_message(message: &SessionMessage) -> Option<WebTranscriptMessage> {
    match message.role.as_str() {
        "user" | "assistant" => {
            let content = session_content_text(message)?;
            let content_html = if message.role == "assistant" {
                Some(render_web_html(&content))
            } else {
                None
            };
            Some(WebTranscriptMessage {
                role: message.role.clone(),
                content,
                timestamp: message.timestamp,
                content_html,
            })
        }
        _ => None,
    }
}

fn session_content_text(message: &SessionMessage) -> Option<String> {
    match &message.content {
        serde_json::Value::String(text) if !text.trim().is_empty() => Some(text.clone()),
        serde_json::Value::Null => None,
        other => Some(other.to_string()),
    }
}

fn session_preview(messages: &[SessionMessage]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find_map(|message| match message.role.as_str() {
            "user" | "assistant" => session_content_text(message),
            _ => None,
        })
        .map(|text| truncate_preview(&text, 120))
}

fn truncate_preview(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    let chars = trimmed.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return trimmed.to_string();
    }
    format!("{}…", chars[..max_chars].iter().collect::<String>())
}

fn effective_profile(service: &AgentChatService, selected: Option<&str>) -> String {
    selected
        .filter(|key| service.agent.has_profile(key))
        .unwrap_or(service.agent.default_profile())
        .to_string()
}

fn public_session_id(key: &str) -> String {
    key.strip_prefix(&format!("{WEB_NAMESPACE}:"))
        .unwrap_or(key)
        .to_string()
}
