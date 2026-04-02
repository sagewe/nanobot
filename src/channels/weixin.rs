use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, info, warn};
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::bus::{InboundMessage, MessageBus, OutboundMessage};
use crate::channels::Channel;
use crate::config::WeixinConfig;
use crate::presentation::should_deliver_to_channel;

const WEIXIN_IDLE_RETRY_DELAY: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeixinAccountState {
    pub bot_token: String,
    pub ilink_bot_id: String,
    pub baseurl: String,
    pub ilink_user_id: Option<String>,
    pub get_updates_buf: String,
    #[serde(default = "default_longpolling_timeout_ms")]
    pub longpolling_timeout_ms: u64,
    pub status: String,
    pub updated_at: DateTime<Utc>,
}

impl WeixinAccountState {
    pub fn is_expired(&self) -> bool {
        self.status.eq_ignore_ascii_case("expired")
    }

    pub fn is_logged_in(&self) -> bool {
        !self.bot_token.trim().is_empty() && !self.is_expired()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeixinLoginSession {
    pub account: WeixinAccountState,
    #[serde(default)]
    pub context_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeixinQrLoginResponse {
    pub qrcode: String,
    pub qrcode_img_content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeixinLoginStatus {
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct WeixinLoginSessionState {
    qrcode: String,
    _qrcode_img_content: String,
    status: String,
}

#[derive(Debug, Clone)]
pub struct WeixinClient {
    client: reqwest::Client,
    api_base: String,
}

impl WeixinClient {
    pub fn new(api_base: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_base: api_base.into().trim_end_matches('/').to_string(),
        }
    }

    pub fn api_base(&self) -> &str {
        &self.api_base
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.api_base, path.trim_start_matches('/'))
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.client.get(self.url(path))
    }

    fn get_with_x_wechat_uin(&self, path: &str) -> reqwest::RequestBuilder {
        self.get(path).header("X-WECHAT-UIN", Self::x_wechat_uin())
    }

    pub async fn fetch_qr_code(&self) -> Result<WeixinQrLoginResponse> {
        let payload = self
            .get_with_x_wechat_uin("/ilink/bot/get_bot_qrcode")
            .query(&[("bot_type", "3")])
            .send()
            .await
            .context("failed to request weixin qr code")?
            .error_for_status()
            .context("weixin qr code request failed")?
            .json::<Value>()
            .await
            .context("failed to parse weixin qr code response")?;
        parse_qr_code_response(&payload)
    }

    async fn poll_qr_status(&self, qrcode: &str) -> Result<WeixinLoginStatusPayload> {
        let payload = self
            .get_with_x_wechat_uin("/ilink/bot/get_qrcode_status")
            .query(&[("qrcode", qrcode)])
            .send()
            .await
            .context("failed to request weixin qr status")?
            .error_for_status()
            .context("weixin qr status request failed")?
            .json::<Value>()
            .await
            .context("failed to parse weixin qr status response")?;
        parse_qr_status_response(&payload)
    }

    pub fn x_wechat_uin() -> String {
        let uuid = Uuid::new_v4();
        let random = uuid.as_bytes();
        let seed = u32::from_le_bytes([random[0], random[1], random[2], random[3]]);
        base64_encode(seed.to_string().as_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WeixinLoginStatusPayload {
    status: String,
    bot_token: Option<String>,
    ilink_bot_id: Option<String>,
    baseurl: Option<String>,
    ilink_user_id: Option<String>,
    get_updates_buf: Option<String>,
    longpolling_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct WeixinLoginManager {
    client: WeixinClient,
    store: WeixinAccountStore,
    _channel_version: String,
    session: Arc<Mutex<Option<WeixinLoginSessionState>>>,
}

impl WeixinLoginManager {
    pub fn new(
        api_base: impl Into<String>,
        store: WeixinAccountStore,
        channel_version: impl Into<String>,
    ) -> Self {
        Self {
            client: WeixinClient::new(api_base),
            store,
            _channel_version: channel_version.into(),
            session: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn start_login(&self) -> Result<WeixinQrLoginResponse> {
        let login = self.client.fetch_qr_code().await?;
        let session = WeixinLoginSessionState {
            qrcode: login.qrcode.clone(),
            _qrcode_img_content: login.qrcode_img_content.clone(),
            status: "wait".to_string(),
        };
        *self
            .session
            .lock()
            .map_err(|_| anyhow!("weixin login session lock poisoned"))? = Some(session.clone());
        self.store.save_login_session(&session)?;
        Ok(login)
    }

    pub async fn poll_login_status(&self) -> Result<WeixinLoginStatus> {
        let login_session = {
            let mut session = self
                .session
                .lock()
                .map_err(|_| anyhow!("weixin login session lock poisoned"))?;
            if session.is_none() {
                *session = self.store.load_login_session()?;
            }
            session
                .clone()
                .context("weixin login has not been started")?
        };

        let payload = self.client.poll_qr_status(&login_session.qrcode).await?;
        {
            let mut session = self
                .session
                .lock()
                .map_err(|_| anyhow!("weixin login session lock poisoned"))?;
            if let Some(session) = session.as_mut() {
                session.status = payload.status.clone();
                self.store.save_login_session(session)?;
            }
        }

        if payload.status == "confirmed" {
            let account = WeixinAccountState {
                bot_token: payload
                    .bot_token
                    .clone()
                    .context("weixin confirmed login missing bot_token")?,
                ilink_bot_id: payload
                    .ilink_bot_id
                    .clone()
                    .context("weixin confirmed login missing ilink_bot_id")?,
                baseurl: payload
                    .baseurl
                    .clone()
                    .unwrap_or_else(|| self.client.api_base().to_string()),
                ilink_user_id: payload.ilink_user_id.clone(),
                get_updates_buf: String::new(),
                longpolling_timeout_ms: payload
                    .longpolling_timeout_ms
                    .unwrap_or_else(default_longpolling_timeout_ms),
                status: "confirmed".to_string(),
                updated_at: Utc::now(),
            };
            self.store.save_account(&account)?;
        }

        Ok(WeixinLoginStatus {
            status: payload.status,
        })
    }

    pub fn clear_login_session(&self) -> Result<()> {
        *self
            .session
            .lock()
            .map_err(|_| anyhow!("weixin login session lock poisoned"))? = None;
        self.store.clear_login_session()
    }
}

#[derive(Clone)]
pub struct WeixinChannel {
    config: WeixinConfig,
    store: WeixinAccountStore,
    bus: MessageBus,
    client: reqwest::Client,
    running: Arc<AtomicBool>,
}

impl WeixinChannel {
    pub fn new(config: WeixinConfig, store: WeixinAccountStore, bus: MessageBus) -> Self {
        Self {
            config,
            store,
            bus,
            client: reqwest::Client::new(),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    fn api_base(&self, account: &WeixinAccountState) -> String {
        let base = if account.baseurl.trim().is_empty() {
            &self.config.api_base
        } else {
            &account.baseurl
        };
        base.trim_end_matches('/').to_string()
    }

    fn channel_version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn getupdates_url(&self, account: &WeixinAccountState) -> String {
        format!("{}/ilink/bot/getupdates", self.api_base(account))
    }

    fn getupdates_request(&self, account: &WeixinAccountState) -> reqwest::RequestBuilder {
        self.client
            .post(self.getupdates_url(account))
            .header("AuthorizationType", "ilink_bot_token")
            .header("Authorization", format!("Bearer {}", account.bot_token))
            .header("X-WECHAT-UIN", WeixinClient::x_wechat_uin())
            .json(&serde_json::json!({
                "base_info": {
                    "channel_version": self.channel_version(),
                },
                "bot_token": account.bot_token.clone(),
                "ilink_bot_id": account.ilink_bot_id.clone(),
                "get_updates_buf": account.get_updates_buf.clone(),
                "longpolling_timeout_ms": account.longpolling_timeout_ms,
            }))
    }

    fn sendmessage_url(&self, api_base: &str) -> String {
        format!("{}/ilink/bot/sendmessage", api_base.trim_end_matches('/'))
    }

    fn sendmessage_request(
        &self,
        account: &WeixinAccountState,
        chat_id: &str,
        content: &str,
        context_token: &str,
    ) -> reqwest::RequestBuilder {
        let payload = WeixinSendMessageRequest {
            msg: WeixinSendMessage {
                from_user_id: String::new(),
                to_user_id: chat_id.to_string(),
                client_id: Uuid::new_v4().to_string(),
                message_type: 2,
                message_state: 2,
                item_list: vec![WeixinSendItem {
                    item_type: 1,
                    text_item: WeixinSendTextItem {
                        text: flatten_weixin_text(content),
                    },
                }],
                context_token: context_token.to_string(),
            },
            base_info: WeixinSendBaseInfo {
                channel_version: self.channel_version().to_string(),
            },
        };

        self.client
            .post(self.sendmessage_url(&self.api_base(account)))
            .header("AuthorizationType", "ilink_bot_token")
            .header("Authorization", format!("Bearer {}", account.bot_token))
            .header("X-WECHAT-UIN", WeixinClient::x_wechat_uin())
            .json(&payload)
    }

    async fn poll_once(&self, account: &mut WeixinAccountState) -> Result<PollOutcome> {
        let timeout = Duration::from_millis(account.longpolling_timeout_ms.max(1));
        let response = self
            .getupdates_request(account)
            .timeout(timeout)
            .send()
            .await;

        let response = match response {
            Ok(response) => response,
            Err(error) if error.is_timeout() => {
                debug!("weixin getupdates poll timed out");
                return Ok(PollOutcome::Polled);
            }
            Err(error) => return Err(error).context("failed to request weixin getupdates"),
        };

        let response = match response.error_for_status() {
            Ok(response) => response,
            Err(error) if error.is_timeout() => {
                debug!("weixin getupdates poll timed out after status");
                return Ok(PollOutcome::Polled);
            }
            Err(error) => return Err(error).context("weixin getupdates request failed"),
        };

        let payload = match response.json::<Value>().await {
            Ok(payload) => payload,
            Err(error) if error.is_timeout() => {
                debug!("weixin getupdates response timed out");
                return Ok(PollOutcome::Polled);
            }
            Err(error) => return Err(error).context("failed to parse weixin getupdates response"),
        };

        let errcode = weixin_error_code(&payload);

        if errcode == -14 {
            account.status = "expired".to_string();
            account.updated_at = Utc::now();
            self.store.save_account(account)?;
            warn!("weixin account expired");
            return Ok(PollOutcome::Expired);
        }
        if errcode != 0 {
            let errmsg = weixin_error_message(&payload);
            warn!("weixin getupdates failed: errcode={errcode} errmsg={errmsg}");
            return Err(anyhow!(
                "weixin getupdates failed: errcode={errcode} errmsg={errmsg}"
            ));
        }

        let root = payload.get("data").unwrap_or(&payload);
        let mut should_persist = false;

        if let Some(buf) = root
            .get("get_updates_buf")
            .or_else(|| root.get("getUpdatesBuf"))
            .and_then(Value::as_str)
        {
            if account.get_updates_buf != buf {
                account.get_updates_buf = buf.to_string();
                should_persist = true;
            }
        }
        if let Some(timeout_ms) = root
            .get("longpolling_timeout_ms")
            .or_else(|| root.get("longpollingTimeoutMs"))
            .and_then(Value::as_u64)
        {
            if account.longpolling_timeout_ms != timeout_ms {
                account.longpolling_timeout_ms = timeout_ms;
                should_persist = true;
            }
        }

        for message in parse_weixin_messages(root) {
            should_persist = true;
            if let Some(context_token) = message.context_token.as_deref() {
                self.store
                    .save_context_token(&message.from_user_id, context_token)?;
            }
            if message.is_text {
                info!(
                    "weixin text callback sender={} chat={}",
                    message.from_user_id, message.from_user_id
                );
            } else {
                info!(
                    "weixin non-text callback sender={} chat={}",
                    message.from_user_id, message.from_user_id
                );
            }
            self.bus
                .publish_inbound(InboundMessage {
                    channel: "weixin".to_string(),
                    sender_id: message.from_user_id.clone(),
                    chat_id: message.from_user_id,
                    content: message.text,
                    media: Vec::new(),
                    timestamp: Utc::now(),
                    metadata: Default::default(),
                    session_key_override: None,
                })
                .await?;
        }

        if should_persist {
            account.updated_at = Utc::now();
            self.store.save_account(account)?;
        }

        Ok(PollOutcome::Polled)
    }
}

#[async_trait]
impl Channel for WeixinChannel {
    fn name(&self) -> &'static str {
        "weixin"
    }

    async fn start(&self) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        self.running.store(true, Ordering::SeqCst);
        let mut phase: Option<WeixinRuntimePhase> = None;
        while self.running.load(Ordering::SeqCst) {
            let account = match self.store.load_account() {
                Ok(account) => account,
                Err(error) => {
                    warn!("failed to load weixin account: {error}");
                    tokio::time::sleep(WEIXIN_IDLE_RETRY_DELAY).await;
                    continue;
                }
            };
            let Some(mut account) = account else {
                if phase != Some(WeixinRuntimePhase::WaitingForLogin) {
                    info!("weixin waiting for login");
                    phase = Some(WeixinRuntimePhase::WaitingForLogin);
                }
                tokio::time::sleep(WEIXIN_IDLE_RETRY_DELAY).await;
                continue;
            };

            if account.status == "expired" {
                if phase != Some(WeixinRuntimePhase::Expired) {
                    info!("weixin account expired; waiting for relogin");
                    phase = Some(WeixinRuntimePhase::Expired);
                }
                tokio::time::sleep(WEIXIN_IDLE_RETRY_DELAY).await;
                continue;
            }

            let current_phase = WeixinRuntimePhase::Polling {
                bot_id: account.ilink_bot_id.clone(),
                api_base: self.api_base(&account),
            };
            if phase.as_ref() != Some(&current_phase) {
                if let WeixinRuntimePhase::Polling { bot_id, api_base } = &current_phase {
                    info!("weixin polling started bot={bot_id} base={api_base}");
                }
                phase = Some(current_phase);
            }

            match self.poll_once(&mut account).await {
                Ok(PollOutcome::Polled) => {}
                Ok(PollOutcome::Expired) => {
                    tokio::time::sleep(WEIXIN_IDLE_RETRY_DELAY).await;
                }
                Err(error) => {
                    warn!("weixin polling failed: {error:#}");
                    tokio::time::sleep(WEIXIN_IDLE_RETRY_DELAY).await;
                }
            }
        }
        info!("weixin channel stopped");
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    async fn send(&self, msg: OutboundMessage) -> Result<()> {
        if !should_deliver_to_channel("weixin", &msg.metadata) {
            return Ok(());
        }

        let account = self
            .store
            .load_account()?
            .context("missing weixin account")?;
        let context_token = self
            .store
            .load_context_token(&msg.chat_id)?
            .with_context(|| format!("missing context_token for weixin chat {}", msg.chat_id))?;

        let response = self
            .sendmessage_request(&account, &msg.chat_id, &msg.content, &context_token)
            .send()
            .await
            .context("failed to request weixin sendmessage")?
            .error_for_status()
            .context("weixin sendmessage request failed")?;
        let body = response
            .bytes()
            .await
            .context("failed to read weixin sendmessage response body")?;
        if !body.is_empty() {
            let payload = serde_json::from_slice::<Value>(&body)
                .context("failed to parse weixin sendmessage response")?;
            let errcode = weixin_error_code(&payload);
            if errcode != 0 {
                let errmsg = weixin_error_message(&payload);
                return Err(anyhow!(
                    "weixin sendmessage failed: ret={errcode} errmsg={errmsg}"
                ));
            }
        }
        info!("weixin reply sent chat={}", msg.chat_id);
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
struct WeixinSendMessageRequest {
    msg: WeixinSendMessage,
    base_info: WeixinSendBaseInfo,
}

#[derive(Debug, Clone, Serialize)]
struct WeixinSendMessage {
    from_user_id: String,
    to_user_id: String,
    client_id: String,
    message_type: i64,
    message_state: i64,
    item_list: Vec<WeixinSendItem>,
    context_token: String,
}

#[derive(Debug, Clone, Serialize)]
struct WeixinSendItem {
    #[serde(rename = "type")]
    item_type: i64,
    text_item: WeixinSendTextItem,
}

#[derive(Debug, Clone, Serialize)]
struct WeixinSendTextItem {
    text: String,
}

#[derive(Debug, Clone, Serialize)]
struct WeixinSendBaseInfo {
    channel_version: String,
}

fn flatten_weixin_text(content: &str) -> String {
    let mut parts = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let line = strip_weixin_block_prefix(line);
        let line = replace_weixin_links(line);
        let line = replace_weixin_inline_markers(&line);
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if !line.is_empty() {
            parts.push(line);
        }
    }
    parts.join(" ")
}

fn strip_weixin_block_prefix(line: &str) -> &str {
    let line = line.trim_start();
    let line = strip_weixin_heading_prefix(line);
    if let Some(rest) = line.strip_prefix("- ") {
        return rest;
    }
    if let Some(rest) = line.strip_prefix("* ") {
        return rest;
    }
    if let Some(rest) = line.strip_prefix("+ ") {
        return rest;
    }
    if let Some(rest) = strip_weixin_numbered_prefix(line) {
        return rest;
    }
    line
}

fn strip_weixin_heading_prefix(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() && bytes[index] == b'#' {
        index += 1;
    }
    if index == 0 || index > 6 || index >= bytes.len() || !bytes[index].is_ascii_whitespace() {
        return line;
    }
    line[index..].trim_start()
}

fn strip_weixin_numbered_prefix(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == 0 || index + 1 >= bytes.len() {
        return None;
    }
    let separator = bytes[index];
    if (separator == b'.' || separator == b')') && bytes[index + 1] == b' ' {
        Some(line[index + 2..].trim_start())
    } else {
        None
    }
}

fn replace_weixin_links(line: &str) -> String {
    static LINK_RE: OnceLock<Regex> = OnceLock::new();
    let link_re = LINK_RE
        .get_or_init(|| Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").expect("valid weixin link regex"));
    link_re.replace_all(line, "$1 ($2)").to_string()
}

fn replace_weixin_inline_markers(line: &str) -> String {
    static CODE_RE: OnceLock<Regex> = OnceLock::new();
    static STRONG_RE: OnceLock<Regex> = OnceLock::new();
    static EMPHASIS_STAR_RE: OnceLock<Regex> = OnceLock::new();
    static EMPHASIS_UNDERSCORE_RE: OnceLock<Regex> = OnceLock::new();
    static STRIKE_RE: OnceLock<Regex> = OnceLock::new();

    let code_re = CODE_RE.get_or_init(|| Regex::new(r"`([^`]+)`").expect("valid code regex"));
    let strong_re =
        STRONG_RE.get_or_init(|| Regex::new(r"\*\*([^*]+)\*\*").expect("valid strong regex"));
    let emphasis_star_re = EMPHASIS_STAR_RE.get_or_init(|| {
        Regex::new(r"(^|[^[:alnum:]_])\*([^\s*][^*]*?)\*([^[:alnum:]_]|$)")
            .expect("valid emphasis regex")
    });
    let emphasis_underscore_re = EMPHASIS_UNDERSCORE_RE.get_or_init(|| {
        Regex::new(r"(^|[^[:alnum:]_])_([^\s_][^_]*?)_([^[:alnum:]_]|$)")
            .expect("valid emphasis regex")
    });
    let strike_re =
        STRIKE_RE.get_or_init(|| Regex::new(r"~~([^~]+)~~").expect("valid strike regex"));

    let line = code_re.replace_all(line, "$1").to_string();
    let line = strong_re.replace_all(&line, "$1").to_string();
    let line = emphasis_star_re.replace_all(&line, "$1$2$3").to_string();
    let line = emphasis_underscore_re
        .replace_all(&line, "$1$2$3")
        .to_string();
    strike_re.replace_all(&line, "$1").to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedWeixinMessage {
    from_user_id: String,
    text: String,
    context_token: Option<String>,
    is_text: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PollOutcome {
    Polled,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WeixinRuntimePhase {
    WaitingForLogin,
    Polling { bot_id: String, api_base: String },
    Expired,
}

fn parse_weixin_messages(root: &Value) -> Vec<ParsedWeixinMessage> {
    if let Some(messages) = root
        .get("msgs")
        .or_else(|| root.get("message_list"))
        .or_else(|| root.get("messageList"))
        .and_then(Value::as_array)
    {
        return messages.iter().filter_map(parse_weixin_message).collect();
    }
    parse_weixin_message(root).into_iter().collect()
}

fn parse_weixin_message(root: &Value) -> Option<ParsedWeixinMessage> {
    let message_type = root
        .get("message_type")
        .or_else(|| root.get("messageType"))
        .and_then(Value::as_i64)?;
    if root
        .get("group_id")
        .and_then(Value::as_str)
        .is_some_and(|group| !group.is_empty())
        || root
            .get("groupId")
            .and_then(Value::as_str)
            .is_some_and(|group| !group.is_empty())
    {
        debug!("dropping group weixin item");
        return None;
    }

    let context_token = root
        .get("context_token")
        .or_else(|| root.get("contextToken"))
        .and_then(Value::as_str)
        .map(ToString::to_string);

    let from_user_id = root
        .get("from_user_id")
        .or_else(|| root.get("fromUserId"))
        .or_else(|| root.pointer("/from/user_id"))
        .or_else(|| root.pointer("/from/userId"))
        .and_then(Value::as_str)?;

    let item_list = root
        .get("item_list")
        .or_else(|| root.get("itemList"))
        .and_then(Value::as_array)?;
    let text_item = item_list.iter().find_map(parse_weixin_text_item);
    let has_non_text_item = item_list
        .iter()
        .filter_map(weixin_item_type)
        .any(|item_type| item_type != 1);
    let (text, is_text) = if let Some(text) = text_item {
        (text.to_string(), true)
    } else if message_type != 1 || has_non_text_item {
        (
            summarize_non_text_weixin_message(message_type, item_list),
            false,
        )
    } else {
        return None;
    };

    if !is_text {
        debug!("accepted non-text weixin message_type {message_type}");
    }

    Some(ParsedWeixinMessage {
        from_user_id: from_user_id.to_string(),
        text,
        context_token,
        is_text,
    })
}

fn summarize_non_text_weixin_message(message_type: i64, item_list: &[Value]) -> String {
    let mut labels = Vec::new();
    for item in item_list {
        let item_type = weixin_item_type(item);
        let Some(item_type) = item_type else {
            continue;
        };
        let label = match item_type {
            1 => "text",
            2 => "image",
            3 => "voice",
            4 => "video",
            5 => "file",
            6 => "location",
            7 => "link",
            _ => "unknown",
        };
        if !labels.iter().any(|existing| existing == &label) {
            labels.push(label);
        }
    }
    if labels.is_empty() {
        format!("Received non-text weixin message (message_type={message_type})")
    } else {
        format!(
            "Received non-text weixin message (message_type={message_type}, items={})",
            labels.join(",")
        )
    }
}

fn weixin_item_type(item: &Value) -> Option<i64> {
    item.get("item_type")
        .or_else(|| item.get("type"))
        .or_else(|| item.get("itemType"))
        .and_then(Value::as_i64)
}

fn parse_weixin_text_item(item: &Value) -> Option<&str> {
    let item_type = item
        .get("item_type")
        .or_else(|| item.get("type"))
        .or_else(|| item.get("itemType"))
        .and_then(Value::as_i64)?;
    if item_type != 1 {
        return None;
    }

    if let Some(text) = item.get("text").and_then(Value::as_str) {
        let text = text.trim();
        if !text.is_empty() {
            return Some(text);
        }
    }

    item.pointer("/text/content")
        .or_else(|| item.pointer("/text_item/text"))
        .or_else(|| item.pointer("/textItem/text"))
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
}

fn weixin_error_code(payload: &Value) -> i64 {
    payload
        .get("errcode")
        .or_else(|| payload.get("err_code"))
        .and_then(Value::as_i64)
        .filter(|code| *code != 0)
        .or_else(|| {
            payload
                .get("ret")
                .and_then(Value::as_i64)
                .filter(|code| *code != 0)
        })
        .unwrap_or_default()
}

fn weixin_error_message(payload: &Value) -> &str {
    payload
        .get("errmsg")
        .or_else(|| payload.get("err_msg"))
        .and_then(Value::as_str)
        .unwrap_or("unknown error")
}

#[derive(Debug, Clone)]
pub struct WeixinAccountStore {
    dir: PathBuf,
    workspace_lock: Arc<Mutex<()>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeixinLoginStatusSummary {
    pub configured: bool,
    pub login_state: String,
    pub account_status: String,
    pub bot_id: Option<String>,
}

impl WeixinAccountStore {
    pub fn new(workspace: &Path) -> Result<Self> {
        let dir = workspace.join("channels").join("weixin");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;
        let workspace_lock = workspace_lock(&dir)?;
        Ok(Self {
            dir,
            workspace_lock,
        })
    }

    fn account_path(&self) -> PathBuf {
        self.dir.join("account.json")
    }

    fn context_tokens_path(&self) -> PathBuf {
        self.dir.join("context_tokens.json")
    }

    fn login_session_path(&self) -> PathBuf {
        self.dir.join("login_session.json")
    }

    pub fn load_account(&self) -> Result<Option<WeixinAccountState>> {
        let path = self.account_path();
        if !path.exists() {
            return Ok(None);
        }
        let account = read_json::<WeixinAccountState>(&path)?;
        Ok(Some(account))
    }

    pub fn login_status_summary(&self) -> Result<WeixinLoginStatusSummary> {
        match self.load_account()? {
            Some(account) => Ok(WeixinLoginStatusSummary {
                configured: true,
                login_state: if account.is_expired() {
                    "expired".to_string()
                } else if account.is_logged_in() {
                    "logged in".to_string()
                } else {
                    "not logged in".to_string()
                },
                account_status: account.status,
                bot_id: Some(account.ilink_bot_id),
            }),
            None => Ok(WeixinLoginStatusSummary {
                configured: false,
                login_state: "not logged in".to_string(),
                account_status: "missing".to_string(),
                bot_id: None,
            }),
        }
    }

    pub fn save_account(&self, account: &WeixinAccountState) -> Result<()> {
        let _guard = self
            .workspace_lock
            .lock()
            .map_err(|_| anyhow!("weixin workspace lock poisoned"))?;
        write_json(&self.account_path(), account)
    }

    pub fn clear_account(&self) -> Result<()> {
        let _guard = self
            .workspace_lock
            .lock()
            .map_err(|_| anyhow!("weixin workspace lock poisoned"))?;
        remove_if_exists(&self.account_path())
    }

    fn load_login_session(&self) -> Result<Option<WeixinLoginSessionState>> {
        let path = self.login_session_path();
        if !path.exists() {
            return Ok(None);
        }
        let session = read_json::<WeixinLoginSessionState>(&path)?;
        Ok(Some(session))
    }

    fn save_login_session(&self, session: &WeixinLoginSessionState) -> Result<()> {
        let _guard = self
            .workspace_lock
            .lock()
            .map_err(|_| anyhow!("weixin workspace lock poisoned"))?;
        write_json(&self.login_session_path(), session)
    }

    fn clear_login_session(&self) -> Result<()> {
        let _guard = self
            .workspace_lock
            .lock()
            .map_err(|_| anyhow!("weixin workspace lock poisoned"))?;
        remove_if_exists(&self.login_session_path())
    }

    pub fn load_context_token(&self, peer_user_id: &str) -> Result<Option<String>> {
        let path = self.context_tokens_path();
        if !path.exists() {
            return Ok(None);
        }
        let tokens = read_json::<BTreeMap<String, String>>(&path)?;
        Ok(tokens.get(peer_user_id).cloned())
    }

    pub fn save_context_token(&self, peer_user_id: &str, token: &str) -> Result<()> {
        let _guard = self
            .workspace_lock
            .lock()
            .map_err(|_| anyhow!("weixin workspace lock poisoned"))?;
        let mut tokens = if self.context_tokens_path().exists() {
            read_json::<BTreeMap<String, String>>(&self.context_tokens_path())?
        } else {
            BTreeMap::new()
        };
        tokens.insert(peer_user_id.to_string(), token.to_string());
        write_json(&self.context_tokens_path(), &tokens)
    }

    pub fn clear_all(&self) -> Result<()> {
        let _guard = self
            .workspace_lock
            .lock()
            .map_err(|_| anyhow!("weixin workspace lock poisoned"))?;
        remove_if_exists(&self.account_path())?;
        remove_if_exists(&self.context_tokens_path())?;
        remove_if_exists(&self.login_session_path())
    }
}

fn parse_qr_code_response(payload: &Value) -> Result<WeixinQrLoginResponse> {
    let root = payload.get("data").unwrap_or(payload);
    Ok(WeixinQrLoginResponse {
        qrcode: json_string(root, &["qrcode", "qrCode"])
            .context("weixin qr response missing qrcode")?,
        qrcode_img_content: json_string(root, &["qrcode_img_content", "qrcodeImgContent"])
            .context("weixin qr response missing qrcode_img_content")?,
    })
}

fn parse_qr_status_response(payload: &Value) -> Result<WeixinLoginStatusPayload> {
    let root = payload.get("data").unwrap_or(payload);
    Ok(WeixinLoginStatusPayload {
        status: json_string(root, &["status"])
            .context("weixin qr status response missing status")?,
        bot_token: json_string(root, &["bot_token", "botToken"]),
        ilink_bot_id: json_string(root, &["ilink_bot_id", "ilinkBotId"]),
        baseurl: json_string(root, &["baseurl", "baseUrl"]),
        ilink_user_id: json_string(root, &["ilink_user_id", "ilinkUserId"]),
        get_updates_buf: json_string(root, &["get_updates_buf", "getUpdatesBuf"]),
        longpolling_timeout_ms: root
            .get("longpolling_timeout_ms")
            .or_else(|| root.get("longpollingTimeoutMs"))
            .and_then(Value::as_u64),
    })
}

fn default_longpolling_timeout_ms() -> u64 {
    35_000
}

fn json_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(ToString::to_string)
    })
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

fn workspace_lock(dir: &Path) -> Result<Arc<Mutex<()>>> {
    static WORKSPACE_LOCKS: OnceLock<Mutex<std::collections::HashMap<PathBuf, Arc<Mutex<()>>>>> =
        OnceLock::new();

    let mut locks = WORKSPACE_LOCKS
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .map_err(|_| anyhow!("weixin workspace lock registry poisoned"))?;
    Ok(locks
        .entry(dir.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone())
}

fn read_json<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let value = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(value)
}

fn write_json<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(value).context("failed to serialize json")?;
    let temp_path = path.with_extension(format!(
        "{}.tmp-{}",
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("json"),
        Uuid::new_v4()
    ));
    {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }
        let mut file = options
            .open(&temp_path)
            .with_context(|| format!("failed to create {}", temp_path.display()))?;
        file.write_all(raw.as_bytes())
            .with_context(|| format!("failed to write {}", temp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync {}", temp_path.display()))?;
    }
    std::fs::rename(&temp_path, path).with_context(|| {
        let _ = std::fs::remove_file(&temp_path);
        format!(
            "failed to atomically replace {} with {}",
            path.display(),
            temp_path.display()
        )
    })?;
    #[cfg(unix)]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to set permissions on {}", path.display()))?;
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}
