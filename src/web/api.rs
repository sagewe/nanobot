use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path as FsPath, PathBuf};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::config::Config;
use crate::control::AuthenticatedUser;
use crate::cron::{CronJob, CronSchedule};
use crate::mcp::{McpServerInfo, McpServerToolAction};
use crate::skills::{ManagedSkillEntry, SkillSource, normalize_skill_name};

use super::{
    AppState, WebSessionDetail, WebSessionGroup, WebSessionSummary, WebWeixinAccount,
    WebWeixinLoginStatus, WeixinLoginStartResponse, WeixinWorkflowError, WeixinWorkflowErrorKind,
};

#[derive(Debug, Serialize)]
pub struct ProfileListResponse {
    pub profiles: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetProfileRequest {
    pub profile: String,
}
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
    pub persisted: bool,
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

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminCreateUserRequest {
    pub username: String,
    pub display_name: Option<String>,
    pub password: String,
    pub role: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AdminPasswordRequest {
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct AdminRoleRequest {
    pub role: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthUserResponse {
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    pub role: String,
}

const SESSION_COOKIE_NAME: &str = "sidekick_session";

fn user_response(user: &AuthenticatedUser) -> AuthUserResponse {
    AuthUserResponse {
        user_id: user.user_id.clone(),
        username: user.username.clone(),
        display_name: user.display_name.clone(),
        role: match user.role {
            crate::control::Role::Admin => "admin".to_string(),
            crate::control::Role::User => "user".to_string(),
        },
    }
}

fn session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| {
            for part in raw.split(';') {
                let part = part.trim();
                let Some((name, value)) = part.split_once('=') else {
                    continue;
                };
                if name == SESSION_COOKIE_NAME {
                    return Some(value.to_string());
                }
            }
            None
        })
}

async fn authenticated_user(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<AuthenticatedUser>, ApiError> {
    let Some(auth) = state.auth_service() else {
        return Ok(None);
    };
    let session_id =
        session_cookie(headers).ok_or_else(|| ApiError::unauthorized("authentication required"))?;
    auth.authenticate_session(&session_id)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::unauthorized("authentication required"))
        .map(Some)
}

async fn resolve_chat_service(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<std::sync::Arc<dyn super::ChatService>, ApiError> {
    let user = authenticated_user(state, headers).await?;
    state
        .chat_for_user(user.as_ref())
        .await
        .map_err(ApiError::internal)
}

async fn resolve_cron_service(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<std::sync::Arc<crate::cron::CronService>, ApiError> {
    let user = authenticated_user(state, headers).await?;
    state
        .cron_for_user(user.as_ref())
        .await
        .map_err(ApiError::internal)
}

fn require_authenticated_user<'a>(
    user: &'a Option<AuthenticatedUser>,
) -> Result<&'a AuthenticatedUser, ApiError> {
    user.as_ref()
        .ok_or_else(|| ApiError::unauthorized("authentication required"))
}

fn require_admin_user<'a>(user: &'a AuthenticatedUser) -> Result<&'a AuthenticatedUser, ApiError> {
    match user.role {
        crate::control::Role::Admin => Ok(user),
        crate::control::Role::User => Err(ApiError::forbidden("admin access required")),
    }
}

fn parse_role(value: Option<&str>) -> Result<crate::control::Role, ApiError> {
    match value.unwrap_or("user").trim().to_ascii_lowercase().as_str() {
        "admin" => Ok(crate::control::Role::Admin),
        "user" => Ok(crate::control::Role::User),
        _ => Err(ApiError::bad_request(
            "role must be either 'admin' or 'user'",
        )),
    }
}

fn user_record_json(user: &crate::control::UserRecord) -> serde_json::Value {
    json!({
        "userId": user.user_id,
        "username": user.username,
        "displayName": user.display_name,
        "role": match user.role {
            crate::control::Role::Admin => "admin",
            crate::control::Role::User => "user",
        },
        "enabled": user.enabled,
    })
}

fn auth_error(error: anyhow::Error) -> ApiError {
    ApiError::unauthorized(error.to_string())
}

pub async fn healthz() -> &'static str {
    "ok"
}

pub async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    let auth = state
        .auth_service()
        .ok_or_else(|| ApiError::not_found("auth service not configured"))?;
    let session = auth
        .login(request.username.trim(), request.password.trim())
        .map_err(auth_error)?;
    let user = auth
        .authenticate_session(&session.session_id)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::unauthorized("authentication required"))?;
    let mut response = Json(user_response(&user)).into_response();
    let cookie = format!(
        "{SESSION_COOKIE_NAME}={}; Path=/; HttpOnly; SameSite=Strict",
        session.session_id
    );
    let header_value = HeaderValue::from_str(&cookie)
        .map_err(|error| ApiError::internal(anyhow::anyhow!(error)))?;
    response
        .headers_mut()
        .insert(header::SET_COOKIE, header_value);
    Ok(response)
}

