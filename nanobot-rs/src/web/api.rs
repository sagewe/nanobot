use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::Config;
use crate::control::AuthenticatedUser;
use crate::cron::{CronJob, CronSchedule};
use crate::mcp::{McpServerInfo, McpServerToolAction};

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

const SESSION_COOKIE_NAME: &str = "nanobot_session";

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
            raw.split(';').find_map(|part| {
                let part = part.trim();
                let (name, value) = part.split_once('=')?;
                if name == SESSION_COOKIE_NAME {
                    Some(value.to_string())
                } else {
                    None
                }
            })
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
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static(
            "nanobot_session=deleted; Path=/; Max-Age=0; HttpOnly; SameSite=Strict",
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
    Json(config): Json<Config>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let user = require_authenticated_user(&user)?;
    let control = state
        .control_store()
        .ok_or_else(|| ApiError::not_found("control store not configured"))?;
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
