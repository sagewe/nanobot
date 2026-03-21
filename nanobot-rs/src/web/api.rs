use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{AppState, WebSessionDetail, WebSessionSummary};
use crate::presentation::render_web_html;

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub reply: String,
    #[serde(rename = "replyHtml")]
    pub reply_html: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "activeProfile")]
    pub active_profile: String,
}

#[derive(Debug, Serialize)]
pub struct SessionListResponse {
    pub sessions: Vec<WebSessionSummary>,
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
    Ok(Json(SessionListResponse { sessions }))
}

pub async fn get_session(
    Path(session_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<WebSessionDetail>, ApiError> {
    let session_id = validate_session_id(&session_id)?;
    let session = state
        .chat
        .get_session(session_id)
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
    let session_id = match request.session_id {
        Some(session_id) => validate_session_id(&session_id)?.to_string(),
        None => {
            state
                .chat
                .create_session()
                .await
                .map_err(ApiError::internal)?
                .session_id
        }
    };
    let chat = state
        .chat
        .chat(message, &session_id)
        .await
        .map_err(ApiError::internal)?;
    let reply_html = render_web_html(&chat.reply);
    Ok(Json(ChatResponse {
        reply: chat.reply,
        reply_html,
        session_id,
        active_profile: chat.active_profile,
    }))
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
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err(ApiError::bad_request("invalid session id"));
    }
    Ok(trimmed)
}