pub async fn change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let user = require_authenticated_user(&user)?;
    let control = state
        .control_store()
        .ok_or_else(|| ApiError::not_found("control store not configured"))?;
    let valid = control
        .verify_user_password(&user.user_id, request.current_password.trim())
        .map_err(ApiError::internal)?;
    if !valid {
        return Err(ApiError::unauthorized("current password is incorrect"));
    }
    control
        .set_user_password(&user.user_id, request.new_password.trim())
        .map_err(ApiError::internal)?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(auth) = state.auth_service() {
        if let Some(session_id) = session_cookie(&headers) {
            auth.logout(&session_id).map_err(ApiError::internal)?;
        }
    }
    let mut response = Json(json!({ "ok": true })).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_static(
            "sidekick_session=deleted; Path=/; Max-Age=0; HttpOnly; SameSite=Strict",
        ),
    );
    Ok(response)
}

pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AuthUserResponse>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    Ok(Json(user_response(require_authenticated_user(&user)?)))
}

pub async fn get_my_config(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Config>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let user = require_authenticated_user(&user)?;
    let control = state
        .control_store()
        .ok_or_else(|| ApiError::not_found("control store not configured"))?;
    let config = control
        .load_user_config(&user.user_id)
        .map_err(ApiError::internal)?;
    Ok(Json(config))
}

pub async fn list_admin_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let user = require_admin_user(require_authenticated_user(&user)?)?;
    let control = state
        .control_store()
        .ok_or_else(|| ApiError::not_found("control store not configured"))?;
    let mut users = Vec::new();
    for entry in control.list_users().map_err(ApiError::internal)? {
        let runtime_status = if let Some(runtimes) = &state.runtimes {
            if runtimes.is_running(&entry.user_id).await {
                "running"
            } else {
                "stopped"
            }
        } else {
            "unknown"
        };
        let mut value = user_record_json(&entry);
        if let Some(object) = value.as_object_mut() {
            object.insert("runtimeStatus".to_string(), json!(runtime_status));
        }
        users.push(value);
    }
    let _ = user;
    Ok(Json(json!({ "users": users })))
}

pub async fn create_admin_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AdminCreateUserRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    require_admin_user(require_authenticated_user(&user)?)?;
    let control = state
        .control_store()
        .ok_or_else(|| ApiError::not_found("control store not configured"))?;
    let display_name = request
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(request.username.as_str());
    let created = control
        .create_user(
            request.username.trim(),
            display_name,
            parse_role(request.role.as_deref())?,
            request.password.trim(),
        )
        .map_err(ApiError::internal)?;
    Ok(Json(json!({ "user": user_record_json(&created) })))
}

pub async fn enable_admin_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    require_admin_user(require_authenticated_user(&user)?)?;
    let control = state
        .control_store()
        .ok_or_else(|| ApiError::not_found("control store not configured"))?;
    let updated = control
        .set_user_enabled(&id, true)
        .map_err(ApiError::internal)?;
    Ok(Json(json!({ "user": user_record_json(&updated) })))
}

pub async fn disable_admin_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    require_admin_user(require_authenticated_user(&user)?)?;
    let control = state
        .control_store()
        .ok_or_else(|| ApiError::not_found("control store not configured"))?;
    let updated = control
        .set_user_enabled(&id, false)
        .map_err(ApiError::internal)?;
    if let Some(runtimes) = &state.runtimes {
        runtimes.stop_user(&id).await.map_err(ApiError::internal)?;
    }
    Ok(Json(json!({ "user": user_record_json(&updated) })))
}

pub async fn set_admin_user_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<AdminPasswordRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    require_admin_user(require_authenticated_user(&user)?)?;
    let control = state
        .control_store()
        .ok_or_else(|| ApiError::not_found("control store not configured"))?;
    let updated = control
        .set_user_password(&id, request.password.trim())
        .map_err(ApiError::internal)?;
    Ok(Json(json!({ "user": user_record_json(&updated) })))
}

pub async fn set_admin_user_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<AdminRoleRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    require_admin_user(require_authenticated_user(&user)?)?;
    let control = state
        .control_store()
        .ok_or_else(|| ApiError::not_found("control store not configured"))?;
    let updated = control
        .set_user_role(&id, parse_role(Some(request.role.as_str()))?)
        .map_err(ApiError::internal)?;
    Ok(Json(json!({ "user": user_record_json(&updated) })))
}

pub async fn put_my_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let user = require_authenticated_user(&user)?;
    let control = state
        .control_store()
        .ok_or_else(|| ApiError::not_found("control store not configured"))?;
    let config = serde_json::from_str::<Config>(&body)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    control
        .write_user_config(&user.user_id, &config)
        .map_err(ApiError::internal)?;
    if let Some(runtimes) = &state.runtimes {
        let _ = runtimes
            .reload(&user.user_id)
            .await
            .map_err(ApiError::internal)?;
    }
    Ok(Json(json!({ "ok": true })))
}

