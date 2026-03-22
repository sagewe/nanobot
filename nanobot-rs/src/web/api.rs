use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{AppState, WebSessionDetail, WebSessionGroup, WebSessionSummary};
use crate::presentation::render_web_html;

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub channel: Option<String>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub reply: String,
    #[serde(rename = "replyHtml")]
    pub reply_html: String,
    pub channel: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "activeProfile")]
    pub active_profile: String,
}

#[derive(Debug, Serialize)]
pub struct SessionListResponse {
    pub groups: Vec<WebSessionGroup>,
}

#[derive(Debug, Deserialize)]
pub struct DuplicateSessionRequest {
    pub channel: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
}

pub async fn healthz() -> &'static str {
    "ok"
}

pub async fn list_sessions(
    State(state): State<AppState>,
) -> Result<Json<SessionListResponse>, ApiError> {
    let sessions = state
        .chat
        .list_sessions()
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(SessionListResponse { groups: sessions }))
}

pub async fn get_session(
    Path((channel, session_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<WebSessionDetail>, ApiError> {
    let channel = validate_channel(&channel)?.to_string();
    let session_id = validate_session_id(&session_id)?;
    let session = state
        .chat
        .get_session(&channel, session_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("session not found"))?;
    Ok(Json(session))
}

pub async fn create_session(
    State(state): State<AppState>,
) -> Result<Json<WebSessionSummary>, ApiError> {
    let session = state
        .chat
        .create_session()
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(session))
}

pub async fn chat(
    State(state): State<AppState>,
    Json(request): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, ApiError> {
    let message = request.message.trim();
    if message.is_empty() {
        return Err(ApiError::bad_request("message must not be empty"));
    }
    let requested_channel = request.channel.as_deref().unwrap_or("web");
    let channel = validate_channel(requested_channel)?.to_string();
    let session_id = match request.session_id {
        Some(session_id) => validate_session_id(&session_id)?.to_string(),
        None if channel == "web" => {
            state
                .chat
                .create_session()
                .await
                .map_err(ApiError::internal)?
                .session_id
        }
        None => {
            return Err(ApiError::bad_request(
                "session is read-only; duplicate it into web before sending",
            ));
        }
    };
    if channel != "web" {
        return Err(ApiError::bad_request(
            "session is read-only; duplicate it into web before sending",
        ));
    }
    let chat = state
        .chat
        .chat(message, &channel, &session_id)
        .await
        .map_err(ApiError::internal)?;
    let reply_html = render_web_html(&chat.reply);
    Ok(Json(ChatResponse {
        reply: chat.reply,
        reply_html,
        channel,
        session_id,
        active_profile: chat.active_profile,
    }))
}

pub async fn duplicate_session(
    State(state): State<AppState>,
    Json(request): Json<DuplicateSessionRequest>,
) -> Result<Json<WebSessionDetail>, ApiError> {
    let channel = validate_channel(&request.channel)?.to_string();
    let session_id = validate_session_id(&request.session_id)?.to_string();
    if channel == "web" {
        return Err(ApiError::bad_request(
            "session is already writable; duplicate non-web sessions only",
        ));
    }
    let session = state
        .chat
        .duplicate_session(&channel, &session_id)
        .await
        .map_err(|error| map_duplicate_error(error, &channel, &session_id))?;
    Ok(Json(session))
}

pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn internal(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": self.message,
            })),
        )
            .into_response()
    }
}

fn validate_session_id(session_id: &str) -> Result<&str, ApiError> {
    let trimmed = session_id.trim();
    if trimmed.is_empty()
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':'))
    {
        return Err(ApiError::bad_request("invalid session id"));
    }
    Ok(trimmed)
}

fn validate_channel(channel: &str) -> Result<&str, ApiError> {
    let trimmed = channel.trim();
    if trimmed.is_empty()
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
    {
        return Err(ApiError::bad_request("invalid channel"));
    }
    Ok(trimmed)
}

fn map_duplicate_error(error: anyhow::Error, channel: &str, session_id: &str) -> ApiError {
    let message = error.to_string();
    if message.contains("not found") {
        return ApiError::not_found(format!("session {channel}:{session_id} not found"));
    }
    ApiError::internal(error)
}
