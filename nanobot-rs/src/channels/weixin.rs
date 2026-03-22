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
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, warn};
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::bus::{InboundMessage, MessageBus, OutboundMessage};
use crate::channels::Channel;
use crate::config::WeixinConfig;

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

#[derive(Debug, Clone)]
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
        *self
            .session
            .lock()
            .map_err(|_| anyhow!("weixin login session lock poisoned"))? =
            Some(WeixinLoginSessionState {
                qrcode: login.qrcode.clone(),
                _qrcode_img_content: login.qrcode_img_content.clone(),
                status: "wait".to_string(),
            });
        Ok(login)
    }

    pub async fn poll_login_status(&self) -> Result<WeixinLoginStatus> {
        let qrcode = {
            let session = self
                .session
                .lock()
                .map_err(|_| anyhow!("weixin login session lock poisoned"))?;
            session
                .as_ref()
                .context("weixin login has not been started")?
                .qrcode
                .clone()
        };

        let payload = self.client.poll_qr_status(&qrcode).await?;
        {
            let mut session = self
                .session
                .lock()
                .map_err(|_| anyhow!("weixin login session lock poisoned"))?;
            if let Some(session) = session.as_mut() {
                session.status = payload.status.clone();
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
                return Ok(PollOutcome::Empty);
            }
            Err(error) => return Err(error).context("failed to request weixin getupdates"),
        };

        let response = match response.error_for_status() {
            Ok(response) => response,
            Err(error) if error.is_timeout() => {
                debug!("weixin getupdates poll timed out after status");
                return Ok(PollOutcome::Empty);
            }
            Err(error) => return Err(error).context("weixin getupdates request failed"),
        };

        let payload = match response.json::<Value>().await {
            Ok(payload) => payload,
            Err(error) if error.is_timeout() => {
                debug!("weixin getupdates response timed out");
                return Ok(PollOutcome::Empty);
            }
            Err(error) => return Err(error).context("failed to parse weixin getupdates response"),
        };

        let errcode = payload
            .get("errcode")
            .or_else(|| payload.get("err_code"))
            .and_then(Value::as_i64)
            .unwrap_or_default();

        if errcode == -14 {
            account.status = "expired".to_string();
            account.updated_at = Utc::now();
            self.store.save_account(account)?;
            warn!("weixin account expired");
            return Ok(PollOutcome::Expired);
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

        if let Some(items) = root
            .get("items")
            .or_else(|| root.get("result"))
            .and_then(Value::as_array)
        {
            for item in items {
                if let Some(parsed) = parse_weixin_item(item) {
                    should_persist = true;
                    if let Some(context_token) = parsed.context_token.as_deref() {
                        self.store
                            .save_context_token(&parsed.from_user_id, context_token)?;
                    }
                    self.bus
                        .publish_inbound(InboundMessage {
                            channel: "weixin".to_string(),
                            sender_id: parsed.from_user_id.clone(),
                            chat_id: parsed.from_user_id,
                            content: parsed.text,
                            timestamp: Utc::now(),
                            metadata: Default::default(),
                            session_key_override: None,
                        })
                        .await?;
                }
            }
        }

        if should_persist {
            account.updated_at = Utc::now();
            self.store.save_account(account)?;
        }

        Ok(PollOutcome::Empty)
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
        while self.running.load(Ordering::SeqCst) {
            let mut account = match self.store.load_account()? {
                Some(account) => account,
                None => {
                    warn!("weixin enabled but no account is configured");
                    break;
                }
            };
            match self.poll_once(&mut account).await? {
                PollOutcome::Empty => {}
                PollOutcome::Expired => {
                    self.running.store(false, Ordering::SeqCst);
                    break;
                }
            }
        }
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    async fn send(&self, _msg: OutboundMessage) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedWeixinItem {
    from_user_id: String,
    text: String,
    context_token: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PollOutcome {
    Empty,
    Expired,
}

fn parse_weixin_item(item: &Value) -> Option<ParsedWeixinItem> {
    let msg_type = item
        .get("msg_type")
        .or_else(|| item.get("msgType"))
        .or_else(|| item.get("type"))
        .and_then(Value::as_str)?;
    if msg_type != "text" {
        debug!("dropping non-text weixin item: {msg_type}");
        return None;
    }
    if item
        .get("group_id")
        .and_then(Value::as_str)
        .is_some_and(|group| !group.is_empty())
        || item
            .get("groupId")
            .and_then(Value::as_str)
            .is_some_and(|group| !group.is_empty())
    {
        debug!("dropping group weixin item");
        return None;
    }

    let from_user_id = item
        .get("from_user_id")
        .or_else(|| item.get("fromUserId"))
        .or_else(|| item.pointer("/from/user_id"))
        .or_else(|| item.pointer("/from/userId"))
        .and_then(Value::as_str)?;
    let text = item
        .get("text")
        .or_else(|| item.get("content"))
        .and_then(Value::as_str)?;
    let context_token = item
        .get("context_token")
        .or_else(|| item.get("contextToken"))
        .or_else(|| item.get("context"))
        .and_then(Value::as_str)
        .map(ToString::to_string);

    Some(ParsedWeixinItem {
        from_user_id: from_user_id.to_string(),
        text: text.to_string(),
        context_token,
    })
}

#[derive(Debug, Clone)]
pub struct WeixinAccountStore {
    dir: PathBuf,
    workspace_lock: Arc<Mutex<()>>,
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

    pub fn load_account(&self) -> Result<Option<WeixinAccountState>> {
        let path = self.account_path();
        if !path.exists() {
            return Ok(None);
        }
        let account = read_json::<WeixinAccountState>(&path)?;
        Ok(Some(account))
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
        remove_if_exists(&self.context_tokens_path())
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
