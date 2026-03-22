use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use axum::Router;
use chrono::{Duration, Utc};
use nanobot_rs::agent::AgentLoop;
use nanobot_rs::bus::MessageBus;
use nanobot_rs::channels::weixin::{WeixinAccountState, WeixinAccountStore};
use nanobot_rs::config::{AgentProfileConfig, Config, WebToolsConfig};
use nanobot_rs::providers::{LlmProvider, LlmResponse, ToolCall};
use nanobot_rs::session::{Session, SessionMessage, SessionStore};
use nanobot_rs::web::{
    self, AgentChatService, AppState, ChatService, WebChatReply, WebSessionDetail,
};
use serde_json::{json, Map};
use std::collections::VecDeque;
use tempfile::{tempdir, TempDir};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

#[derive(Clone, Default)]
struct StaticChatService;

#[async_trait]
impl ChatService for StaticChatService {
    async fn chat(
        &self,
        _message: &str,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebChatReply> {
        Ok(WebChatReply {
            reply: "unused".to_string(),
            active_profile: "openai:mock-model".to_string(),
        })
    }

    async fn duplicate_session(
        &self,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebSessionDetail> {
        anyhow::bail!("unused")
    }
}

fn test_state() -> AppState {
    AppState::new(Arc::new(StaticChatService))
}

#[derive(Clone)]
struct ReplyChatService {
    reply: String,
}

#[async_trait]
impl ChatService for ReplyChatService {
    async fn chat(
        &self,
        _message: &str,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebChatReply> {
        Ok(WebChatReply {
            reply: self.reply.clone(),
            active_profile: "openai:mock-model".to_string(),
        })
    }

    async fn duplicate_session(
        &self,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebSessionDetail> {
        anyhow::bail!("unused")
    }
}

fn test_state_with_reply(reply: &str) -> AppState {
    AppState::new(Arc::new(ReplyChatService {
        reply: reply.to_string(),
    }))
}

#[derive(Clone)]
struct ErrorChatService;

#[async_trait]
impl ChatService for ErrorChatService {
    async fn chat(
        &self,
        _message: &str,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebChatReply> {
        anyhow::bail!("provider exploded")
    }

    async fn duplicate_session(
        &self,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebSessionDetail> {
        anyhow::bail!("unused")
    }
}

fn test_state_with_error() -> AppState {
    AppState::new(Arc::new(ErrorChatService))
}

#[derive(Clone)]
struct WeixinLoginSnapshot {
    qrcode: String,
    qrcode_img_content: String,
    status: String,
}

struct WeixinAccountChatService {
    enabled: bool,
    store: WeixinAccountStore,
    _workspace: TempDir,
    login: Arc<Mutex<Option<WeixinLoginSnapshot>>>,
}

impl WeixinAccountChatService {
    fn new(enabled: bool, workspace: TempDir, store: WeixinAccountStore) -> Self {
        Self {
            enabled,
            store,
            _workspace: workspace,
            login: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl ChatService for WeixinAccountChatService {
    async fn chat(
        &self,
        _message: &str,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebChatReply> {
        anyhow::bail!("unused")
    }

    async fn duplicate_session(
        &self,
        _channel: &str,
        _session_id: &str,
    ) -> Result<WebSessionDetail> {
        anyhow::bail!("unused")
    }

    async fn get_weixin_account(&self) -> Result<web::WebWeixinAccount> {
        let account = self.store.load_account()?;
        Ok(web::WebWeixinAccount::from_account(
            self.enabled,
            account.as_ref(),
        ))
    }

    async fn start_weixin_login(&self) -> Result<web::WeixinLoginStartResponse> {
        let login = WeixinLoginSnapshot {
            qrcode: "qr-token".to_string(),
            qrcode_img_content: "data:image/png;base64,abc".to_string(),
            status: "wait".to_string(),
        };
        *self.login.lock().await = Some(login.clone());
        Ok(web::WeixinLoginStartResponse {
            qrcode: login.qrcode,
            qrcode_img_content: login.qrcode_img_content,
        })
    }

    async fn poll_weixin_login(&self) -> Result<web::WebWeixinLoginStatus> {
        let login = self.login.lock().await.clone();
        let account = self.store.load_account()?;
        Ok(web::WebWeixinLoginStatus::from_state(
            login.as_ref().map(|snapshot| snapshot.status.as_str()),
            account.as_ref(),
        ))
    }

    async fn logout_weixin(&self) -> Result<web::WebWeixinAccount> {
        self.store.clear_all()?;
        *self.login.lock().await = None;
        Ok(web::WebWeixinAccount::from_account(self.enabled, None))
    }
}

async fn build_test_router_with_weixin_account_state(account: WeixinAccountState) -> Router {
    let dir = tempdir().expect("tempdir");
    let store = WeixinAccountStore::new(dir.path()).expect("weixin store");
    store.save_account(&account).expect("save account");
    web::build_router(AppState::new(Arc::new(WeixinAccountChatService::new(
        true, dir, store,
    ))))
}

fn sample_weixin_account() -> WeixinAccountState {
    WeixinAccountState {
        bot_token: "bot-token".to_string(),
        ilink_bot_id: "bot@im.bot".to_string(),
        baseurl: "https://ilinkai.weixin.qq.com".to_string(),
        ilink_user_id: Some("user@im.wechat".to_string()),
        get_updates_buf: "cursor-1".to_string(),
        longpolling_timeout_ms: 35_000,
        status: "active".to_string(),
        updated_at: Utc::now(),
    }
}

#[derive(Clone)]
struct MockProvider {
    model: String,
    responses: Arc<Mutex<VecDeque<LlmResponse>>>,
}

#[async_trait]
impl LlmProvider for MockProvider {
    fn default_model(&self) -> &str {
        &self.model
    }

    async fn chat(
        &self,
        _messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        _model: &str,
    ) -> Result<LlmResponse> {
        self.responses
            .lock()
            .await
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("no more responses"))
    }
}

fn mock_provider(responses: Vec<LlmResponse>) -> Arc<dyn LlmProvider> {
    Arc::new(MockProvider {
        model: "mock-model".to_string(),
        responses: Arc::new(Mutex::new(responses.into())),
    })
}

async fn spawn_test_server(app: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    addr
}

async fn agent_app(dir: &TempDir, responses: Vec<LlmResponse>) -> Router {
    let provider = mock_provider(responses);
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus,
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    )
    .await
    .expect("agent");
    web::build_router(AppState::new(Arc::new(AgentChatService::new(agent))))
}

async fn agent_app_with_profiles(
    dir: &TempDir,
    responses: Vec<LlmResponse>,
    extra_profiles: &[(&str, &str, &str)],
) -> Router {
    let provider = mock_provider(responses);
    let bus = MessageBus::new(32);
    let mut config = Config::default();
    config.agents.defaults.workspace = dir.path().display().to_string();
    config.agents.defaults.default_profile = "openai:mock-model".to_string();
    config.agents.defaults.provider = "openai".to_string();
    config.agents.defaults.model = "mock-model".to_string();
    config.agents.profiles.insert(
        "openai:mock-model".to_string(),
        AgentProfileConfig {
            provider: "openai".to_string(),
            model: "mock-model".to_string(),
            request: Map::new(),
        },
    );
    for (key, provider_name, model_name) in extra_profiles {
        config.agents.profiles.insert(
            (*key).to_string(),
            AgentProfileConfig {
                provider: (*provider_name).to_string(),
                model: (*model_name).to_string(),
                request: Map::new(),
            },
        );
    }
    let agent = AgentLoop::from_config(bus, provider, config)
        .await
        .expect("agent");
    web::build_router(AppState::new(Arc::new(AgentChatService::new(agent))))
}

fn save_session(workspace: &Path, session: &Session) {
    SessionStore::new(workspace)
        .expect("session store")
        .save(session)
        .expect("save session");
}

fn text_message(role: &str, content: &str) -> SessionMessage {
    SessionMessage {
        role: role.to_string(),
        content: json!(content),
        timestamp: Some(Utc::now()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Map::new(),
    }
}

#[tokio::test]
async fn root_and_health_routes_respond() {
    let app = web::build_router(test_state());
    let addr = spawn_test_server(app).await;

    let html = reqwest::get(format!("http://{addr}/"))
        .await
        .expect("fetch root")
        .text()
        .await
        .expect("root body");
    let health = reqwest::get(format!("http://{addr}/healthz"))
        .await
        .expect("fetch health")
        .text()
        .await
        .expect("health body");

    assert!(html.to_ascii_lowercase().contains("<!doctype html>"));
    assert_eq!(health, "ok");
}

#[tokio::test]
async fn chat_endpoint_returns_agent_reply() {
    let app = web::build_router(test_state_with_reply("**hello** from agent"));
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/chat"))
        .json(&serde_json::json!({
            "message": "hi",
            "sessionId": "browser-session-1"
        }))
        .send()
        .await
        .expect("send chat request")
        .json()
        .await
        .expect("chat response body");

    assert_eq!(response["reply"], "**hello** from agent");
    assert!(response["replyHtml"]
        .as_str()
        .unwrap_or_default()
        .contains("<strong>hello</strong>"));
    assert_eq!(response["channel"], "web");
    assert_eq!(response["sessionId"], "browser-session-1");
}

#[tokio::test]
async fn chat_endpoint_rejects_blank_messages() {
    let app = web::build_router(test_state_with_reply("should not be used"));
    let addr = spawn_test_server(app).await;

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/chat"))
        .json(&serde_json::json!({
            "message": "   ",
            "sessionId": "browser-session-2"
        }))
        .send()
        .await
        .expect("send blank chat request");
    let status = response.status();
    let payload: serde_json::Value = response.json().await.expect("blank chat response");

    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST);
    assert!(payload["error"]
        .as_str()
        .unwrap_or_default()
        .contains("message must not be empty"));
}

#[tokio::test]
async fn chat_endpoint_returns_internal_error_for_web_session_service_failures() {
    let app = web::build_router(test_state_with_error());
    let addr = spawn_test_server(app).await;

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/chat"))
        .json(&serde_json::json!({
            "message": "hi",
            "sessionId": "browser-session-error"
        }))
        .send()
        .await
        .expect("send chat request");
    let status = response.status();
    let payload: serde_json::Value = response.json().await.expect("chat error payload");

    assert_eq!(status, reqwest::StatusCode::INTERNAL_SERVER_ERROR);
    assert!(payload["error"]
        .as_str()
        .unwrap_or_default()
        .contains("provider exploded"));
}

#[tokio::test]
async fn chat_endpoint_returns_message_tool_reply() {
    let dir = tempdir().expect("tempdir");
    let app = agent_app(
        &dir,
        vec![
            LlmResponse {
                content: Some("sending".to_string()),
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "message".to_string(),
                    arguments: json!({
                        "content": "Hi from the message tool"
                    }),
                }],
                finish_reason: "tool_calls".to_string(),
                extra: Map::new(),
            },
            LlmResponse {
                content: Some("done".to_string()),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
                extra: Map::new(),
            },
        ],
    )
    .await;
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/chat"))
        .json(&serde_json::json!({
            "message": "hi",
            "sessionId": "browser-session-message"
        }))
        .send()
        .await
        .expect("send chat request")
        .json()
        .await
        .expect("chat response body");

    assert_eq!(response["reply"], "Hi from the message tool");
    assert_eq!(response["replyHtml"], "<p>Hi from the message tool</p>\n");
}

#[tokio::test]
async fn sessions_endpoint_returns_channel_grouped_results_with_stable_order_and_capabilities() {
    let dir = tempdir().expect("tempdir");
    let mut recent = Session::new("web:recent");
    recent.active_profile = Some("openrouter:deepseek-r1".to_string());
    recent.messages = vec![
        text_message("user", "hi"),
        text_message("assistant", "Most recent assistant reply"),
    ];
    recent.created_at = Utc::now() - Duration::minutes(10);
    recent.updated_at = Utc::now() - Duration::minutes(1);
    save_session(dir.path(), &recent);

    let mut telegram = Session::new("telegram:chat-1");
    telegram.active_profile = Some("openai:gpt-4.1-mini".to_string());
    telegram.messages = vec![text_message("assistant", "Telegram transcript")];
    telegram.created_at = Utc::now() - Duration::minutes(9);
    telegram.updated_at = Utc::now() - Duration::minutes(2);
    save_session(dir.path(), &telegram);

    let mut wecom = Session::new("wecom:room-2");
    wecom.active_profile = Some("openai:gpt-4.1-mini".to_string());
    wecom.messages = vec![text_message("assistant", "WeCom transcript")];
    wecom.created_at = Utc::now() - Duration::minutes(8);
    wecom.updated_at = Utc::now() - Duration::minutes(3);
    save_session(dir.path(), &wecom);

    let mut cli = Session::new("cli:terminal-3");
    cli.active_profile = Some("openai:gpt-4.1-mini".to_string());
    cli.messages = vec![text_message("assistant", "CLI transcript")];
    cli.created_at = Utc::now() - Duration::minutes(7);
    cli.updated_at = Utc::now() - Duration::minutes(4);
    save_session(dir.path(), &cli);

    let mut system = Session::new("system:job-4");
    system.active_profile = Some("openai:gpt-4.1-mini".to_string());
    system.messages = vec![text_message("assistant", "System transcript")];
    system.created_at = Utc::now() - Duration::minutes(6);
    system.updated_at = Utc::now() - Duration::minutes(5);
    save_session(dir.path(), &system);

    let mut alpha = Session::new("alpha:item-5");
    alpha.active_profile = Some("openai:gpt-4.1-mini".to_string());
    alpha.messages = vec![text_message("assistant", "Alpha transcript")];
    alpha.created_at = Utc::now() - Duration::minutes(5);
    alpha.updated_at = Utc::now() - Duration::minutes(6);
    save_session(dir.path(), &alpha);

    let mut zeta = Session::new("zeta:item-6");
    zeta.active_profile = Some("openai:gpt-4.1-mini".to_string());
    zeta.messages = vec![text_message("assistant", "Zeta transcript")];
    zeta.created_at = Utc::now() - Duration::minutes(4);
    zeta.updated_at = Utc::now() - Duration::minutes(7);
    save_session(dir.path(), &zeta);

    let app = agent_app_with_profiles(
        &dir,
        Vec::new(),
        &[(
            "openrouter:deepseek-r1",
            "openrouter",
            "deepseek/deepseek-r1",
        )],
    )
    .await;
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::get(format!("http://{addr}/api/sessions"))
        .await
        .expect("fetch sessions")
        .json()
        .await
        .expect("sessions payload");

    let groups = response["groups"].as_array().expect("groups array");
    let channels = groups
        .iter()
        .map(|group| {
            group["channel"]
                .as_str()
                .expect("group channel")
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        channels,
        vec!["web", "telegram", "wecom", "cli", "system", "alpha", "zeta"]
    );

    let web_sessions = groups[0]["sessions"].as_array().expect("web sessions");
    assert_eq!(web_sessions.len(), 1);
    assert_eq!(web_sessions[0]["sessionId"], "recent");
    assert_eq!(web_sessions[0]["channel"], "web");
    assert_eq!(web_sessions[0]["activeProfile"], "openrouter:deepseek-r1");
    assert_eq!(web_sessions[0]["preview"], "Most recent assistant reply");
    assert_eq!(web_sessions[0]["readOnly"], false);
    assert_eq!(web_sessions[0]["canSend"], true);
    assert_eq!(web_sessions[0]["canDuplicate"], false);

    let telegram_sessions = groups[1]["sessions"].as_array().expect("telegram sessions");
    assert_eq!(telegram_sessions.len(), 1);
    assert_eq!(telegram_sessions[0]["sessionId"], "chat-1");
    assert_eq!(telegram_sessions[0]["channel"], "telegram");
    assert_eq!(telegram_sessions[0]["readOnly"], true);
    assert_eq!(telegram_sessions[0]["canSend"], false);
    assert_eq!(telegram_sessions[0]["canDuplicate"], true);
}

#[tokio::test]
async fn session_detail_endpoint_returns_channel_capabilities_and_source_session_key() {
    let dir = tempdir().expect("tempdir");
    let mut session = Session::new("telegram:focus");
    session.active_profile = Some("openrouter:deepseek-r1".to_string());
    session.source_session_key = Some("wecom:origin-room".to_string());
    session.messages = vec![
        text_message("user", "hello"),
        text_message("assistant", "**hi** back"),
    ];
    save_session(dir.path(), &session);

    let app = agent_app_with_profiles(
        &dir,
        Vec::new(),
        &[(
            "openrouter:deepseek-r1",
            "openrouter",
            "deepseek/deepseek-r1",
        )],
    )
    .await;
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value =
        reqwest::get(format!("http://{addr}/api/sessions/telegram/focus"))
            .await
            .expect("fetch session detail")
            .json()
            .await
            .expect("detail payload");

    assert_eq!(response["sessionId"], "focus");
    assert_eq!(response["channel"], "telegram");
    assert_eq!(response["activeProfile"], "openrouter:deepseek-r1");
    assert_eq!(response["readOnly"], true);
    assert_eq!(response["canSend"], false);
    assert_eq!(response["canDuplicate"], true);
    assert_eq!(response["sourceSessionKey"], "wecom:origin-room");
    assert!(response["updatedAt"].as_str().is_some());
    let messages = response["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "hello");
    assert!(messages[0]["timestamp"].as_str().is_some());
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"], "**hi** back");
    assert!(messages[1]["contentHtml"]
        .as_str()
        .unwrap_or_default()
        .contains("<strong>hi</strong>"));
}

#[tokio::test]
async fn create_session_endpoint_initializes_default_profile() {
    let dir = tempdir().expect("tempdir");
    let app = agent_app(&dir, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/sessions"))
        .send()
        .await
        .expect("create session")
        .json()
        .await
        .expect("create payload");

    assert!(response["sessionId"].as_str().is_some());
    assert_eq!(response["channel"], "web");
    assert_eq!(response["activeProfile"], "openai:mock-model");
    assert_eq!(response["readOnly"], false);
    assert_eq!(response["canSend"], true);
    assert_eq!(response["canDuplicate"], false);
    assert!(response.get("messages").is_none());
}

#[tokio::test]
async fn chat_endpoint_includes_active_profile() {
    let dir = tempdir().expect("tempdir");
    let app = agent_app(
        &dir,
        vec![LlmResponse {
            content: Some("hello from model".to_string()),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
            extra: Map::new(),
        }],
    )
    .await;
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/chat"))
        .json(&serde_json::json!({
            "message": "hello",
            "sessionId": "browser-session-profile"
        }))
        .send()
        .await
        .expect("send chat request")
        .json()
        .await
        .expect("chat response body");

    assert_eq!(response["reply"], "hello from model");
    assert_eq!(response["channel"], "web");
    assert_eq!(response["activeProfile"], "openai:mock-model");
}

#[tokio::test]
async fn weixin_account_endpoints_report_login_status() {
    let app = build_test_router_with_weixin_account_state(sample_weixin_account()).await;
    let addr = spawn_test_server(app).await;

    let account: serde_json::Value = reqwest::get(format!("http://{addr}/api/weixin/account"))
        .await
        .expect("fetch weixin account")
        .json()
        .await
        .expect("weixin account payload");
    assert_eq!(account["enabled"], true);
    assert_eq!(account["loggedIn"], true);
    assert_eq!(account["expired"], false);
    assert_eq!(account["botId"], "bot@im.bot");
    assert_eq!(account["userId"], "user@im.wechat");

    let login_start: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/weixin/login/start"))
        .send()
        .await
        .expect("start weixin login")
        .json()
        .await
        .expect("login start payload");
    assert_eq!(login_start["qrcode"], "qr-token");
    assert_eq!(login_start["qrcodeImgContent"], "data:image/png;base64,abc");

    let login_status: serde_json::Value =
        reqwest::get(format!("http://{addr}/api/weixin/login/status"))
            .await
            .expect("poll weixin login")
            .json()
            .await
            .expect("login status payload");
    assert_eq!(login_status["status"], "wait");
    assert_eq!(login_status["loggedIn"], true);
    assert_eq!(login_status["expired"], false);

    let logout: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/weixin/logout"))
        .send()
        .await
        .expect("logout weixin")
        .json()
        .await
        .expect("logout payload");
    assert_eq!(logout["loggedIn"], false);
    assert_eq!(logout["expired"], false);

    let after_logout: serde_json::Value = reqwest::get(format!("http://{addr}/api/weixin/account"))
        .await
        .expect("fetch weixin account after logout")
        .json()
        .await
        .expect("weixin account after logout payload");
    assert_eq!(after_logout["loggedIn"], false);
    assert_eq!(after_logout["expired"], false);
}

#[tokio::test]
async fn chat_endpoint_rejects_non_web_sessions_until_duplicated() {
    let dir = tempdir().expect("tempdir");
    let mut telegram = Session::new("telegram:outside");
    telegram.active_profile = Some("openai:gpt-4.1-mini".to_string());
    telegram.messages = vec![text_message("assistant", "not web")];
    save_session(dir.path(), &telegram);

    let app = agent_app(&dir, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/chat"))
        .json(&serde_json::json!({
            "message": "hello",
            "channel": "telegram",
            "sessionId": "outside"
        }))
        .send()
        .await
        .expect("send non-web chat request");
    let status = response.status();
    let payload: serde_json::Value = response.json().await.expect("non-web payload");

    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST);
    assert!(payload["error"]
        .as_str()
        .unwrap_or_default()
        .contains("duplicate"));
    assert!(payload["error"]
        .as_str()
        .unwrap_or_default()
        .contains("read-only"));
}

#[tokio::test]
async fn duplicate_session_endpoint_returns_new_web_detail_with_copied_history() {
    let dir = tempdir().expect("tempdir");
    let mut telegram = Session::new("telegram:outside");
    telegram.active_profile = Some("openrouter:deepseek-r1".to_string());
    telegram.messages = vec![
        text_message("user", "hello from telegram"),
        text_message("assistant", "reply from telegram"),
    ];
    save_session(dir.path(), &telegram);

    let app = agent_app_with_profiles(
        &dir,
        Vec::new(),
        &[(
            "openrouter:deepseek-r1",
            "openrouter",
            "deepseek/deepseek-r1",
        )],
    )
    .await;
    let addr = spawn_test_server(app).await;

    let response: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/sessions/duplicate"))
        .json(&serde_json::json!({
            "channel": "telegram",
            "sessionId": "outside"
        }))
        .send()
        .await
        .expect("duplicate session")
        .json()
        .await
        .expect("duplicate payload");

    assert_eq!(response["channel"], "web");
    assert!(response["sessionId"].as_str().is_some());
    assert_eq!(response["activeProfile"], "openrouter:deepseek-r1");
    assert_eq!(response["readOnly"], false);
    assert_eq!(response["canSend"], true);
    assert_eq!(response["canDuplicate"], false);
    assert_eq!(response["sourceSessionKey"], "telegram:outside");
    let messages = response["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["content"], "hello from telegram");
    assert_eq!(messages[1]["content"], "reply from telegram");
}

#[tokio::test]
async fn nested_session_ids_are_browsable_and_duplicable() {
    let dir = tempdir().expect("tempdir");
    let mut system = Session::new("system:wecom:chat-42");
    system.active_profile = Some("openai:gpt-4.1-mini".to_string());
    system.messages = vec![
        text_message("user", "nested hello"),
        text_message("assistant", "nested reply"),
    ];
    save_session(dir.path(), &system);

    let app = agent_app(&dir, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let detail: serde_json::Value =
        reqwest::get(format!("http://{addr}/api/sessions/system/wecom:chat-42"))
            .await
            .expect("fetch nested detail")
            .json()
            .await
            .expect("nested detail payload");

    assert_eq!(detail["channel"], "system");
    assert_eq!(detail["sessionId"], "wecom:chat-42");
    assert_eq!(detail["messages"][1]["content"], "nested reply");

    let duplicated: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/api/sessions/duplicate"))
        .json(&serde_json::json!({
            "channel": "system",
            "sessionId": "wecom:chat-42"
        }))
        .send()
        .await
        .expect("duplicate nested session")
        .json()
        .await
        .expect("duplicate nested payload");

    assert_eq!(duplicated["channel"], "web");
    assert_eq!(duplicated["sourceSessionKey"], "system:wecom:chat-42");
}

#[tokio::test]
async fn duplicate_session_endpoint_returns_not_found_for_missing_source() {
    let dir = tempdir().expect("tempdir");
    let app = agent_app(&dir, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/sessions/duplicate"))
        .json(&serde_json::json!({
            "channel": "telegram",
            "sessionId": "missing"
        }))
        .send()
        .await
        .expect("duplicate missing session");
    let status = response.status();
    let payload: serde_json::Value = response.json().await.expect("missing duplicate payload");

    assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
    assert!(payload["error"]
        .as_str()
        .unwrap_or_default()
        .contains("not found"));
}

#[tokio::test]
async fn duplicate_session_endpoint_rejects_already_writable_web_sessions() {
    let dir = tempdir().expect("tempdir");
    let mut web_session = Session::new("web:alpha");
    web_session.active_profile = Some("openai:gpt-4.1-mini".to_string());
    web_session.messages = vec![text_message("assistant", "already web")];
    save_session(dir.path(), &web_session);

    let app = agent_app(&dir, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/sessions/duplicate"))
        .json(&serde_json::json!({
            "channel": "web",
            "sessionId": "alpha"
        }))
        .send()
        .await
        .expect("duplicate web session");
    let status = response.status();
    let payload: serde_json::Value = response.json().await.expect("duplicate web payload");

    assert_eq!(status, reqwest::StatusCode::BAD_REQUEST);
    assert!(payload["error"]
        .as_str()
        .unwrap_or_default()
        .contains("already writable"));
}

#[tokio::test]
async fn session_endpoints_reject_invalid_or_missing_ids() {
    let dir = tempdir().expect("tempdir");
    let mut telegram = Session::new("telegram:outside");
    telegram.active_profile = Some("openai:gpt-4.1-mini".to_string());
    telegram.messages = vec![text_message("assistant", "not web")];
    save_session(dir.path(), &telegram);

    let app = agent_app(&dir, Vec::new()).await;
    let addr = spawn_test_server(app).await;

    let invalid = reqwest::get(format!("http://{addr}/api/sessions/telegram/bad$id"))
        .await
        .expect("fetch invalid id");
    let invalid_status = invalid.status();
    let invalid_payload: serde_json::Value = invalid.json().await.expect("invalid payload");
    assert_eq!(invalid_status, reqwest::StatusCode::BAD_REQUEST);
    assert!(invalid_payload["error"]
        .as_str()
        .unwrap_or_default()
        .contains("invalid session id"));

    let missing = reqwest::get(format!("http://{addr}/api/sessions/telegram/missing"))
        .await
        .expect("fetch missing id");
    let missing_status = missing.status();
    let missing_payload: serde_json::Value = missing.json().await.expect("missing payload");
    assert_eq!(missing_status, reqwest::StatusCode::NOT_FOUND);
    assert!(missing_payload["error"]
        .as_str()
        .unwrap_or_default()
        .contains("session not found"));
}
