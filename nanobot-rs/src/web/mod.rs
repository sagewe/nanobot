pub mod api;
pub mod page;

use std::error::Error as StdError;
use std::fmt;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, bail};
use async_trait::async_trait;
use axum::{
    Router,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use qrcodegen::{QrCode, QrCodeEcc};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tracing::{error, info};
use url::Url;
use uuid::Uuid;

use crate::agent::AgentLoop;
use crate::channels::weixin::{WeixinAccountState, WeixinAccountStore, WeixinLoginManager};
use crate::config::WeixinConfig;
use crate::control::{AuthService, AuthenticatedUser, ControlStore, RuntimeManager};
use crate::cron::CronService;
use crate::mcp::{McpServerInfo, McpServerToolAction};
use crate::presentation::render_web_html;
use crate::session::{
    Session, SessionGroupSummary, SessionMessage, SessionSummary, split_session_key,
};

const WEB_NAMESPACE: &str = "web";

#[async_trait]
pub trait ChatService: Send + Sync {
    async fn chat(&self, message: &str, channel: &str, session_id: &str) -> Result<WebChatReply>;

    fn workspace_path(&self) -> Option<&Path> {
        None
    }

    async fn list_sessions(&self) -> Result<Vec<WebSessionGroup>> {
        bail!("session listing is not implemented for this service")
    }

    async fn get_session(
        &self,
        _channel: &str,
        _session_id: &str,
    ) -> Result<Option<WebSessionDetail>> {
        bail!("session detail is not implemented for this service")
    }

    async fn create_session(&self) -> Result<WebSessionSummary> {
        bail!("session creation is not implemented for this service")
    }

    async fn duplicate_session(
        &self,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebSessionDetail> {
        bail!("session duplication is not implemented for this service")
    }

    async fn get_weixin_account(&self) -> Result<WebWeixinAccount> {
        bail!("weixin account lookup is not implemented for this service")
    }

    async fn start_weixin_login(&self) -> Result<WeixinLoginStartResponse> {
        bail!("weixin login start is not implemented for this service")
    }

    async fn poll_weixin_login(&self) -> Result<WebWeixinLoginStatus> {
        bail!("weixin login status is not implemented for this service")
    }

    async fn logout_weixin(&self) -> Result<WebWeixinAccount> {
        bail!("weixin logout is not implemented for this service")
    }

    async fn list_profiles(&self) -> Result<Vec<String>> {
        bail!("profile listing is not implemented for this service")
    }

    async fn set_session_profile(
        &self,
        _channel: &str,
        _session_id: &str,
        _profile: &str,
    ) -> Result<()> {
        bail!("profile setting is not implemented for this service")
    }

    async fn delete_session(&self, _channel: &str, _session_id: &str) -> Result<bool> {
        bail!("session deletion is not implemented for this service")
    }

    async fn list_mcp_servers(&self) -> Result<Vec<McpServerInfo>> {
        Ok(Vec::new())
    }

    async fn toggle_mcp_tool(&self, _name: &str, _enabled: bool) -> Result<bool> {
        bail!("toggle mcp tool is not implemented for this service")
    }

    async fn apply_mcp_server_action(
        &self,
        _name: &str,
        _action: McpServerToolAction,
    ) -> Result<bool> {
        bail!("mcp server tool action is not implemented for this service")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebChatReply {
    pub reply: String,
    pub active_profile: String,
    pub persisted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebSessionSummary {
    pub channel: String,
    pub session_id: String,
    pub updated_at: DateTime<Utc>,
    pub active_profile: String,
    pub preview: Option<String>,
    pub read_only: bool,
    pub can_send: bool,
    pub can_duplicate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebSessionGroup {
    pub channel: String,
    pub sessions: Vec<WebSessionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebToolCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebTranscriptMessage {
    pub kind: String,
    pub role: String,
    pub content: String,
    pub timestamp: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub content_html: Option<String>,
    #[serde(default)]
    pub pending: bool,
    #[serde(default)]
    pub stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<WebToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebSessionDetail {
    pub channel: String,
    pub session_id: String,
    pub updated_at: DateTime<Utc>,
    pub active_profile: String,
    pub messages: Vec<WebTranscriptMessage>,
    pub read_only: bool,
    pub can_send: bool,
    pub can_duplicate: bool,
    pub source_session_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebWeixinAccount {
    pub enabled: bool,
    pub logged_in: bool,
    pub expired: bool,
    pub bot_id: Option<String>,
    pub user_id: Option<String>,
    pub base_url: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl WebWeixinAccount {
    pub fn from_account(enabled: bool, account: Option<&WeixinAccountState>) -> Self {
        match account {
            Some(account) => Self {
                enabled,
                logged_in: account.is_logged_in(),
                expired: account.is_expired(),
                bot_id: Some(account.ilink_bot_id.clone()),
                user_id: account.ilink_user_id.clone(),
                base_url: Some(account.baseurl.clone()),
                updated_at: Some(account.updated_at),
            },
            None => Self {
                enabled,
                logged_in: false,
                expired: false,
                bot_id: None,
                user_id: None,
                base_url: None,
                updated_at: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WeixinLoginStartResponse {
    pub qrcode: String,
    pub qrcode_img_content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebWeixinLoginStatus {
    pub status: String,
    pub logged_in: bool,
    pub expired: bool,
}

impl WebWeixinLoginStatus {
    pub fn from_state(status: Option<&str>, account: Option<&WeixinAccountState>) -> Self {
        let logged_in = account.is_some_and(WeixinAccountState::is_logged_in);
        let expired = account.is_some_and(WeixinAccountState::is_expired)
            || status.is_some_and(|status| status.eq_ignore_ascii_case("expired"));
        let status = status
            .map(ToString::to_string)
            .unwrap_or_else(|| if logged_in { "confirmed" } else { "failed" }.to_string());
        Self {
            status,
            logged_in,
            expired,
        }
    }
}

#[derive(Debug, Clone)]
struct WeixinWebConfig {
    enabled: bool,
    api_base: String,
}

impl From<&WeixinConfig> for WeixinWebConfig {
    fn from(config: &WeixinConfig) -> Self {
        Self {
            enabled: config.enabled,
            api_base: config.api_base.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeixinWorkflowErrorKind {
    Disabled,
    LoginNotStarted,
    InitFailed,
}

#[derive(Debug, Clone)]
pub struct WeixinWorkflowError {
    kind: WeixinWorkflowErrorKind,
    message: String,
}

impl WeixinWorkflowError {
    fn disabled() -> Self {
        Self {
            kind: WeixinWorkflowErrorKind::Disabled,
            message: "weixin runtime is disabled".to_string(),
        }
    }

    fn login_not_started() -> Self {
        Self {
            kind: WeixinWorkflowErrorKind::LoginNotStarted,
            message: "weixin login has not been started".to_string(),
        }
    }

    fn init_failed(error: impl fmt::Display) -> Self {
        Self {
            kind: WeixinWorkflowErrorKind::InitFailed,
            message: format!("failed to initialize weixin web runtime: {error}"),
        }
    }

    pub fn kind(&self) -> WeixinWorkflowErrorKind {
        self.kind
    }
}

impl fmt::Display for WeixinWorkflowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl StdError for WeixinWorkflowError {}

#[derive(Clone)]
pub struct AppState {
    pub(crate) chat: Option<Arc<dyn ChatService>>,
    pub(crate) cron: Option<Arc<CronService>>,
    pub(crate) auth: Option<AuthService>,
    pub(crate) control: Option<ControlStore>,
    pub(crate) runtimes: Option<RuntimeManager>,
}

impl AppState {
    pub fn new(chat: Arc<dyn ChatService>, cron: Option<Arc<CronService>>) -> Self {
        Self {
            chat: Some(chat),
            cron,
            auth: None,
            control: None,
            runtimes: None,
        }
    }

    pub fn with_control(control: ControlStore, runtimes: RuntimeManager) -> Self {
        Self {
            chat: None,
            cron: None,
            auth: Some(AuthService::new(control.clone())),
            control: Some(control),
            runtimes: Some(runtimes),
        }
    }

    pub fn auth_service(&self) -> Option<&AuthService> {
        self.auth.as_ref()
    }

    pub fn control_store(&self) -> Option<&ControlStore> {
        self.control.as_ref()
    }

    pub fn auth_enabled(&self) -> bool {
        self.auth.is_some()
    }

    pub async fn runtime_for_user(
        &self,
        user: Option<&AuthenticatedUser>,
    ) -> Result<Arc<crate::control::UserRuntime>> {
        let user = user.ok_or_else(|| anyhow::anyhow!("authentication required"))?;
        let runtimes = self
            .runtimes
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("runtime manager is not configured"))?;
        runtimes.get_or_start(&user.user_id).await
    }

    pub async fn cron_for_user(
        &self,
        user: Option<&AuthenticatedUser>,
    ) -> Result<Arc<CronService>> {
        if let Some(user) = user {
            return Ok(self.runtime_for_user(Some(user)).await?.cron());
        }
        self.cron
            .clone()
            .ok_or_else(|| anyhow::anyhow!("cron service is not configured"))
    }

    pub async fn chat_for_user(
        &self,
        user: Option<&AuthenticatedUser>,
    ) -> Result<Arc<dyn ChatService>> {
        if let Some(user) = user {
            let runtime = self.runtime_for_user(Some(user)).await?;
            return Ok(Arc::new(AgentChatService::new(runtime.agent().clone())));
        }
        self.chat
            .clone()
            .ok_or_else(|| anyhow::anyhow!("chat service is not configured"))
    }

    pub fn workspace_for_user(&self, user: Option<&AuthenticatedUser>) -> Result<PathBuf> {
        if let Some(user) = user {
            if let Some(control) = &self.control {
                return Ok(control.user_workspace_path(&user.user_id));
            }
            return Err(anyhow::anyhow!("control store is not configured"));
        }
        self.chat
            .as_ref()
            .and_then(|chat| chat.workspace_path())
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow::anyhow!("workspace path is not configured"))
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(page::index_handler))
        .route("/assets/{*path}", get(page::static_handler))
        .route("/healthz", get(api::healthz))
        .route("/api/auth/login", post(api::login))
        .route("/api/auth/logout", post(api::logout))
        .route("/api/auth/me", get(api::me))
        .route("/api/auth/change-password", post(api::change_password))
        .route(
            "/api/me/config",
            get(api::get_my_config).put(api::put_my_config),
        )
        .route(
            "/api/admin/users",
            get(api::list_admin_users).post(api::create_admin_user),
        )
        .route("/api/admin/users/{id}/enable", post(api::enable_admin_user))
        .route(
            "/api/admin/users/{id}/disable",
            post(api::disable_admin_user),
        )
        .route(
            "/api/admin/users/{id}/password",
            post(api::set_admin_user_password),
        )
        .route("/api/admin/users/{id}/role", post(api::set_admin_user_role))
        .route(
            "/api/sessions",
            get(api::list_sessions).post(api::create_session),
        )
        .route("/api/sessions/duplicate", post(api::duplicate_session))
        .route(
            "/api/sessions/{channel}/{id}",
            get(api::get_session).delete(api::delete_session),
        )
        .route(
            "/api/sessions/{channel}/{id}/profile",
            post(api::set_session_profile),
        )
        .route("/api/profiles", get(api::list_profiles))
        .route("/api/weixin/account", get(api::get_weixin_account))
        .route("/api/weixin/login/start", post(api::start_weixin_login))
        .route("/api/weixin/login/status", get(api::poll_weixin_login))
        .route("/api/weixin/logout", post(api::logout_weixin))
        .route("/api/chat", post(api::chat))
        .route(
            "/api/cron/jobs",
            get(api::list_cron_jobs).post(api::add_cron_job),
        )
        .route(
            "/api/cron/jobs/{id}",
            axum::routing::delete(api::delete_cron_job),
        )
        .route("/api/cron/jobs/{id}/toggle", post(api::toggle_cron_job))
        .route("/api/cron/jobs/{id}/run", post(api::run_cron_job))
        .route("/api/mcp/servers", get(api::list_mcp_servers))
        .route("/api/mcp/tools/{name}/toggle", post(api::toggle_mcp_tool))
        .route(
            "/api/mcp/servers/{name}/tools/bulk",
            post(api::apply_mcp_server_action),
        )
        .route("/api/skills", get(api::list_skills))
        .route("/api/skills/{source}/{id}", get(api::get_skill))
        .route("/api/skills/workspace", post(api::create_workspace_skill))
        .route(
            "/api/skills/workspace/{id}",
            axum::routing::put(api::update_workspace_skill).delete(api::delete_workspace_skill),
        )
        .route(
            "/api/skills/workspace/{id}/state",
            axum::routing::put(api::update_workspace_skill_state),
        )
        .with_state(state)
}

#[derive(Clone)]
struct WeixinRuntime {
    config: WeixinWebConfig,
    store: WeixinAccountStore,
    login: WeixinLoginManager,
}

impl WeixinRuntime {
    fn new(workspace: PathBuf, config: WeixinWebConfig) -> Result<Self> {
        let store = WeixinAccountStore::new(&workspace)?;
        let login = WeixinLoginManager::new(
            config.api_base.clone(),
            store.clone(),
            env!("CARGO_PKG_VERSION"),
        );
        Ok(Self {
            config,
            store,
            login,
        })
    }

    async fn get_account(&self) -> Result<WebWeixinAccount> {
        let account = self.store.load_account()?;
        Ok(WebWeixinAccount::from_account(
            self.config.enabled,
            account.as_ref(),
        ))
    }

    async fn start_login(&self) -> Result<WeixinLoginStartResponse> {
        if !self.config.enabled {
            return Err(anyhow::Error::new(WeixinWorkflowError::disabled()));
        }
        let login = self.login.start_login().await?;
        let qrcode_img_content = {
            let normalized = normalize_weixin_qr_image_source(&login.qrcode_img_content);
            if normalized.is_empty() {
                qr_text_to_svg_data_url(&login.qrcode)
            } else {
                normalized
            }
        };
        Ok(WeixinLoginStartResponse {
            qrcode: login.qrcode,
            qrcode_img_content,
        })
    }

    async fn poll_login(&self) -> Result<WebWeixinLoginStatus> {
        if !self.config.enabled {
            return Err(anyhow::Error::new(WeixinWorkflowError::disabled()));
        }
        let status = match self.login.poll_login_status().await {
            Ok(status) => status,
            Err(error)
                if error
                    .to_string()
                    .contains("weixin login has not been started") =>
            {
                return Err(anyhow::Error::new(WeixinWorkflowError::login_not_started()));
            }
            Err(error) => return Err(error),
        };
        let account = self.store.load_account()?;
        Ok(WebWeixinLoginStatus::from_state(
            Some(status.status.as_str()),
            account.as_ref(),
        ))
    }

    async fn logout(&self) -> Result<WebWeixinAccount> {
        self.store.clear_all()?;
        self.login.clear_login_session()?;
        Ok(WebWeixinAccount::from_account(self.config.enabled, None))
    }
}

#[derive(Clone)]
enum WeixinServiceState {
    Disabled(WeixinWebConfig),
    Ready(WeixinRuntime),
    InitFailed { error: Arc<str> },
}

impl WeixinServiceState {
    fn new(workspace: PathBuf, config: WeixinWebConfig) -> Self {
        if !config.enabled {
            return Self::Disabled(config);
        }

        match WeixinRuntime::new(workspace, config.clone()) {
            Ok(runtime) => Self::Ready(runtime),
            Err(error) => {
                error!(error = %error, "failed to initialize weixin web runtime");
                Self::InitFailed {
                    error: Arc::<str>::from(error.to_string()),
                }
            }
        }
    }

    fn account_placeholder(config: &WeixinWebConfig) -> WebWeixinAccount {
        WebWeixinAccount::from_account(config.enabled, None)
    }

    fn init_failed_error(error: &Arc<str>) -> anyhow::Error {
        anyhow::Error::new(WeixinWorkflowError::init_failed(error.as_ref()))
    }

    fn runtime(&self) -> Result<&WeixinRuntime> {
        match self {
            Self::Disabled(_) => Err(anyhow::Error::new(WeixinWorkflowError::disabled())),
            Self::Ready(runtime) => Ok(runtime),
            Self::InitFailed { error } => Err(Self::init_failed_error(error)),
        }
    }

    async fn account_when_unavailable(&self) -> Result<WebWeixinAccount> {
        match self {
            Self::Disabled(config) => Ok(Self::account_placeholder(config)),
            Self::Ready(runtime) => runtime.get_account().await,
            Self::InitFailed { error } => Err(Self::init_failed_error(error)),
        }
    }

    async fn account_after_logout(&self) -> Result<WebWeixinAccount> {
        match self {
            Self::Disabled(config) => Ok(Self::account_placeholder(config)),
            Self::Ready(runtime) => runtime.logout().await,
            Self::InitFailed { error } => Err(Self::init_failed_error(error)),
        }
    }
}

#[derive(Clone)]
pub struct AgentChatService {
    agent: AgentLoop,
    weixin: WeixinServiceState,
}

impl AgentChatService {
    pub fn new(agent: AgentLoop) -> Self {
        let weixin_config = WeixinWebConfig::from(agent.weixin_web_config());
        let weixin = WeixinServiceState::new(agent.workspace_path().to_path_buf(), weixin_config);
        Self { agent, weixin }
    }
}

#[async_trait]
impl ChatService for AgentChatService {
    async fn chat(&self, message: &str, channel: &str, session_id: &str) -> Result<WebChatReply> {
        if channel != WEB_NAMESPACE {
            bail!("session is read-only; duplicate it into web before sending");
        }
        let session_key = session_key(channel, session_id);
        info!(
            session = %session_id,
            preview = %preview(message),
            "web session {session_id} started"
        );
        let result = self
            .agent
            .process_direct_result_logged(message, &session_key, channel, session_id)
            .await;
        match &result {
            Ok(reply) => {
                info!(
                    session = %session_id,
                    preview = %preview(&reply.reply),
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
            reply: reply.reply,
            active_profile,
            persisted: reply.persisted,
        })
    }

    async fn list_sessions(&self) -> Result<Vec<WebSessionGroup>> {
        let groups = self
            .agent
            .list_sessions_grouped_by_channel()?
            .into_iter()
            .map(|group| group_from_sessions(self, group))
            .collect::<Vec<_>>();
        Ok(sort_groups(groups))
    }

    async fn get_session(
        &self,
        channel: &str,
        session_id: &str,
    ) -> Result<Option<WebSessionDetail>> {
        let session = self
            .agent
            .load_session_by_key(&session_key(channel, session_id))?;
        Ok(session.map(|session| detail_from_session(self, session)))
    }

    async fn create_session(&self) -> Result<WebSessionSummary> {
        let session_id = Uuid::new_v4().to_string();
        let session = self
            .agent
            .create_session(&session_key(WEB_NAMESPACE, &session_id))?;
        Ok(summary_from_full_session(self, session))
    }

    async fn duplicate_session(&self, channel: &str, session_id: &str) -> Result<WebSessionDetail> {
        let session = self
            .agent
            .duplicate_session_to_web(&session_key(channel, session_id))?;
        Ok(detail_from_session(self, session))
    }

    async fn get_weixin_account(&self) -> Result<WebWeixinAccount> {
        self.weixin.account_when_unavailable().await
    }

    async fn start_weixin_login(&self) -> Result<WeixinLoginStartResponse> {
        self.weixin.runtime()?.start_login().await
    }

    async fn poll_weixin_login(&self) -> Result<WebWeixinLoginStatus> {
        self.weixin.runtime()?.poll_login().await
    }

    async fn logout_weixin(&self) -> Result<WebWeixinAccount> {
        self.weixin.account_after_logout().await
    }

    async fn list_profiles(&self) -> Result<Vec<String>> {
        Ok(self.agent.list_profiles())
    }

    async fn set_session_profile(
        &self,
        channel: &str,
        session_id: &str,
        profile: &str,
    ) -> Result<()> {
        self.agent
            .set_session_profile(&session_key(channel, session_id), profile)
    }

    async fn delete_session(&self, channel: &str, session_id: &str) -> Result<bool> {
        self.agent.delete_session(&session_key(channel, session_id))
    }

    async fn list_mcp_servers(&self) -> Result<Vec<McpServerInfo>> {
        Ok(self.agent.list_mcp_servers().await)
    }

    async fn toggle_mcp_tool(&self, name: &str, enabled: bool) -> Result<bool> {
        self.agent.toggle_mcp_tool(name, enabled).await
    }

    async fn apply_mcp_server_action(
        &self,
        name: &str,
        action: McpServerToolAction,
    ) -> Result<bool> {
        self.agent.apply_mcp_server_action(name, action).await
    }

    fn workspace_path(&self) -> Option<&Path> {
        Some(self.agent.workspace_path())
    }
}

pub async fn serve(agent: AgentLoop, host: &str, port: u16) -> Result<()> {
    let cron = agent.cron_service().await;
    let state = AppState::new(Arc::new(AgentChatService::new(agent)), cron);
    let listener = TcpListener::bind(format!("{host}:{port}")).await?;
    axum::serve(listener, build_router(state)).await?;
    Ok(())
}

pub async fn serve_control(
    control: ControlStore,
    runtimes: RuntimeManager,
    host: &str,
    port: u16,
) -> Result<()> {
    let state = AppState::with_control(control, runtimes);
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

fn session_key(channel: &str, session_id: &str) -> String {
    format!("{channel}:{session_id}")
}

fn summary_from_session(service: &AgentChatService, summary: SessionSummary) -> WebSessionSummary {
    let capabilities = capabilities_for_channel(&summary.channel);
    WebSessionSummary {
        channel: summary.channel,
        session_id: public_session_id(&summary.key),
        updated_at: summary.updated_at,
        active_profile: effective_profile(service, summary.active_profile.as_deref()),
        preview: summary.preview,
        read_only: capabilities.read_only,
        can_send: capabilities.can_send,
        can_duplicate: capabilities.can_duplicate,
    }
}

fn summary_from_full_session(service: &AgentChatService, session: Session) -> WebSessionSummary {
    let (channel, _) = split_session_key(&session.key);
    let capabilities = capabilities_for_channel(&channel);
    WebSessionSummary {
        channel,
        session_id: public_session_id(&session.key),
        updated_at: session.updated_at,
        active_profile: effective_profile(service, session.active_profile.as_deref()),
        preview: session_preview(&session.messages),
        read_only: capabilities.read_only,
        can_send: capabilities.can_send,
        can_duplicate: capabilities.can_duplicate,
    }
}

fn detail_from_session(service: &AgentChatService, session: Session) -> WebSessionDetail {
    let (channel, _) = split_session_key(&session.key);
    let capabilities = capabilities_for_channel(&channel);
    WebSessionDetail {
        channel,
        session_id: public_session_id(&session.key),
        updated_at: session.updated_at,
        active_profile: effective_profile(service, session.active_profile.as_deref()),
        messages: transcript_messages(&session.messages),
        read_only: capabilities.read_only,
        can_send: capabilities.can_send,
        can_duplicate: capabilities.can_duplicate,
        source_session_key: session.source_session_key,
    }
}

fn group_from_sessions(service: &AgentChatService, group: SessionGroupSummary) -> WebSessionGroup {
    WebSessionGroup {
        channel: group.channel,
        sessions: group
            .sessions
            .into_iter()
            .map(|summary| summary_from_session(service, summary))
            .collect(),
    }
}

#[derive(Clone, Copy)]
struct SessionCapabilities {
    read_only: bool,
    can_send: bool,
    can_duplicate: bool,
}

fn capabilities_for_channel(channel: &str) -> SessionCapabilities {
    if channel == WEB_NAMESPACE {
        SessionCapabilities {
            read_only: false,
            can_send: true,
            can_duplicate: false,
        }
    } else {
        SessionCapabilities {
            read_only: true,
            can_send: false,
            can_duplicate: true,
        }
    }
}

fn sort_groups(mut groups: Vec<WebSessionGroup>) -> Vec<WebSessionGroup> {
    groups.sort_by(|a, b| channel_sort_key(&a.channel).cmp(&channel_sort_key(&b.channel)));
    groups
}

fn channel_sort_key(channel: &str) -> (usize, &str) {
    let rank = match channel {
        "web" => 0,
        "telegram" => 1,
        "wecom" => 2,
        "weixin" => 3,
        "cli" => 4,
        "system" => 5,
        _ => 6,
    };
    (rank, channel)
}

fn transcript_messages(messages: &[SessionMessage]) -> Vec<WebTranscriptMessage> {
    let mut transcript = Vec::new();
    let mut index = 0;
    while let Some(message) = messages.get(index) {
        match message.timeline_kind() {
            Some("btw_query") => {
                let query = session_content_text(message).unwrap_or_default();
                let mut answer = String::new();
                let mut answer_html = None;
                let mut stale = false;
                let mut pending = true;
                let mut timestamp = message.timestamp;

                if let Some(next) = messages.get(index + 1) {
                    if next.timeline_kind() == Some("btw_answer")
                        && message.btw_id().is_some()
                        && message.btw_id() == next.btw_id()
                    {
                        answer = session_content_text(next).unwrap_or_default();
                        answer_html = (!answer.is_empty()).then(|| render_web_html(&answer));
                        stale = next
                            .extra
                            .get("_btw_stale")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false);
                        pending = false;
                        timestamp = next.timestamp.or(timestamp);
                        index += 1;
                    }
                }

                transcript.push(WebTranscriptMessage {
                    kind: "btw_thread".to_string(),
                    role: "btw".to_string(),
                    content: answer,
                    timestamp,
                    query: Some(query),
                    content_html: answer_html,
                    pending,
                    stale,
                    tool_calls: None,
                    tool_name: None,
                    tool_call_id: None,
                });
            }
            Some("btw_answer") => {}
            _ => {
                if let Some(entry) = transcript_message(message) {
                    transcript.push(entry);
                }
            }
        }
        index += 1;
    }
    transcript
}

fn transcript_message(message: &SessionMessage) -> Option<WebTranscriptMessage> {
    match message.role.as_str() {
        "user" => {
            let content = session_content_text(message)?;
            Some(WebTranscriptMessage {
                kind: "message".to_string(),
                role: "user".to_string(),
                content,
                timestamp: message.timestamp,
                query: None,
                content_html: None,
                pending: false,
                stale: false,
                tool_calls: None,
                tool_name: None,
                tool_call_id: None,
            })
        }
        "assistant" => {
            let content = session_content_text(message).unwrap_or_default();
            let tool_calls = message.tool_calls.as_ref().and_then(|calls| {
                let result: Vec<WebToolCall> = calls
                    .iter()
                    .filter_map(|call| {
                        let function = call
                            .get("function")
                            .and_then(serde_json::Value::as_object)
                            .cloned()
                            .unwrap_or_default();
                        let id = call
                            .get("id")
                            .or_else(|| call.get("call_id"))
                            .and_then(serde_json::Value::as_str)
                            .map(ToString::to_string);
                        let name = function
                            .get("name")
                            .or_else(|| call.get("name"))
                            .and_then(serde_json::Value::as_str)?
                            .to_string();
                        let arguments = function
                            .get("arguments")
                            .or_else(|| call.get("arguments"))
                            .and_then(stringify_web_tool_payload);
                        Some(WebToolCall {
                            id,
                            name,
                            arguments,
                        })
                    })
                    .collect();
                if result.is_empty() {
                    None
                } else {
                    Some(result)
                }
            });
            if content.is_empty() && tool_calls.is_none() {
                return None;
            }
            let content_html = if !content.is_empty() {
                Some(render_web_html(&content))
            } else {
                None
            };
            Some(WebTranscriptMessage {
                kind: "message".to_string(),
                role: "assistant".to_string(),
                content,
                timestamp: message.timestamp,
                query: None,
                content_html,
                pending: false,
                stale: false,
                tool_calls,
                tool_name: None,
                tool_call_id: None,
            })
        }
        "tool" => {
            let content = session_content_text(message).unwrap_or_default();
            let tool_name = message.name.clone().unwrap_or_else(|| "tool".to_string());
            Some(WebTranscriptMessage {
                kind: "message".to_string(),
                role: "tool".to_string(),
                content,
                timestamp: message.timestamp,
                query: None,
                content_html: None,
                pending: false,
                stale: false,
                tool_calls: None,
                tool_name: Some(tool_name),
                tool_call_id: message.tool_call_id.clone(),
            })
        }
        _ => None,
    }
}

fn stringify_web_tool_payload(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
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
            _ if message.excluded_from_context() => None,
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

fn normalize_weixin_qr_image_source(source: &str) -> String {
    let value = source.trim();
    if value.is_empty() {
        return String::new();
    }
    if value.starts_with("data:") || value.starts_with("blob:") || value.starts_with('/') {
        return value.to_string();
    }
    if looks_like_base64_image_payload(value) {
        return format!(
            "data:image/png;base64,{}",
            value.replace(char::is_whitespace, "")
        );
    }
    if looks_like_direct_image_url(value) {
        return value.to_string();
    }
    qr_text_to_svg_data_url(value)
}

fn looks_like_base64_image_payload(value: &str) -> bool {
    let compact = value.replace(char::is_whitespace, "");
    !compact.is_empty()
        && compact.len() % 4 == 0
        && compact
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '='))
}

fn looks_like_direct_image_url(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let path = url.path().to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".bmp"]
        .iter()
        .any(|suffix| path.ends_with(suffix))
}

fn qr_text_to_svg_data_url(text: &str) -> String {
    let qr = QrCode::encode_text(text, QrCodeEcc::Medium)
        .expect("weixin qr text should fit within QR capacity");
    let svg = qr_to_svg_string(&qr, 4);
    format!(
        "data:image/svg+xml;base64,{}",
        base64_encode(svg.as_bytes())
    )
}

fn qr_to_svg_string(qr: &QrCode, border: i32) -> String {
    let size = qr.size() + border * 2;
    let mut path = String::new();
    for y in 0..qr.size() {
        for x in 0..qr.size() {
            if qr.get_module(x, y) {
                let _ = write!(&mut path, "M{},{}h1v1h-1z", x + border, y + border);
            }
        }
    }

    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {size} {size}\" shape-rendering=\"crispEdges\"><rect width=\"100%\" height=\"100%\" fill=\"#fff\"/><path d=\"{path}\" fill=\"#000\"/></svg>"
    )
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut chunks = bytes.chunks_exact(3);
    for chunk in &mut chunks {
        let triple = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | chunk[2] as u32;
        out.push(TABLE[((triple >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3f) as usize] as char);
        out.push(TABLE[((triple >> 6) & 0x3f) as usize] as char);
        out.push(TABLE[(triple & 0x3f) as usize] as char);
    }
    let rem = chunks.remainder();
    if !rem.is_empty() {
        let mut triple = (rem[0] as u32) << 16;
        if rem.len() == 2 {
            triple |= (rem[1] as u32) << 8;
        }
        out.push(TABLE[((triple >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3f) as usize] as char);
        if rem.len() == 2 {
            out.push(TABLE[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        out.push('=');
    }
    out
}

fn effective_profile(service: &AgentChatService, selected: Option<&str>) -> String {
    selected
        .filter(|key| service.agent.has_profile(key))
        .unwrap_or(service.agent.default_profile())
        .to_string()
}

fn public_session_id(key: &str) -> String {
    let (_, session_id) = split_session_key(key);
    session_id
}
