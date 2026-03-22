pub mod api;
pub mod page;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Result};
use async_trait::async_trait;
use axum::{
    routing::{get, post},
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tracing::{error, info};
use uuid::Uuid;

use crate::agent::AgentLoop;
use crate::channels::weixin::{WeixinAccountState, WeixinAccountStore, WeixinLoginManager};
use crate::config::WeixinConfig;
use crate::presentation::render_web_html;
use crate::session::{
    split_session_key, Session, SessionGroupSummary, SessionMessage, SessionSummary,
};

const WEB_NAMESPACE: &str = "web";

#[async_trait]
pub trait ChatService: Send + Sync {
    async fn chat(&self, message: &str, channel: &str, session_id: &str) -> Result<WebChatReply>;

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
pub struct WebTranscriptMessage {
    pub role: String,
    pub content: String,
    pub timestamp: Option<DateTime<Utc>>,
    pub content_html: Option<String>,
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
        .route("/api/sessions/duplicate", post(api::duplicate_session))
        .route("/api/sessions/{channel}/{id}", get(api::get_session))
        .route("/api/weixin/account", get(api::get_weixin_account))
        .route("/api/weixin/login/start", post(api::start_weixin_login))
        .route("/api/weixin/login/status", get(api::poll_weixin_login))
        .route("/api/weixin/logout", post(api::logout_weixin))
        .route("/api/chat", post(api::chat))
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
            bail!("weixin runtime is not available");
        }
        let login = self.login.start_login().await?;
        Ok(WeixinLoginStartResponse {
            qrcode: login.qrcode,
            qrcode_img_content: login.qrcode_img_content,
        })
    }

    async fn poll_login(&self) -> Result<WebWeixinLoginStatus> {
        if !self.config.enabled {
            bail!("weixin runtime is not available");
        }
        let status = self.login.poll_login_status().await?;
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
pub struct AgentChatService {
    agent: AgentLoop,
    weixin_config: WeixinWebConfig,
    weixin: Option<WeixinRuntime>,
}

impl AgentChatService {
    pub fn new(agent: AgentLoop) -> Self {
        let weixin_config = WeixinWebConfig::from(agent.weixin_web_config());
        let weixin =
            match WeixinRuntime::new(agent.workspace_path().to_path_buf(), weixin_config.clone()) {
                Ok(runtime) => Some(runtime),
                Err(error) => {
                    error!(error = %error, "failed to initialize weixin web runtime");
                    None
                }
            };
        Self {
            agent,
            weixin_config,
            weixin,
        }
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
            .process_direct_logged(message, &session_key, channel, session_id)
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
        match &self.weixin {
            Some(weixin) => weixin.get_account().await,
            None => Ok(WebWeixinAccount::from_account(
                self.weixin_config.enabled,
                None,
            )),
        }
    }

    async fn start_weixin_login(&self) -> Result<WeixinLoginStartResponse> {
        match &self.weixin {
            Some(weixin) => weixin.start_login().await,
            None => Err(anyhow::anyhow!("weixin runtime is not available")),
        }
    }

    async fn poll_weixin_login(&self) -> Result<WebWeixinLoginStatus> {
        match &self.weixin {
            Some(weixin) => weixin.poll_login().await,
            None => Err(anyhow::anyhow!("weixin runtime is not available")),
        }
    }

    async fn logout_weixin(&self) -> Result<WebWeixinAccount> {
        match &self.weixin {
            Some(weixin) => weixin.logout().await,
            None => Ok(WebWeixinAccount::from_account(
                self.weixin_config.enabled,
                None,
            )),
        }
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
        messages: session
            .messages
            .iter()
            .filter_map(transcript_message)
            .collect(),
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
        "cli" => 3,
        "system" => 4,
        _ => 5,
    };
    (rank, channel)
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
    let (_, session_id) = split_session_key(key);
    session_id
}