pub async fn list_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SessionListResponse>, ApiError> {
    let chat = resolve_chat_service(&state, &headers).await?;
    let sessions = chat.list_sessions().await.map_err(ApiError::internal)?;
    Ok(Json(SessionListResponse { groups: sessions }))
}

pub async fn get_session(
    Path((channel, session_id)): Path<(String, String)>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WebSessionDetail>, ApiError> {
    let channel = validate_channel(&channel)?.to_string();
    let session_id = validate_session_id(&session_id)?;
    let chat = resolve_chat_service(&state, &headers).await?;
    let session = chat
        .get_session(&channel, session_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found("session not found"))?;
    Ok(Json(session))
}

pub async fn create_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WebSessionSummary>, ApiError> {
    let chat = resolve_chat_service(&state, &headers).await?;
    let session = chat.create_session().await.map_err(ApiError::internal)?;
    Ok(Json(session))
}

pub async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, ApiError> {
    let message = request.message.trim();
    if message.is_empty() {
        return Err(ApiError::bad_request("message must not be empty"));
    }
    let requested_channel = request.channel.as_deref().unwrap_or("web");
    let channel = validate_channel(requested_channel)?.to_string();
    let chat_service = resolve_chat_service(&state, &headers).await?;
    let session_id = match request.session_id {
        Some(session_id) => validate_session_id(&session_id)?.to_string(),
        None if channel == "web" => {
            chat_service
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
    let chat = chat_service
        .chat(message, &channel, &session_id)
        .await
        .map_err(ApiError::internal)?;
    let reply_html = render_web_html(&chat.reply);
    Ok(Json(ChatResponse {
        reply: chat.reply,
        reply_html,
        persisted: chat.persisted,
        channel,
        session_id,
        active_profile: chat.active_profile,
    }))
}

pub async fn list_profiles(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ProfileListResponse>, ApiError> {
    let chat = resolve_chat_service(&state, &headers).await?;
    let profiles = chat.list_profiles().await.map_err(ApiError::internal)?;
    Ok(Json(ProfileListResponse { profiles }))
}

pub async fn set_session_profile(
    Path((channel, session_id)): Path<(String, String)>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SetProfileRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let channel = validate_channel(&channel)?.to_string();
    let session_id = validate_session_id(&session_id)?.to_string();
    resolve_chat_service(&state, &headers)
        .await?
        .set_session_profile(&channel, &session_id, &request.profile)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_session(
    Path((channel, session_id)): Path<(String, String)>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let channel = validate_channel(&channel)?.to_string();
    let session_id = validate_session_id(&session_id)?.to_string();
    resolve_chat_service(&state, &headers)
        .await?
        .delete_session(&channel, &session_id)
        .await
        .map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn duplicate_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<DuplicateSessionRequest>,
) -> Result<Json<WebSessionDetail>, ApiError> {
    let channel = validate_channel(&request.channel)?.to_string();
    let session_id = validate_session_id(&request.session_id)?.to_string();
    if channel == "web" {
        return Err(ApiError::bad_request(
            "session is already writable; duplicate non-web sessions only",
        ));
    }
    let chat = resolve_chat_service(&state, &headers).await?;
    let session = chat
        .duplicate_session(&channel, &session_id)
        .await
        .map_err(|error| map_duplicate_error(error, &channel, &session_id))?;
    Ok(Json(session))
}

pub async fn get_weixin_account(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WebWeixinAccount>, ApiError> {
    let chat = resolve_chat_service(&state, &headers).await?;
    let account = chat
        .get_weixin_account()
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(account))
}

pub async fn start_weixin_login(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WeixinLoginStartResponse>, ApiError> {
    let chat = resolve_chat_service(&state, &headers).await?;
    let login = chat
        .start_weixin_login()
        .await
        .map_err(map_weixin_workflow_error)?;
    Ok(Json(login))
}

pub async fn poll_weixin_login(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WebWeixinLoginStatus>, ApiError> {
    let chat = resolve_chat_service(&state, &headers).await?;
    let status = chat
        .poll_weixin_login()
        .await
        .map_err(map_weixin_workflow_error)?;
    Ok(Json(status))
}

pub async fn logout_weixin(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WebWeixinAccount>, ApiError> {
    let chat = resolve_chat_service(&state, &headers).await?;
    let account = chat
        .logout_weixin()
        .await
        .map_err(map_weixin_workflow_error)?;
    Ok(Json(account))
}

// ---------------------------------------------------------------------------
// Cron API
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct CronJobListResponse {
    pub jobs: Vec<CronJob>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddCronJobRequest {
    pub message: String,
    pub name: Option<String>,
    pub every_seconds: Option<i64>,
    pub cron_expr: Option<String>,
    pub tz: Option<String>,
    pub at: Option<String>,
    pub channel: Option<String>,
    pub chat_id: Option<String>,
}

pub async fn list_cron_jobs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<CronJobListResponse>, ApiError> {
    let cron = resolve_cron_service(&state, &headers).await?;
    let jobs = cron.list_jobs(true);
    Ok(Json(CronJobListResponse { jobs }))
}

pub async fn add_cron_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AddCronJobRequest>,
) -> Result<Json<CronJob>, ApiError> {
    let cron = resolve_cron_service(&state, &headers).await?;
    if req.message.is_empty() {
        return Err(ApiError::bad_request("message is required"));
    }
    let schedule = if let Some(every_s) = req.every_seconds {
        CronSchedule::every(every_s * 1000)
    } else if let Some(expr) = req.cron_expr {
        CronSchedule::cron(expr, req.tz)
    } else if let Some(at_str) = req.at {
        let dt = chrono::DateTime::parse_from_rfc3339(&at_str)
            .or_else(|_| {
                chrono::NaiveDateTime::parse_from_str(&at_str, "%Y-%m-%dT%H:%M:%S")
                    .map(|ndt| ndt.and_utc().fixed_offset())
            })
            .map_err(|_| ApiError::bad_request(format!("invalid datetime: {at_str}")))?;
        CronSchedule::at(dt.timestamp_millis())
    } else {
        return Err(ApiError::bad_request(
            "one of every_seconds, cron_expr, or at is required",
        ));
    };
    let delete_after = matches!(schedule.kind, crate::cron::ScheduleKind::At);
    let job_name = req.name.filter(|n| !n.is_empty()).unwrap_or_else(|| {
        let name_len = req.message.len().min(30);
        req.message[..name_len].to_string()
    });
    let job = cron
        .add_job(
            &job_name,
            schedule,
            req.message.clone(),
            true,
            req.channel,
            req.chat_id,
            delete_after,
        )
        .map_err(ApiError::internal)?;
    Ok(Json(job))
}

pub async fn delete_cron_job(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let cron = resolve_cron_service(&state, &headers).await?;
    if cron.remove_job(&id) {
        Ok(Json(json!({ "ok": true })))
    } else {
        Err(ApiError::not_found(format!("job {id} not found")))
    }
}

pub async fn toggle_cron_job(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<CronJob>, ApiError> {
    let cron = resolve_cron_service(&state, &headers).await?;
    let job = cron
        .toggle_job(&id)
        .ok_or_else(|| ApiError::not_found(format!("job {id} not found")))?;
    Ok(Json(job))
}

pub async fn run_cron_job(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let cron = resolve_cron_service(&state, &headers).await?;
    if cron.run_job(&id, true).await {
        Ok(Json(json!({ "ok": true })))
    } else {
        Err(ApiError::not_found(format!("job {id} not found")))
    }
}

// ---------------------------------------------------------------------------
// MCP API
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct McpServerListResponse {
    pub servers: Vec<McpServerInfo>,
}

pub async fn list_mcp_servers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<McpServerListResponse>, ApiError> {
    let chat = resolve_chat_service(&state, &headers).await?;
    let servers = chat.list_mcp_servers().await.map_err(ApiError::internal)?;
    Ok(Json(McpServerListResponse { servers }))
}

#[derive(Debug, Deserialize)]
pub struct ToggleMcpToolRequest {
    pub enabled: bool,
}

pub async fn toggle_mcp_tool(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(body): Json<ToggleMcpToolRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let chat = resolve_chat_service(&state, &headers).await?;
    let found = chat
        .toggle_mcp_tool(&name, body.enabled)
        .await
        .map_err(ApiError::internal)?;
    if found {
        Ok(Json(json!({ "ok": true })))
    } else {
        Err(ApiError::not_found(format!("tool '{name}' not found")))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerActionRequest {
    pub action: McpServerToolAction,
}

pub async fn apply_mcp_server_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(body): Json<McpServerActionRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let chat = resolve_chat_service(&state, &headers).await?;
    let found = chat
        .apply_mcp_server_action(&name, body.action)
        .await
        .map_err(ApiError::internal)?;
    if found {
        Ok(Json(json!({ "ok": true })))
    } else {
        Err(ApiError::not_found(format!("server '{name}' not found")))
    }
}

// ---------------------------------------------------------------------------
// Skills API
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsListResponse {
    pub workspace: Vec<SkillSummaryResponse>,
    pub builtin: Vec<SkillSummaryResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummaryResponse {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: String,
    pub enabled: bool,
    pub effective: bool,
    pub available: bool,
    pub missing_requirements: String,
    pub overrides_builtin: bool,
    pub shadowed_by_workspace: bool,
    pub read_only: bool,
    pub has_extra_files: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRequirementsResponse {
    pub bins: Vec<String>,
    pub env: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMetadataResponse {
    pub always: bool,
    pub requires: SkillRequirementsResponse,
    pub keywords: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDetailResponse {
    #[serde(flatten)]
    pub summary: SkillSummaryResponse,
    pub path: String,
    pub raw_content: String,
    pub body: String,
    pub normalized_name: String,
    pub metadata: SkillMetadataResponse,
    pub parse_warnings: Vec<String>,
    pub extra_files: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkspaceSkillRequest {
    pub id: String,
    pub raw_content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWorkspaceSkillRequest {
    pub raw_content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWorkspaceSkillStateRequest {
    pub enabled: bool,
}

pub async fn list_skills(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SkillsListResponse>, ApiError> {
    let workspace = resolve_skills_workspace(&state, &headers).await?;
    let managed = load_managed_skills(&state, &workspace)?;
    let mut workspace_summaries = managed
        .workspace
        .iter()
        .map(skill_summary_response)
        .collect::<Vec<_>>();
    let managed_ids = managed
        .workspace
        .iter()
        .map(|skill| skill.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for id in list_workspace_skill_ids(&workspace)? {
        if managed_ids.contains(&id) {
            continue;
        }
        if let Some(detail) = try_load_workspace_skill_detail(&state, &workspace, &id)? {
            workspace_summaries.push(detail.summary);
        }
    }
    workspace_summaries.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(Json(SkillsListResponse {
        workspace: workspace_summaries,
        builtin: managed.builtin.iter().map(skill_summary_response).collect(),
    }))
}

pub async fn get_skill(
    Path((source, id)): Path<(String, String)>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SkillDetailResponse>, ApiError> {
    get_skill_with_source(source, id, state, headers).await
}

pub async fn get_workspace_skill(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SkillDetailResponse>, ApiError> {
    get_skill_with_source("workspace".to_string(), id, state, headers).await
}

async fn get_skill_with_source(
    source: String,
    id: String,
    state: AppState,
    headers: HeaderMap,
) -> Result<Json<SkillDetailResponse>, ApiError> {
    let workspace = resolve_skills_workspace(&state, &headers).await?;
    let source = parse_skill_source(&source)?;
    let id = validate_existing_skill_id(&id)?;
    match source {
        SkillSource::Builtin => {
            let managed = load_managed_skills(&state, &workspace)?;
            let skill = managed_skill_by_id(&managed, source, id)
                .ok_or_else(|| ApiError::not_found(format!("skill '{id}' not found")))?;
            Ok(Json(skill_detail_response(&skill)?))
        }
        SkillSource::Workspace => try_load_workspace_skill_detail(&state, &workspace, id)?
            .map(Json)
            .ok_or_else(|| ApiError::not_found(format!("skill '{id}' not found"))),
    }
}

pub async fn create_workspace_skill(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateWorkspaceSkillRequest>,
) -> Result<Json<SkillDetailResponse>, ApiError> {
    let workspace = resolve_skills_workspace(&state, &headers).await?;
    let id = validate_skill_id(&request.id)?.to_string();
    if request.raw_content.is_empty() {
        return Err(ApiError::bad_request("rawContent must not be empty"));
    }
    let skill_dir = workspace_skill_dir(&workspace, &id);
    if skill_dir.exists() {
        return Err(ApiError::conflict(format!(
            "workspace skill '{id}' already exists"
        )));
    }
    fs::create_dir_all(&skill_dir).map_err(|error| ApiError::internal(error.into()))?;
    fs::write(skill_dir.join("SKILL.md"), &request.raw_content)
        .map_err(|error| ApiError::internal(error.into()))?;
    let detail = saved_workspace_skill_detail(&state, &workspace, &id, &request.raw_content)?;
    Ok(Json(detail))
}

pub async fn update_workspace_skill(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<UpdateWorkspaceSkillRequest>,
) -> Result<Json<SkillDetailResponse>, ApiError> {
    let workspace = resolve_skills_workspace(&state, &headers).await?;
    let id = validate_existing_skill_id(&id)?;
    let skill_path = require_mutable_workspace_skill(&state, &workspace, &id)?;
    fs::write(&skill_path, &request.raw_content)
        .map_err(|error| ApiError::internal(error.into()))?;
    let detail = saved_workspace_skill_detail(&state, &workspace, &id, &request.raw_content)?;
    Ok(Json(detail))
}

pub async fn update_workspace_skill_state(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<UpdateWorkspaceSkillStateRequest>,
) -> Result<Json<SkillDetailResponse>, ApiError> {
    let workspace = resolve_skills_workspace(&state, &headers).await?;
    let id = validate_existing_skill_id(&id)?;
    let _skill_path = require_mutable_workspace_skill(&state, &workspace, &id)?;
    let state_read_path = workspace_skill_state_read_path(&workspace);
    let state_write_path = workspace_skill_state_path(&workspace);
    let mut states = load_workspace_skill_state_document(&state_read_path)?;
    set_workspace_skill_enabled(&mut states, &id, request.enabled)?;
    save_workspace_skill_state_document(&state_write_path, &states)?;
    let detail = try_load_workspace_skill_detail(&state, &workspace, &id)?
        .ok_or_else(|| ApiError::bad_request(format!("skill '{id}' could not be loaded")))?;
    Ok(Json(detail))
}

pub async fn delete_workspace_skill(
    Path(id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let workspace = resolve_skills_workspace(&state, &headers).await?;
    let id = validate_existing_skill_id(&id)?;
    let skill_dir = require_mutable_workspace_skill_dir(&state, &workspace, &id)?;
    let state_read_path = workspace_skill_state_read_path(&workspace);
    let state_write_path = workspace_skill_state_path(&workspace);
    fs::remove_dir_all(&skill_dir).map_err(|error| ApiError::internal(error.into()))?;
    if let Some(mut states) = load_workspace_skill_state_document_for_delete(&state_read_path)? {
        states.remove(id);
        save_or_remove_workspace_skill_state_document(&state_write_path, &states)?;
        let legacy_state_path = legacy_workspace_skill_state_path(&workspace);
        if legacy_state_path != state_write_path {
            save_or_remove_workspace_skill_state_document(&legacy_state_path, &states)?;
        }
    }
    Ok(Json(json!({ "ok": true })))
}

async fn resolve_skills_workspace(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<PathBuf, ApiError> {
    let user = authenticated_user(state, headers).await?;
    state
        .workspace_for_user(user.as_ref())
        .map_err(ApiError::internal)
}

fn parse_skill_source(source: &str) -> Result<SkillSource, ApiError> {
    match source.trim() {
        "builtin" => Ok(SkillSource::Builtin),
        "workspace" => Ok(SkillSource::Workspace),
        _ => Err(ApiError::bad_request(
            "skill source must be either 'builtin' or 'workspace'",
        )),
    }
}

fn validate_skill_id(skill_id: &str) -> Result<&str, ApiError> {
    let trimmed = skill_id.trim();
    if trimmed.is_empty()
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
    {
        return Err(ApiError::bad_request("invalid skill id"));
    }
    Ok(trimmed)
}

fn validate_existing_skill_id(skill_id: &str) -> Result<&str, ApiError> {
    if skill_id.is_empty() || skill_id.contains('/') || skill_id.contains('\\') {
        return Err(ApiError::bad_request("invalid skill id"));
    }
    let mut components = FsPath::new(skill_id).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(skill_id),
        _ => Err(ApiError::bad_request("invalid skill id")),
    }
}

fn managed_skill_by_id(
    managed: &crate::skills::ManagedSkills,
    source: SkillSource,
    id: &str,
) -> Option<ManagedSkillEntry> {
    let entries = match source {
        SkillSource::Builtin => &managed.builtin,
        SkillSource::Workspace => &managed.workspace,
    };
    entries.iter().find(|skill| skill.id == id).cloned()
}

fn load_managed_skills(
    state: &AppState,
    workspace: &FsPath,
) -> Result<crate::skills::ManagedSkills, ApiError> {
    state
        .skills_catalog(workspace.to_path_buf())
        .discover_managed()
        .map_err(ApiError::internal)
}

fn try_load_workspace_skill(
    state: &AppState,
    workspace: &FsPath,
    id: &str,
) -> Result<Option<ManagedSkillEntry>, ApiError> {
    let managed = load_managed_skills(state, workspace)?;
    Ok(managed_skill_by_id(&managed, SkillSource::Workspace, id))
}

fn try_load_workspace_skill_detail(
    state: &AppState,
    workspace: &FsPath,
    id: &str,
) -> Result<Option<SkillDetailResponse>, ApiError> {
    if let Some(skill) = try_load_workspace_skill(state, workspace, id)? {
        return Ok(Some(skill_detail_response(&skill)?));
    }
    try_load_fallback_workspace_skill_detail(workspace, id)
}

fn skill_summary_response(skill: &ManagedSkillEntry) -> SkillSummaryResponse {
    SkillSummaryResponse {
        id: skill.id.clone(),
        name: skill.entry.name.clone(),
        description: skill.entry.description.clone(),
        source: match skill.source {
            SkillSource::Builtin => "builtin".to_string(),
            SkillSource::Workspace => "workspace".to_string(),
        },
        enabled: skill.enabled,
        effective: skill.effective,
        available: skill.entry.available,
        missing_requirements: skill.entry.missing_requirements.clone(),
        overrides_builtin: skill.overrides_builtin,
        shadowed_by_workspace: skill.shadowed_by_workspace,
        read_only: matches!(skill.source, SkillSource::Builtin),
        has_extra_files: skill.has_extra_files,
    }
}

fn skill_detail_response(skill: &ManagedSkillEntry) -> Result<SkillDetailResponse, ApiError> {
    Ok(SkillDetailResponse {
        summary: skill_summary_response(skill),
        path: skill.entry.path.display().to_string(),
        raw_content: skill.entry.raw_content.clone(),
        body: skill.entry.body.clone(),
        normalized_name: skill.entry.normalized_name.clone(),
        metadata: SkillMetadataResponse {
            always: skill.entry.metadata.always,
            requires: SkillRequirementsResponse {
                bins: skill.entry.metadata.requires.bins.clone(),
                env: skill.entry.metadata.requires.env.clone(),
            },
            keywords: skill.entry.metadata.keywords.clone(),
            tags: skill.entry.metadata.tags.clone(),
        },
        parse_warnings: Vec::new(),
        extra_files: list_extra_files(
            skill.entry.path.parent().unwrap_or_else(|| FsPath::new("")),
        )?,
    })
}

fn list_extra_files(skill_dir: &FsPath) -> Result<Vec<String>, ApiError> {
    let Ok(entries) = fs::read_dir(skill_dir) else {
        return Ok(Vec::new());
    };
    let mut files = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_string_lossy().into_owned();
            (name != "SKILL.md").then_some(name)
        })
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn workspace_skill_dir(workspace: &FsPath, id: &str) -> PathBuf {
    workspace.join("skills").join(id)
}

fn list_workspace_skill_ids(workspace: &FsPath) -> Result<Vec<String>, ApiError> {
    let skills_root = workspace.join("skills");
    if !skills_root.exists() {
        return Ok(Vec::new());
    }
    let mut ids = fs::read_dir(&skills_root)
        .map_err(|error| ApiError::internal(error.into()))?
        .flatten()
        .filter(|entry| entry.path().is_dir() && entry.path().join("SKILL.md").exists())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    ids.sort();
    Ok(ids)
}

fn require_mutable_workspace_skill(
    state: &AppState,
    workspace: &FsPath,
    id: &str,
) -> Result<PathBuf, ApiError> {
    let skill_path = workspace_skill_dir(workspace, id).join("SKILL.md");
    if skill_path.exists() {
        return Ok(skill_path);
    }
    if builtin_skill_exists(state, workspace, id)? {
        return Err(ApiError::bad_request(
            "builtin skills are read-only; create a workspace copy to customize them",
        ));
    }
    Err(ApiError::not_found(format!("skill '{id}' not found")))
}

fn require_mutable_workspace_skill_dir(
    state: &AppState,
    workspace: &FsPath,
    id: &str,
) -> Result<PathBuf, ApiError> {
    let skill_dir = workspace_skill_dir(workspace, id);
    if skill_dir.exists() {
        return Ok(skill_dir);
    }
    if builtin_skill_exists(state, workspace, id)? {
        return Err(ApiError::bad_request(
            "builtin skills are read-only; create a workspace copy to customize them",
        ));
    }
    Err(ApiError::not_found(format!("skill '{id}' not found")))
}

fn builtin_skill_exists(state: &AppState, workspace: &FsPath, id: &str) -> Result<bool, ApiError> {
    let managed = load_managed_skills(state, workspace)?;
    Ok(managed.builtin.iter().any(|skill| skill.id == id))
}

fn workspace_skill_state_path(workspace: &FsPath) -> PathBuf {
    workspace.join(".sidekick").join("skills-state.json")
}

fn legacy_workspace_skill_state_path(workspace: &FsPath) -> PathBuf {
    workspace.join(".nanobot").join("skills-state.json")
}

fn workspace_skill_state_read_path(workspace: &FsPath) -> PathBuf {
    let primary = workspace_skill_state_path(workspace);
    if primary.exists() {
        primary
    } else {
        legacy_workspace_skill_state_path(workspace)
    }
}

fn workspace_skill_enabled_lenient(workspace: &FsPath, id: &str) -> bool {
    let path = workspace_skill_state_read_path(workspace);
    let Ok(raw) = fs::read_to_string(path) else {
        return true;
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return true;
    };
    value
        .as_object()
        .and_then(|states| states.get(id))
        .and_then(Value::as_object)
        .and_then(|state| state.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

fn load_workspace_skill_state_document(path: &FsPath) -> Result<Map<String, Value>, ApiError> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(ApiError::internal(error.into())),
    };
    let value: Value = serde_json::from_str(&raw).map_err(|error| {
        ApiError::bad_request(format!("skills state file is malformed: {error}"))
    })?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::bad_request("skills state file must contain a JSON object"))
}

fn load_workspace_skill_state_document_for_delete(
    path: &FsPath,
) -> Result<Option<Map<String, Value>>, ApiError> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Some(Map::new())),
        Err(error) => return Err(ApiError::internal(error.into())),
    };
    let value: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    Ok(value.as_object().cloned())
}

fn save_workspace_skill_state_document(
    path: &FsPath,
    states: &Map<String, Value>,
) -> Result<(), ApiError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| ApiError::internal(error.into()))?;
    }
    let raw =
        serde_json::to_vec_pretty(states).map_err(|error| ApiError::internal(error.into()))?;
    fs::write(path, raw).map_err(|error| ApiError::internal(error.into()))
}

fn save_or_remove_workspace_skill_state_document(
    path: &FsPath,
    states: &Map<String, Value>,
) -> Result<(), ApiError> {
    if states.is_empty() {
        for candidate in [path.to_path_buf()] {
            match fs::remove_file(candidate) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(ApiError::internal(error.into())),
            }
        }
        Ok(())
    } else {
        save_workspace_skill_state_document(path, states)
    }
}

fn set_workspace_skill_enabled(
    states: &mut Map<String, Value>,
    id: &str,
    enabled: bool,
) -> Result<(), ApiError> {
    match states.entry(id.to_string()) {
        serde_json::map::Entry::Vacant(entry) => {
            entry.insert(json!({ "enabled": enabled }));
            Ok(())
        }
        serde_json::map::Entry::Occupied(mut entry) => {
            let object = entry.get_mut().as_object_mut().ok_or_else(|| {
                ApiError::bad_request(format!("skills state entry '{id}' must be a JSON object"))
            })?;
            object.insert("enabled".to_string(), json!(enabled));
            Ok(())
        }
    }
}

fn saved_workspace_skill_detail(
    state: &AppState,
    workspace: &FsPath,
    id: &str,
    raw_content: &str,
) -> Result<SkillDetailResponse, ApiError> {
    if let Some(skill) = try_load_workspace_skill(state, workspace, id)? {
        return skill_detail_response(&skill);
    }
    fallback_workspace_skill_detail(
        workspace,
        id,
        raw_content,
        workspace_skill_enabled_lenient(workspace, id),
    )
}

fn fallback_workspace_skill_detail(
    workspace: &FsPath,
    id: &str,
    raw_content: &str,
    enabled: bool,
) -> Result<SkillDetailResponse, ApiError> {
    let extra_files = list_extra_files(&workspace_skill_dir(workspace, id))?;
    let parsed = parse_raw_skill_content(raw_content);
    let parsed_name = parsed
        .meta
        .get("name")
        .cloned()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| id.to_string());
    let normalized_name = normalize_skill_name(&parsed_name);
    let description = parsed
        .meta
        .get("description")
        .cloned()
        .filter(|description| !description.trim().is_empty())
        .unwrap_or_else(|| parsed_name.clone());

    Ok(SkillDetailResponse {
        summary: SkillSummaryResponse {
            id: id.to_string(),
            name: parsed_name,
            description,
            source: "workspace".to_string(),
            enabled,
            effective: false,
            available: true,
            missing_requirements: String::new(),
            overrides_builtin: false,
            shadowed_by_workspace: false,
            read_only: false,
            has_extra_files: !extra_files.is_empty(),
        },
        path: workspace_skill_dir(workspace, id)
            .join("SKILL.md")
            .display()
            .to_string(),
        raw_content: raw_content.to_string(),
        body: parsed.body,
        normalized_name: if normalized_name.is_empty() {
            id.to_string()
        } else {
            normalized_name
        },
        metadata: SkillMetadataResponse {
            always: false,
            requires: SkillRequirementsResponse {
                bins: Vec::new(),
                env: Vec::new(),
            },
            keywords: Vec::new(),
            tags: Vec::new(),
        },
        parse_warnings: vec![
            "saved raw content could not be rediscovered as a managed skill".to_string(),
        ],
        extra_files,
    })
}

fn try_load_fallback_workspace_skill_detail(
    workspace: &FsPath,
    id: &str,
) -> Result<Option<SkillDetailResponse>, ApiError> {
    let skill_path = workspace_skill_dir(workspace, id).join("SKILL.md");
    let raw_content = match fs::read_to_string(&skill_path) {
        Ok(raw_content) => raw_content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ApiError::internal(error.into())),
    };
    Ok(Some(fallback_workspace_skill_detail(
        workspace,
        id,
        &raw_content,
        workspace_skill_enabled_lenient(workspace, id),
    )?))
}

#[derive(Debug, Default)]
struct ParsedRawSkillContent {
    meta: BTreeMap<String, String>,
    body: String,
}

fn parse_raw_skill_content(content: &str) -> ParsedRawSkillContent {
    if let Some(rest) = content.strip_prefix("---\n")
        && let Some((frontmatter, body)) = rest.split_once("\n---\n")
    {
        let mut meta = BTreeMap::new();
        for line in frontmatter.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            meta.insert(key.trim().to_string(), trim_wrapped_quotes(value.trim()));
        }
        return ParsedRawSkillContent {
            meta,
            body: body.to_string(),
        };
    }
    ParsedRawSkillContent {
        meta: BTreeMap::new(),
        body: content.to_string(),
    }
}

fn trim_wrapped_quotes(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
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

    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    fn internal(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
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
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':' | '@' | '.'))
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

fn map_weixin_workflow_error(error: anyhow::Error) -> ApiError {
    if let Some(error) = error.downcast_ref::<WeixinWorkflowError>() {
        return match error.kind() {
            WeixinWorkflowErrorKind::Disabled | WeixinWorkflowErrorKind::LoginNotStarted => {
                ApiError::conflict(error.to_string())
            }
            WeixinWorkflowErrorKind::InitFailed => ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: error.to_string(),
            },
        };
    }
    ApiError::internal(error)
}
