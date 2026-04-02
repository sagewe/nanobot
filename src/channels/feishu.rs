use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use futures_util::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::fs;
use tokio::sync::Mutex;
use tokio::time::{self, MissedTickBehavior};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};
use url::Url;

use crate::bus::{InboundMessage, MessageBus, OutboundMessage};
use crate::channels::Channel;
use crate::config::FeishuConfig;
use crate::presentation::should_deliver_to_channel;

const FEISHU_DEDUP_CAPACITY: usize = 1000;
const FEISHU_RECONNECT_DELAY: Duration = Duration::from_millis(100);
const FEISHU_FRAME_METHOD_CONTROL: i32 = 0;
const FEISHU_FRAME_METHOD_DATA: i32 = 1;
const FEISHU_EVENT_TYPE: &str = "event";
const FEISHU_PING_TYPE: &str = "ping";
const FEISHU_PONG_TYPE: &str = "pong";
const FEISHU_HEADER_TYPE: &str = "type";
const FEISHU_HEADER_BIZ_RT: &str = "biz_rt";
const FEISHU_HTTP_OK: i32 = 200;
const FEISHU_HTTP_INTERNAL_ERROR: i32 = 500;

#[derive(Clone, PartialEq, prost::Message)]
struct FeishuWsHeader {
    #[prost(string, tag = "1")]
    key: String,
    #[prost(string, tag = "2")]
    value: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct FeishuWsFrame {
    #[prost(uint64, tag = "1")]
    seq_id: u64,
    #[prost(uint64, tag = "2")]
    log_id: u64,
    #[prost(int32, tag = "3")]
    service: i32,
    #[prost(int32, tag = "4")]
    method: i32,
    #[prost(message, repeated, tag = "5")]
    headers: Vec<FeishuWsHeader>,
    #[prost(string, tag = "6")]
    payload_encoding: String,
    #[prost(string, tag = "7")]
    payload_type: String,
    #[prost(bytes = "vec", tag = "8")]
    payload: Vec<u8>,
    #[prost(string, tag = "9")]
    log_id_new: String,
}

#[derive(Debug, Deserialize)]
struct FeishuWsConnectConfigResponse {
    code: i64,
    data: Option<FeishuWsConnectConfigData>,
}

#[derive(Debug, Deserialize)]
struct FeishuWsConnectConfigData {
    #[serde(rename = "URL")]
    url: String,
    #[serde(rename = "ClientConfig")]
    client_config: FeishuWsClientConfig,
}

#[derive(Debug, Deserialize)]
struct FeishuWsClientConfig {
    #[serde(rename = "PingInterval")]
    ping_interval: Option<u64>,
}

#[derive(Debug, Clone)]
struct FeishuWsSessionConfig {
    connect_url: String,
    service_id: i32,
    ping_interval: Duration,
}

#[derive(Clone, Debug)]
struct FeishuToken {
    token: String,
    expire_at_unix: i64,
}

struct NormalizedInboundContent {
    content: String,
    media: Vec<String>,
}

pub struct FeishuChannel {
    config: FeishuConfig,
    bus: MessageBus,
    client: reqwest::Client,
    running: AtomicBool,
    token: Arc<Mutex<Option<FeishuToken>>>,
    bot_open_id: Arc<Mutex<Option<String>>>,
    dedup: Arc<Mutex<RecentMessageDedup>>,
}

impl FeishuChannel {
    pub fn new(config: FeishuConfig, bus: MessageBus) -> Self {
        Self {
            config,
            bus,
            client: reqwest::Client::new(),
            running: AtomicBool::new(false),
            token: Arc::new(Mutex::new(None)),
            bot_open_id: Arc::new(Mutex::new(None)),
            dedup: Arc::new(Mutex::new(RecentMessageDedup::new(FEISHU_DEDUP_CAPACITY))),
        }
    }

    fn validate_startup_config(&self) -> Result<()> {
        ensure!(
            !self.config.app_id.trim().is_empty() && !self.config.app_secret.trim().is_empty(),
            "feishu app_id/app_secret is required"
        );

        let api_url = Url::parse(self.config.api_base.trim())
            .map_err(|error| anyhow::anyhow!("invalid feishu api_base: {error}"))?;
        ensure!(
            matches!(api_url.scheme(), "http" | "https"),
            "invalid feishu api_base scheme"
        );

        normalize_ws_config_endpoint(self.config.ws_base.trim())?;

        Ok(())
    }

    async fn cached_token(&self) -> Option<String> {
        let now = chrono::Utc::now().timestamp();
        self.token
            .lock()
            .await
            .as_ref()
            .and_then(|state| (state.expire_at_unix > now).then(|| state.token.clone()))
    }

    async fn tenant_access_token(&self) -> Result<String> {
        if let Some(token) = self.cached_token().await {
            debug!("feishu tenant token cache hit");
            return Ok(token);
        }

        let url = format!(
            "{}/auth/v3/tenant_access_token/internal",
            self.config.api_base.trim_end_matches('/')
        );
        let payload: Value = self
            .client
            .post(url)
            .json(&json!({
                "app_id": self.config.app_id,
                "app_secret": self.config.app_secret,
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let code = payload.get("code").and_then(Value::as_i64).unwrap_or(-1);
        ensure!(code == 0, "feishu auth failed");

        let token = payload
            .get("tenant_access_token")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing tenant_access_token"))?
            .to_string();
        let expire = payload
            .get("expire")
            .and_then(Value::as_i64)
            .unwrap_or(7200)
            .max(120);
        *self.token.lock().await = Some(FeishuToken {
            token: token.clone(),
            expire_at_unix: chrono::Utc::now().timestamp() + expire - 60,
        });
        debug!(expires_in_s = expire, "feishu tenant token refreshed");
        Ok(token)
    }

    async fn bot_open_id(&self) -> Result<String> {
        if let Some(open_id) = self.bot_open_id.lock().await.clone() {
            return Ok(open_id);
        }

        let access_token = self.tenant_access_token().await?;
        let url = format!("{}/bot/v3/info", self.config.api_base.trim_end_matches('/'));
        let payload: Value = self
            .client
            .get(url)
            .bearer_auth(access_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let code = payload.get("code").and_then(Value::as_i64).unwrap_or(-1);
        ensure!(code == 0, "feishu bot info failed");
        let open_id = payload
            .pointer("/bot/open_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing feishu bot open_id"))?
            .to_string();
        *self.bot_open_id.lock().await = Some(open_id.clone());
        info!(bot_open_id = %redact_identifier(&open_id), "feishu bot identity resolved");
        Ok(open_id)
    }

    async fn ws_session_config(&self) -> Result<FeishuWsSessionConfig> {
        let endpoint = normalize_ws_config_endpoint(self.config.ws_base.trim())?;
        let payload: FeishuWsConnectConfigResponse = self
            .client
            .post(endpoint.clone())
            .header("locale", "zh")
            .json(&json!({
                "AppID": self.config.app_id,
                "AppSecret": self.config.app_secret,
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        ensure!(payload.code == 0, "feishu ws config failed");
        let data = payload
            .data
            .ok_or_else(|| anyhow::anyhow!("missing feishu ws config data"))?;
        let connect_url = Url::parse(data.url.as_str()).context("invalid feishu connect URL")?;
        let service_id = connect_url
            .query_pairs()
            .find_map(|(key, value)| {
                if key == "service_id" {
                    Some(value.to_string())
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow::anyhow!("missing feishu service_id"))?
            .parse::<i32>()
            .context("invalid feishu service_id")?;
        let ping_secs = data.client_config.ping_interval.unwrap_or(120).max(1);
        info!(
            ws_endpoint = %sanitize_url_for_log(endpoint.as_str()),
            connect_url = %sanitize_url_for_log(data.url.as_str()),
            service_id,
            ping_interval_s = ping_secs,
            "feishu websocket session config loaded"
        );
        Ok(FeishuWsSessionConfig {
            connect_url: data.url,
            service_id,
            ping_interval: Duration::from_secs(ping_secs),
        })
    }

    async fn create_message(
        &self,
        access_token: &str,
        receive_id_type: &str,
        receive_id: &str,
        msg_type: &str,
        content: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/im/v1/messages?receive_id_type={receive_id_type}",
            self.config.api_base.trim_end_matches('/')
        );
        let payload: Value = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .json(&json!({
                "receive_id": receive_id,
                "msg_type": msg_type,
                "content": content,
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        ensure!(
            payload.get("code").and_then(Value::as_i64).unwrap_or(-1) == 0,
            "feishu create message failed"
        );
        Ok(())
    }

    async fn reply_message(
        &self,
        access_token: &str,
        message_id: &str,
        msg_type: &str,
        content: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/im/v1/messages/{message_id}/reply",
            self.config.api_base.trim_end_matches('/')
        );
        let payload: Value = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .json(&json!({
                "msg_type": msg_type,
                "content": content,
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        ensure!(
            payload.get("code").and_then(Value::as_i64).unwrap_or(-1) == 0,
            "feishu reply failed"
        );
        Ok(())
    }

    async fn add_reaction(&self, message_id: &str) -> Result<()> {
        if self.config.react_emoji.trim().is_empty() {
            return Ok(());
        }

        let access_token = self.tenant_access_token().await?;
        let url = format!(
            "{}/im/v1/messages/{message_id}/reactions",
            self.config.api_base.trim_end_matches('/')
        );
        let payload: Value = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .json(&json!({
                "reaction_type": {
                    "emoji_type": self.config.react_emoji,
                }
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        ensure!(
            payload.get("code").and_then(Value::as_i64).unwrap_or(-1) == 0,
            "feishu add reaction failed"
        );
        Ok(())
    }

    fn media_dir(&self) -> PathBuf {
        std::env::temp_dir().join("sidekick-feishu-media")
    }

    fn sanitize_media_component(value: &str) -> String {
        let sanitized: String = value
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
                    ch
                } else {
                    '_'
                }
            })
            .collect();
        if sanitized.is_empty() {
            "media.bin".to_string()
        } else {
            sanitized
        }
    }

    fn upload_is_image(media_path: &str) -> bool {
        let extension = Path::new(media_path)
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase());
        matches!(
            extension.as_deref(),
            Some("jpg") | Some("jpeg") | Some("png") | Some("gif") | Some("webp") | Some("bmp")
        )
    }

    async fn download_message_resource(
        &self,
        access_token: &str,
        message_id: &str,
        resource_key: &str,
        resource_type: &str,
        label: &str,
    ) -> Result<String> {
        let url = format!(
            "{}/im/v1/messages/{message_id}/resources/{resource_key}",
            self.config.api_base.trim_end_matches('/')
        );
        let bytes = self
            .client
            .get(url)
            .bearer_auth(access_token)
            .query(&[("type", resource_type)])
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        let mut local_path = self.media_dir();
        let file_key = Self::sanitize_media_component(resource_key);
        local_path.push(format!("{message_id}-{label}-{file_key}"));
        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&local_path, bytes).await?;
        Ok(local_path.display().to_string())
    }

    async fn upload_image(&self, access_token: &str, media_path: &str) -> Result<(String, String)> {
        let bytes = fs::read(media_path).await?;
        let file_name = Path::new(media_path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("image.bin")
            .to_string();
        let (content_type, payload_bytes) =
            Self::multipart_body(&[("image_type", "message")], "image", &file_name, &bytes);
        let url = format!(
            "{}/im/v1/images",
            self.config.api_base.trim_end_matches('/')
        );
        let payload: Value = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .header("Content-Type", content_type)
            .body(payload_bytes)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        ensure!(
            payload.get("code").and_then(Value::as_i64).unwrap_or(-1) == 0,
            "feishu image upload failed"
        );
        let image_key = payload
            .pointer("/data/image_key")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing feishu image_key"))?
            .to_string();
        Ok((
            "image".to_string(),
            json!({ "image_key": image_key }).to_string(),
        ))
    }

    async fn upload_file(&self, access_token: &str, media_path: &str) -> Result<(String, String)> {
        let bytes = fs::read(media_path).await?;
        let file_name = Path::new(media_path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("file.bin")
            .to_string();
        let (content_type, payload_bytes) = Self::multipart_body(
            &[("file_type", "stream"), ("file_name", file_name.as_str())],
            "file",
            &file_name,
            &bytes,
        );
        let url = format!("{}/im/v1/files", self.config.api_base.trim_end_matches('/'));
        let payload: Value = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .header("Content-Type", content_type)
            .body(payload_bytes)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        ensure!(
            payload.get("code").and_then(Value::as_i64).unwrap_or(-1) == 0,
            "feishu file upload failed"
        );
        let file_key = payload
            .pointer("/data/file_key")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing feishu file_key"))?
            .to_string();
        Ok((
            "file".to_string(),
            json!({ "file_key": file_key }).to_string(),
        ))
    }

    fn multipart_body(
        fields: &[(&str, &str)],
        file_field: &str,
        file_name: &str,
        bytes: &[u8],
    ) -> (String, Vec<u8>) {
        let boundary = format!(
            "----sidekick-feishu-{}",
            chrono::Utc::now().timestamp_micros()
        );
        let mut body = Vec::new();
        for (name, value) in fields {
            body.extend_from_slice(
                format!(
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
                )
                .as_bytes(),
            );
        }
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{file_field}\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        (format!("multipart/form-data; boundary={boundary}"), body)
    }

    async fn extract_post_media(
        &self,
        access_token: &str,
        message_id: &str,
        post_payload: &Value,
    ) -> Result<Vec<String>> {
        let root = post_payload.get("post").unwrap_or(post_payload);
        let Some(root_object) = root.as_object() else {
            return Ok(Vec::new());
        };
        let mut media = Vec::new();
        for locale in root_object.values() {
            let Some(rows) = locale.get("content").and_then(Value::as_array) else {
                continue;
            };
            for row in rows {
                let Some(elements) = row.as_array() else {
                    continue;
                };
                for element in elements {
                    let Some("img") = element.get("tag").and_then(Value::as_str) else {
                        continue;
                    };
                    let Some(image_key) = element.get("image_key").and_then(Value::as_str) else {
                        continue;
                    };
                    if image_key.trim().is_empty() {
                        continue;
                    }
                    media.push(
                        self.download_message_resource(
                            access_token,
                            message_id,
                            image_key,
                            "image",
                            "post-image",
                        )
                        .await?,
                    );
                }
            }
        }
        Ok(media)
    }

    async fn normalize_inbound_content(
        &self,
        access_token: &str,
        message_id: &str,
        message_type: &str,
        raw_content: &str,
    ) -> Result<Option<NormalizedInboundContent>> {
        match message_type {
            "text" => {
                let payload: Value =
                    serde_json::from_str(raw_content).context("invalid feishu text content")?;
                Ok(payload.get("text").and_then(Value::as_str).map(|text| {
                    NormalizedInboundContent {
                        content: text.trim().to_string(),
                        media: Vec::new(),
                    }
                }))
            }
            "post" => {
                let payload: Value =
                    serde_json::from_str(raw_content).context("invalid feishu post content")?;
                let mut content = extract_post_text(&payload);
                let media = self
                    .extract_post_media(access_token, message_id, &payload)
                    .await?;
                if content.trim().is_empty() && !media.is_empty() {
                    content = "[post]".to_string();
                }
                Ok(Some(NormalizedInboundContent { content, media }))
            }
            "image" | "audio" | "file" | "media" => {
                let payload: Value = serde_json::from_str(raw_content)
                    .with_context(|| format!("invalid feishu {message_type} content"))?;
                let (key_field, resource_type, label) = if message_type == "image" {
                    ("image_key", "image", "image")
                } else {
                    ("file_key", "file", message_type)
                };
                let mut media = Vec::new();
                if let Some(resource_key) = payload.get(key_field).and_then(Value::as_str) {
                    if !resource_key.trim().is_empty() {
                        media.push(
                            self.download_message_resource(
                                access_token,
                                message_id,
                                resource_key,
                                resource_type,
                                label,
                            )
                            .await?,
                        );
                    }
                }
                let content = payload
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|text| text.trim().to_string())
                    .filter(|text| !text.is_empty())
                    .or_else(|| placeholder_message(message_type))
                    .unwrap_or_else(|| format!("[{message_type}]"));
                Ok(Some(NormalizedInboundContent { content, media }))
            }
            other => Ok(
                placeholder_message(other).map(|content| NormalizedInboundContent {
                    content,
                    media: Vec::new(),
                }),
            ),
        }
    }

    async fn run_session(&self) -> Result<()> {
        let bot_open_id = self.bot_open_id().await?;
        let session_config = self.ws_session_config().await?;
        info!(
            connect_url = %sanitize_url_for_log(session_config.connect_url.as_str()),
            service_id = session_config.service_id,
            "feishu websocket connecting"
        );
        let (stream, _) = connect_async(session_config.connect_url.as_str())
            .await
            .with_context(|| format!("failed to connect to {}", session_config.connect_url))?;
        info!(
            connect_url = %sanitize_url_for_log(session_config.connect_url.as_str()),
            service_id = session_config.service_id,
            "feishu websocket connected"
        );
        let (mut writer, mut reader) = stream.split();
        let mut ping_timer = time::interval(session_config.ping_interval);
        ping_timer.set_missed_tick_behavior(MissedTickBehavior::Delay);

        while self.running.load(Ordering::SeqCst) {
            tokio::select! {
                _ = ping_timer.tick() => {
                    self.send_ping_frame(&mut writer, session_config.service_id).await?;
                }
                incoming = reader.next() => {
                    let frame = match incoming {
                        Some(Ok(frame)) => frame,
                        Some(Err(error)) => return Err(error.into()),
                        None => bail!("feishu websocket closed"),
                    };

                    match frame {
                        Message::Binary(bytes) => {
                            if let Err(error) = self
                                .handle_binary_frame(&mut writer, bytes.to_vec(), &bot_open_id)
                                .await
                            {
                                warn!("feishu dropped invalid frame: {error}");
                            }
                        }
                        Message::Text(text) => {
                            if let Err(error) = self.handle_text_frame(text.as_ref(), &bot_open_id).await {
                                warn!("feishu dropped invalid frame: {error}");
                            }
                        }
                        Message::Ping(payload) => {
                            writer
                                .send(Message::Pong(payload))
                                .await
                                .context("failed to respond to feishu websocket ping")?;
                        }
                        Message::Close(_) => bail!("feishu websocket closed"),
                        _ => {}
                    }
                }
            }
        }

        Ok(())
    }

    async fn send_ping_frame<S>(&self, writer: &mut S, service_id: i32) -> Result<()>
    where
        S: SinkExt<Message> + Unpin,
        S::Error: Into<anyhow::Error>,
    {
        let frame = FeishuWsFrame {
            seq_id: 0,
            log_id: 0,
            service: service_id,
            method: FEISHU_FRAME_METHOD_CONTROL,
            headers: vec![FeishuWsHeader {
                key: FEISHU_HEADER_TYPE.to_string(),
                value: FEISHU_PING_TYPE.to_string(),
            }],
            payload_encoding: String::new(),
            payload_type: String::new(),
            payload: Vec::new(),
            log_id_new: String::new(),
        };
        writer
            .send(Message::Binary(encode_frame(&frame).into()))
            .await
            .map_err(Into::into)
            .context("failed to send feishu ping frame")?;
        debug!(service_id, "feishu websocket ping sent");
        Ok(())
    }

    async fn handle_binary_frame<S>(
        &self,
        writer: &mut S,
        bytes: Vec<u8>,
        bot_open_id: &str,
    ) -> Result<()>
    where
        S: SinkExt<Message> + Unpin,
        S::Error: Into<anyhow::Error>,
    {
        let frame =
            FeishuWsFrame::decode(bytes.as_slice()).context("invalid feishu protobuf frame")?;
        match frame.method {
            FEISHU_FRAME_METHOD_CONTROL => {
                debug!(
                    frame_type = frame_header(&frame, FEISHU_HEADER_TYPE).unwrap_or_default(),
                    service_id = frame.service,
                    "feishu control frame received"
                );
                if frame_header(&frame, FEISHU_HEADER_TYPE) == Some(FEISHU_PONG_TYPE) {
                    debug!(service_id = frame.service, "feishu websocket pong received");
                    return Ok(());
                }
                Ok(())
            }
            FEISHU_FRAME_METHOD_DATA => {
                debug!(
                    frame_type = frame_header(&frame, FEISHU_HEADER_TYPE).unwrap_or_default(),
                    message_id = frame_header(&frame, "message_id").unwrap_or_default(),
                    trace_id = frame_header(&frame, "trace_id").unwrap_or_default(),
                    service_id = frame.service,
                    "feishu data frame received"
                );
                let status_code = match self.handle_event_frame(&frame, bot_open_id).await {
                    Ok(()) => FEISHU_HTTP_OK,
                    Err(error) => {
                        warn!("feishu event frame handling failed: {error}");
                        FEISHU_HTTP_INTERNAL_ERROR
                    }
                };
                let mut ack_frame = frame.clone();
                ack_frame.headers.push(FeishuWsHeader {
                    key: FEISHU_HEADER_BIZ_RT.to_string(),
                    value: "0".to_string(),
                });
                ack_frame.payload = format!("{{\"code\":{status_code}}}").into_bytes();
                writer
                    .send(Message::Binary(encode_frame(&ack_frame).into()))
                    .await
                    .map_err(Into::into)
                    .context("failed to send feishu event ack")?;
                debug!(
                    status_code,
                    message_id = frame_header(&frame, "message_id").unwrap_or_default(),
                    trace_id = frame_header(&frame, "trace_id").unwrap_or_default(),
                    "feishu event ack sent"
                );
                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn handle_event_frame(&self, frame: &FeishuWsFrame, bot_open_id: &str) -> Result<()> {
        if frame_header(frame, FEISHU_HEADER_TYPE) != Some(FEISHU_EVENT_TYPE) {
            return Ok(());
        }
        let text = std::str::from_utf8(frame.payload.as_slice())
            .context("invalid feishu payload encoding")?;
        self.handle_text_frame(text, bot_open_id).await
    }

    async fn handle_text_frame(&self, text: &str, bot_open_id: &str) -> Result<()> {
        let payload: Value = serde_json::from_str(text).context("invalid feishu json payload")?;
        let Some(event) = payload.get("event") else {
            return Ok(());
        };

        let sender_type = event
            .pointer("/sender/sender_type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let sender_id = event
            .pointer("/sender/sender_id/open_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if sender_type == "bot" || sender_id.is_empty() || sender_id == bot_open_id {
            debug!(
                sender_type,
                sender_id = %redact_identifier(sender_id),
                "feishu inbound ignored: bot or invalid sender"
            );
            return Ok(());
        }
        if !is_allowed_sender(&self.config, sender_id) {
            debug!(
                sender_id = %redact_identifier(sender_id),
                allow_from = %summarize_allowlist(self.config.allow_from.as_slice()),
                "feishu inbound ignored: sender not allowed"
            );
            return Ok(());
        }

        let message_id = event
            .pointer("/message/message_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let message_type = event
            .pointer("/message/message_type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let chat_id = event
            .pointer("/message/chat_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let chat_type = event
            .pointer("/message/chat_type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let raw_content = event
            .pointer("/message/content")
            .and_then(Value::as_str)
            .unwrap_or("{}");
        let mentions = event
            .pointer("/message/mentions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        if message_id.is_empty() || chat_id.is_empty() {
            debug!(
                message_id,
                chat_id = %redact_identifier(chat_id),
                "feishu inbound ignored: missing message or chat id"
            );
            return Ok(());
        }
        if chat_type != "p2p"
            && !is_group_message_for_bot(
                raw_content,
                mentions.as_slice(),
                Some(bot_open_id),
                self.config.group_policy.as_str(),
            )
        {
            debug!(
                message_id,
                chat_id = %redact_identifier(chat_id),
                chat_type,
                group_policy = self.config.group_policy.as_str(),
                "feishu inbound ignored: group policy not matched"
            );
            return Ok(());
        }

        let access_token = self.tenant_access_token().await?;
        let NormalizedInboundContent { content, media } = match self
            .normalize_inbound_content(&access_token, message_id, message_type, raw_content)
            .await?
        {
            Some(content) => content,
            None => {
                debug!(
                    message_id,
                    chat_id = %redact_identifier(chat_id),
                    message_type,
                    "feishu inbound ignored: unsupported message type"
                );
                return Ok(());
            }
        };
        if content.trim().is_empty() && media.is_empty() {
            debug!(
                message_id,
                chat_id = %redact_identifier(chat_id),
                message_type,
                "feishu inbound ignored: empty content"
            );
            return Ok(());
        }

        {
            let mut dedup = self.dedup.lock().await;
            if !dedup.insert(message_id) {
                debug!(message_id, "feishu inbound ignored: duplicate message id");
                return Ok(());
            }
        }

        let parent_id = event.pointer("/message/parent_id").and_then(Value::as_str);
        let root_id = event.pointer("/message/root_id").and_then(Value::as_str);
        let content_chars = content.chars().count();
        let mut metadata = HashMap::new();
        metadata.insert("message_id".to_string(), json!(message_id));
        metadata.insert("chat_type".to_string(), json!(chat_type));
        metadata.insert("msg_type".to_string(), json!(message_type));
        metadata.insert("parent_id".to_string(), json!(parent_id));
        metadata.insert("root_id".to_string(), json!(root_id));

        self.bus
            .publish_inbound(InboundMessage {
                channel: "feishu".to_string(),
                sender_id: sender_id.to_string(),
                chat_id: if chat_type == "p2p" {
                    sender_id.to_string()
                } else {
                    chat_id.to_string()
                },
                content,
                media,
                timestamp: chrono::Utc::now(),
                metadata,
                session_key_override: None,
            })
            .await?;
        info!(
            message_id,
            sender_id = %redact_identifier(sender_id),
            chat_id = %redact_identifier(chat_id),
            chat_type,
            message_type,
            parent_id = parent_id.unwrap_or_default(),
            root_id = root_id.unwrap_or_default(),
            content_chars,
            "feishu inbound accepted"
        );

        if let Err(error) = self.add_reaction(message_id).await {
            warn!("feishu reaction failed for {message_id}: {error}");
        } else if !self.config.react_emoji.trim().is_empty() {
            debug!(
                message_id,
                react_emoji = self.config.react_emoji.as_str(),
                "feishu reaction sent"
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeishuMessageFormat {
    Text,
    Post,
    Interactive,
}

#[derive(Debug)]
struct RecentMessageDedup {
    capacity: usize,
    order: VecDeque<String>,
    seen: HashSet<String>,
}

impl RecentMessageDedup {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            order: VecDeque::new(),
            seen: HashSet::new(),
        }
    }

    fn insert(&mut self, message_id: &str) -> bool {
        if self.seen.contains(message_id) {
            return false;
        }
        let message_id = message_id.to_string();
        self.order.push_back(message_id.clone());
        self.seen.insert(message_id);
        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
            }
        }
        true
    }
}

fn normalize_ws_config_endpoint(raw: &str) -> Result<Url> {
    let mut url =
        Url::parse(raw).map_err(|error| anyhow::anyhow!("invalid feishu ws_base: {error}"))?;
    match url.scheme() {
        "http" | "https" => {}
        "ws" => {
            url.set_scheme("http")
                .map_err(|_| anyhow::anyhow!("invalid feishu ws_base scheme"))?;
        }
        "wss" => {
            url.set_scheme("https")
                .map_err(|_| anyhow::anyhow!("invalid feishu ws_base scheme"))?;
        }
        _ => bail!("invalid feishu ws_base scheme"),
    }

    let path = url.path().trim_end_matches('/');
    let endpoint_path = if path.is_empty() || path == "/" {
        "/callback/ws/endpoint".to_string()
    } else if path.ends_with("/callback/ws/endpoint") {
        path.to_string()
    } else if path.ends_with("/open-apis/ws") {
        join_feishu_path(
            &path[..path.len() - "/open-apis/ws".len()],
            "/callback/ws/endpoint",
        )
    } else if path.ends_with("/open-apis") {
        join_feishu_path(
            &path[..path.len() - "/open-apis".len()],
            "/callback/ws/endpoint",
        )
    } else if path.ends_with("/ws") {
        join_feishu_path(&path[..path.len() - "/ws".len()], "/callback/ws/endpoint")
    } else {
        join_feishu_path(path, "/callback/ws/endpoint")
    };
    url.set_path(endpoint_path.as_str());
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn join_feishu_path(prefix: &str, suffix: &str) -> String {
    let prefix = prefix.trim_end_matches('/');
    if prefix.is_empty() {
        suffix.to_string()
    } else {
        format!("{prefix}{suffix}")
    }
}

fn frame_header<'a>(frame: &'a FeishuWsFrame, key: &str) -> Option<&'a str> {
    frame
        .headers
        .iter()
        .find(|header| header.key == key)
        .map(|header| header.value.as_str())
}

fn encode_frame(frame: &FeishuWsFrame) -> Vec<u8> {
    frame.encode_to_vec()
}

fn redact_identifier(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    if value.chars().count() <= 8 {
        return value.to_string();
    }
    let prefix: String = value.chars().take(4).collect();
    let suffix: String = value
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{prefix}...{suffix}")
}

fn summarize_allowlist(allow_from: &[String]) -> String {
    match allow_from {
        [] => "empty".to_string(),
        [only] if only == "*" => "wildcard".to_string(),
        values if values.iter().any(|value| value == "*") => {
            format!("{} entries including wildcard", values.len())
        }
        values => format!("{} entries", values.len()),
    }
}

fn sanitize_url_for_log(raw: &str) -> String {
    let Ok(mut url) = Url::parse(raw) else {
        return raw.to_string();
    };
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

fn markdown_link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[([^\]]+)\]\((https?://[^\)]+)\)").expect("markdown link"))
}

fn heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^#{1,6}\s+.+$").expect("heading"))
}

fn unordered_list_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^[ \t]*[-*+]\s+").expect("unordered list"))
}

fn ordered_list_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^[ \t]*\d+\.\s+").expect("ordered list"))
}

fn bold_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\*\*.+?\*\*|__.+?__").expect("bold"))
}

fn strike_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"~~.+?~~").expect("strike"))
}

fn table_block_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?m)((?:^[ \t]*\|.+\|[ \t]*\n)(?:^[ \t]*\|[-:\s|]+\|[ \t]*\n)(?:^[ \t]*\|.+\|[ \t]*(?:\n|$))+)",
        )
        .expect("table")
    })
}

fn code_block_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)```.*?```").expect("code block"))
}

fn has_italic_marker(text: &str) -> bool {
    let bytes = text.as_bytes();
    for start in 0..bytes.len() {
        if bytes[start] != b'*' {
            continue;
        }
        if start > 0 && bytes[start - 1] == b'*' {
            continue;
        }
        if start + 1 >= bytes.len() || bytes[start + 1] == b'*' {
            continue;
        }
        for end in start + 1..bytes.len() {
            if bytes[end] != b'*' {
                continue;
            }
            if bytes[end - 1] == b'*' {
                continue;
            }
            if end + 1 < bytes.len() && bytes[end + 1] == b'*' {
                continue;
            }
            return true;
        }
    }
    false
}

fn detect_message_format(content: &str) -> FeishuMessageFormat {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return FeishuMessageFormat::Text;
    }

    let has_complex_markdown = trimmed.contains("```")
        || heading_re().is_match(trimmed)
        || table_block_re().is_match(trimmed)
        || unordered_list_re().is_match(trimmed)
        || ordered_list_re().is_match(trimmed)
        || bold_re().is_match(trimmed)
        || has_italic_marker(trimmed)
        || strike_re().is_match(trimmed);

    if has_complex_markdown || trimmed.len() > 2000 {
        return FeishuMessageFormat::Interactive;
    }
    if markdown_link_re().is_match(trimmed) || trimmed.len() > 200 {
        return FeishuMessageFormat::Post;
    }
    FeishuMessageFormat::Text
}

fn render_post_body(content: &str) -> String {
    let paragraphs: Vec<Vec<Value>> = content
        .trim()
        .split('\n')
        .map(|line| {
            let mut elements = Vec::new();
            let mut last_end = 0;
            for captures in markdown_link_re().captures_iter(line) {
                let full = captures.get(0).expect("full match");
                if full.start() > last_end {
                    elements.push(json!({
                        "tag": "text",
                        "text": &line[last_end..full.start()],
                    }));
                }
                elements.push(json!({
                    "tag": "a",
                    "text": captures.get(1).expect("link text").as_str(),
                    "href": captures.get(2).expect("href").as_str(),
                }));
                last_end = full.end();
            }
            if last_end < line.len() {
                elements.push(json!({
                    "tag": "text",
                    "text": &line[last_end..],
                }));
            }
            if elements.is_empty() {
                elements.push(json!({
                    "tag": "text",
                    "text": "",
                }));
            }
            elements
        })
        .collect();

    json!({
        "zh_cn": {
            "content": paragraphs
        }
    })
    .to_string()
}

fn strip_md_formatting(text: &str) -> String {
    let mut result = text.to_string();
    result = result.replace("**", "").replace("__", "");
    result = result.replace('*', "");
    result = result.replace("~~", "");
    result
}

fn parse_md_table(table_text: &str) -> Value {
    let lines: Vec<&str> = table_text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let split_row = |line: &str| -> Vec<String> {
        line.trim()
            .trim_matches('|')
            .split('|')
            .map(|cell| strip_md_formatting(cell.trim()))
            .collect()
    };

    let headers = split_row(lines[0]);
    let rows = lines[2..]
        .iter()
        .map(|line| split_row(line))
        .collect::<Vec<_>>();

    json!({
        "tag": "table",
        "page_size": rows.len() + 1,
        "columns": headers.iter().enumerate().map(|(index, header)| json!({
            "tag": "column",
            "name": format!("c{index}"),
            "display_name": header,
            "width": "auto"
        })).collect::<Vec<_>>(),
        "rows": rows.iter().map(|row| {
            let mut map = serde_json::Map::new();
            for (index, header) in headers.iter().enumerate() {
                let _ = header;
                map.insert(
                    format!("c{index}"),
                    Value::String(row.get(index).cloned().unwrap_or_default()),
                );
            }
            Value::Object(map)
        }).collect::<Vec<_>>()
    })
}

fn split_headings(content: &str) -> Vec<Value> {
    let mut protected = content.to_string();
    let mut code_blocks = Vec::new();
    for captures in code_block_re().find_iter(content) {
        code_blocks.push(captures.as_str().to_string());
        protected = protected.replacen(
            captures.as_str(),
            &format!("\u{0}CODE{}\u{0}", code_blocks.len() - 1),
            1,
        );
    }

    let mut elements = Vec::new();
    let mut last = 0;
    for heading in heading_re().find_iter(&protected) {
        let before = protected[last..heading.start()].trim();
        if !before.is_empty() {
            elements.push(json!({
                "tag": "markdown",
                "content": before,
            }));
        }

        let heading_text = heading.as_str().trim_start_matches('#').trim();
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**{}**", strip_md_formatting(heading_text)),
            }
        }));
        last = heading.end();
    }
    let remaining = protected[last..].trim();
    if !remaining.is_empty() {
        elements.push(json!({
            "tag": "markdown",
            "content": remaining,
        }));
    }

    for (index, code_block) in code_blocks.iter().enumerate() {
        let placeholder = format!("\u{0}CODE{index}\u{0}");
        for element in &mut elements {
            if let Some(content) = element.get_mut("content") {
                if let Some(as_str) = content.as_str() {
                    *content = Value::String(as_str.replace(&placeholder, code_block));
                }
            }
        }
    }

    if elements.is_empty() {
        vec![json!({
            "tag": "markdown",
            "content": content,
        })]
    } else {
        elements
    }
}

fn build_card_elements(content: &str) -> Vec<Value> {
    let mut elements = Vec::new();
    let mut last = 0;
    for table in table_block_re().find_iter(content) {
        let before = content[last..table.start()].trim();
        if !before.is_empty() {
            elements.extend(split_headings(before));
        }
        elements.push(parse_md_table(table.as_str()));
        last = table.end();
    }
    let remaining = content[last..].trim();
    if !remaining.is_empty() {
        elements.extend(split_headings(remaining));
    }
    if elements.is_empty() {
        vec![json!({
            "tag": "markdown",
            "content": content,
        })]
    } else {
        elements
    }
}

fn render_interactive_cards(content: &str) -> Vec<String> {
    let elements = build_card_elements(content);
    let mut groups: Vec<Vec<Value>> = Vec::new();
    let mut current = Vec::new();
    let mut table_count = 0;

    for element in elements {
        let is_table = element.get("tag").and_then(Value::as_str) == Some("table");
        if is_table && table_count >= 1 {
            groups.push(current);
            current = Vec::new();
            table_count = 0;
        }
        if is_table {
            table_count += 1;
        }
        current.push(element);
    }
    if !current.is_empty() {
        groups.push(current);
    }

    groups
        .into_iter()
        .map(|group| {
            json!({
                "config": { "wide_screen_mode": true },
                "elements": group,
            })
            .to_string()
        })
        .collect()
}

fn extract_post_text(payload: &Value) -> String {
    let root = payload.get("post").unwrap_or(payload);
    let Some(root_object) = root.as_object() else {
        return String::new();
    };

    let block = ["zh_cn", "en_us", "ja_jp"]
        .iter()
        .find_map(|key| root_object.get(*key).and_then(Value::as_object))
        .or_else(|| root_object.values().find_map(|value| value.as_object()));

    let Some(block) = block else {
        return String::new();
    };

    let mut lines = Vec::new();
    if let Some(title) = block.get("title").and_then(Value::as_str) {
        if !title.trim().is_empty() {
            lines.push(title.trim().to_string());
        }
    }

    if let Some(rows) = block.get("content").and_then(Value::as_array) {
        for row in rows {
            let Some(elements) = row.as_array() else {
                continue;
            };
            let mut row_parts = Vec::new();
            for element in elements {
                let Some(tag) = element.get("tag").and_then(Value::as_str) else {
                    continue;
                };
                match tag {
                    "text" => {
                        if let Some(text) = element.get("text").and_then(Value::as_str) {
                            row_parts.push(text.to_string());
                        }
                    }
                    "a" => {
                        if let Some(text) = element.get("text").and_then(Value::as_str) {
                            row_parts.push(text.to_string());
                        }
                    }
                    "at" => {
                        let user_name = element
                            .get("user_name")
                            .and_then(Value::as_str)
                            .unwrap_or("user");
                        row_parts.push(format!("@{user_name}"));
                    }
                    "code_block" => {
                        let language = element
                            .get("language")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let text = element.get("text").and_then(Value::as_str).unwrap_or("");
                        row_parts.push(format!("```{language}\n{text}\n```"));
                    }
                    "img" => {}
                    _ => {}
                }
            }
            let row_text = row_parts.join(" ").trim().to_string();
            if !row_text.is_empty() {
                lines.push(row_text);
            }
        }
    }

    lines.join("\n").trim().to_string()
}

fn is_allowed_sender(config: &FeishuConfig, sender_id: &str) -> bool {
    if config.allow_from.is_empty() {
        return false;
    }
    config
        .allow_from
        .iter()
        .any(|allowed| allowed == "*" || allowed == sender_id)
}

fn is_group_message_for_bot(
    raw_content: &str,
    mentions: &[Value],
    bot_open_id: Option<&str>,
    policy: &str,
) -> bool {
    if policy == "open" {
        return true;
    }
    if raw_content.contains("@_all") {
        return true;
    }
    let Some(bot_open_id) = bot_open_id else {
        return false;
    };
    mentions.iter().any(|mention| {
        mention
            .pointer("/id/open_id")
            .and_then(Value::as_str)
            .map(|open_id| open_id == bot_open_id)
            .unwrap_or(false)
    })
}

fn resolve_receive_id_type(chat_id: &str) -> &'static str {
    if chat_id.starts_with("ou_") {
        "open_id"
    } else {
        "chat_id"
    }
}

fn placeholder_message(message_type: &str) -> Option<String> {
    match message_type {
        "image"
        | "audio"
        | "file"
        | "sticker"
        | "interactive"
        | "share_chat"
        | "share_user"
        | "share_calendar_event"
        | "system"
        | "merge_forward" => Some(format!("[{message_type}]")),
        _ => None,
    }
}

#[async_trait::async_trait]
impl Channel for FeishuChannel {
    fn name(&self) -> &'static str {
        "feishu"
    }

    async fn start(&self) -> Result<()> {
        self.validate_startup_config()?;
        info!(
            app_id = %redact_identifier(self.config.app_id.as_str()),
            api_base = self.config.api_base.as_str(),
            ws_base = %sanitize_url_for_log(self.config.ws_base.as_str()),
            allow_from = %summarize_allowlist(self.config.allow_from.as_slice()),
            group_policy = self.config.group_policy.as_str(),
            reply_to_message = self.config.reply_to_message,
            react_emoji = self.config.react_emoji.as_str(),
            "starting feishu channel"
        );
        self.running.store(true, Ordering::SeqCst);
        let _ = self.bot_open_id().await?;
        while self.running.load(Ordering::SeqCst) {
            if let Err(error) = self.run_session().await {
                warn!("feishu channel session ended: {error}");
            }
            if !self.running.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(FEISHU_RECONNECT_DELAY).await;
        }
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);
        info!("stopping feishu channel");
        Ok(())
    }

    async fn send(&self, msg: OutboundMessage) -> Result<()> {
        if !should_deliver_to_channel("feishu", &msg.metadata) {
            return Ok(());
        }

        if msg.chat_id.trim().is_empty() {
            bail!("feishu chat_id is required");
        }

        let access_token = self.tenant_access_token().await?;
        let _ = self.bot_open_id().await?;
        let receive_id_type = resolve_receive_id_type(&msg.chat_id);

        let mut payloads: Vec<(String, String)> = Vec::new();
        for media_path in &msg.media {
            let uploaded = if Self::upload_is_image(media_path) {
                self.upload_image(&access_token, media_path).await?
            } else {
                self.upload_file(&access_token, media_path).await?
            };
            payloads.push(uploaded);
        }
        let text_payloads: Vec<(String, String)> = match detect_message_format(&msg.content) {
            FeishuMessageFormat::Text => vec![(
                "text".to_string(),
                json!({
                    "text": msg.content.trim(),
                })
                .to_string(),
            )],
            FeishuMessageFormat::Post => vec![("post".to_string(), render_post_body(&msg.content))],
            FeishuMessageFormat::Interactive => render_interactive_cards(&msg.content)
                .into_iter()
                .map(|card| ("interactive".to_string(), card))
                .collect(),
        };
        payloads.extend(text_payloads);

        let reply_message_id = if self.config.reply_to_message
            && !msg
                .metadata
                .get("_progress")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            && !msg
                .metadata
                .get("_tool_hint")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            msg.metadata.get("message_id").and_then(Value::as_str)
        } else {
            None
        };

        info!(
            receive_id_type,
            chat_id = %redact_identifier(msg.chat_id.as_str()),
            media_count = msg.media.len(),
            payload_count = payloads.len(),
            reply_message_id = %reply_message_id
                .map(redact_identifier)
                .unwrap_or_else(|| "none".to_string()),
            content_chars = msg.content.chars().count(),
            "feishu outbound prepared"
        );

        for (index, (msg_type, content)) in payloads.iter().enumerate() {
            let use_reply = index == 0 && reply_message_id.is_some();
            if use_reply {
                let message_id = reply_message_id.expect("reply id");
                debug!(
                    message_id = %redact_identifier(message_id),
                    msg_type,
                    content_chars = content.chars().count(),
                    "feishu outbound reply attempt"
                );
                if self
                    .reply_message(&access_token, message_id, msg_type.as_str(), content)
                    .await
                    .is_ok()
                {
                    info!(
                        message_id = %redact_identifier(message_id),
                        msg_type,
                        "feishu outbound replied"
                    );
                    continue;
                }
                warn!(
                    message_id = %redact_identifier(message_id),
                    msg_type,
                    "feishu reply failed, falling back to create message"
                );
            }
            debug!(
                receive_id_type,
                chat_id = %redact_identifier(msg.chat_id.as_str()),
                msg_type,
                content_chars = content.chars().count(),
                "feishu outbound create attempt"
            );
            self.create_message(
                &access_token,
                receive_id_type,
                &msg.chat_id,
                msg_type.as_str(),
                content,
            )
            .await?;
            info!(
                receive_id_type,
                chat_id = %redact_identifier(msg.chat_id.as_str()),
                msg_type,
                "feishu outbound sent"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn feishu_detects_plain_long_content_as_post() {
        let content = "a".repeat(300);
        assert_eq!(detect_message_format(&content), FeishuMessageFormat::Post);
    }

    #[test]
    fn feishu_detects_code_block_as_interactive() {
        assert_eq!(
            detect_message_format("```rust\nfn main() {}\n```"),
            FeishuMessageFormat::Interactive
        );
    }

    #[test]
    fn feishu_detects_heading_as_interactive() {
        assert_eq!(
            detect_message_format("# Title\nbody"),
            FeishuMessageFormat::Interactive
        );
    }

    #[test]
    fn feishu_detects_markdown_link_as_post() {
        assert_eq!(
            detect_message_format("[docs](https://example.com)"),
            FeishuMessageFormat::Post
        );
    }

    #[test]
    fn feishu_detects_threshold_boundaries() {
        assert_eq!(
            detect_message_format(&"a".repeat(200)),
            FeishuMessageFormat::Text
        );
        assert_eq!(
            detect_message_format(&"a".repeat(201)),
            FeishuMessageFormat::Post
        );
        assert_eq!(
            detect_message_format(&"a".repeat(2001)),
            FeishuMessageFormat::Interactive
        );
    }

    #[test]
    fn feishu_detects_lists_and_style_markers_as_interactive() {
        assert_eq!(
            detect_message_format("- item"),
            FeishuMessageFormat::Interactive
        );
        assert_eq!(
            detect_message_format("1. item"),
            FeishuMessageFormat::Interactive
        );
        assert_eq!(
            detect_message_format("**bold**"),
            FeishuMessageFormat::Interactive
        );
        assert_eq!(
            detect_message_format("*italic*"),
            FeishuMessageFormat::Interactive
        );
        assert_eq!(
            detect_message_format("~~strike~~"),
            FeishuMessageFormat::Interactive
        );
    }

    #[test]
    fn feishu_detects_tables_and_splits_multiple_tables() {
        let table = "| a | b |\n|---|---|\n| 1 | 2 |";
        assert_eq!(
            detect_message_format(table),
            FeishuMessageFormat::Interactive
        );
        assert_eq!(
            render_interactive_cards(&format!("{table}\n\n{table}")).len(),
            2
        );
    }

    #[test]
    fn feishu_renders_interactive_cards_with_required_schema() {
        let cards = render_interactive_cards("# Title\n\n| a | b |\n|---|---|\n| 1 | 2 |");
        let first: Value = serde_json::from_str(&cards[0]).expect("card json");
        assert_eq!(first["config"]["wide_screen_mode"], true);
        assert!(first["elements"].as_array().is_some());
    }

    #[test]
    fn feishu_flattens_post_content_deterministically() {
        let payload = json!({
            "zh_cn": {
                "title": "Title",
                "content": [[
                    {"tag": "text", "text": "hello"},
                    {"tag": "a", "text": "docs", "href": "https://example.com"},
                    {"tag": "at", "user_name": "bot"}
                ]]
            }
        });

        assert_eq!(extract_post_text(&payload), "Title\nhello docs @bot");
    }

    #[test]
    fn feishu_flattens_wrapped_post_with_locale_fallback() {
        let payload = json!({
            "post": {
                "en_us": {
                    "content": [[{"tag": "text", "text": "english"}]]
                }
            }
        });

        assert_eq!(extract_post_text(&payload), "english");
    }

    #[test]
    fn feishu_flattens_wrapped_post_with_ja_locale_fallback() {
        let payload = json!({
            "post": {
                "ja_jp": {
                    "content": [[{"tag": "text", "text": "japanese"}]]
                }
            }
        });

        assert_eq!(extract_post_text(&payload), "japanese");
    }

    #[test]
    fn feishu_flattens_wrapped_post_with_first_object_locale_fallback() {
        let payload = json!({
            "post": {
                "custom_locale": {
                    "content": [[{"tag": "text", "text": "custom"}]]
                }
            }
        });

        assert_eq!(extract_post_text(&payload), "custom");
    }

    #[test]
    fn feishu_flattens_post_code_blocks_and_ignores_images() {
        let payload = json!({
            "zh_cn": {
                "content": [[
                    {"tag": "code_block", "language": "rust", "text": "fn main() {}"},
                    {"tag": "img", "image_key": "img_1"}
                ]]
            }
        });

        assert_eq!(extract_post_text(&payload), "```rust\nfn main() {}\n```");
    }

    #[test]
    fn feishu_allowlist_denies_empty_and_accepts_wildcard() {
        let mut config = FeishuConfig::default();
        assert!(!is_allowed_sender(&config, "ou_user_1"));
        config.allow_from = vec!["*".to_string()];
        assert!(is_allowed_sender(&config, "ou_user_1"));
    }

    #[test]
    fn feishu_open_group_policy_accepts_unmentioned_group_messages() {
        assert!(is_group_message_for_bot(
            "hello group",
            &[],
            Some("ou_bot_1"),
            "open"
        ));
    }

    #[test]
    fn feishu_mention_group_policy_requires_all_or_bot_open_id() {
        assert!(!is_group_message_for_bot(
            "hello group",
            &[],
            Some("ou_bot_1"),
            "mention",
        ));
        assert!(is_group_message_for_bot(
            "@_all hello",
            &[],
            Some("ou_bot_1"),
            "mention",
        ));
        assert!(is_group_message_for_bot(
            "hello",
            &[json!({"id": {"open_id": "ou_bot_1"}})],
            Some("ou_bot_1"),
            "mention",
        ));
    }

    #[test]
    fn feishu_resolves_receive_id_type_by_chat_id_prefix() {
        assert_eq!(resolve_receive_id_type("ou_user_1"), "open_id");
        assert_eq!(resolve_receive_id_type("oc_group_1"), "chat_id");
    }

    #[test]
    fn feishu_dedup_cache_evicts_oldest_entries() {
        let mut dedup = RecentMessageDedup::new(2);
        assert!(dedup.insert("m1"));
        assert!(dedup.insert("m2"));
        assert!(!dedup.insert("m1"));
        assert!(dedup.insert("m3"));
        assert!(dedup.insert("m1"));
    }

    #[test]
    fn feishu_redacts_identifiers_for_logs() {
        assert_eq!(redact_identifier(""), "");
        assert_eq!(redact_identifier("ou_user"), "ou_user");
        assert_eq!(redact_identifier("ou_123456789"), "ou_1...6789");
    }

    #[test]
    fn feishu_summarizes_allowlists_for_logs() {
        assert_eq!(summarize_allowlist(&[]), "empty");
        assert_eq!(summarize_allowlist(&["*".to_string()]), "wildcard");
        assert_eq!(
            summarize_allowlist(&["ou_1".to_string(), "ou_2".to_string()]),
            "2 entries"
        );
        assert_eq!(
            summarize_allowlist(&["ou_1".to_string(), "*".to_string()]),
            "2 entries including wildcard"
        );
    }

    #[test]
    fn feishu_sanitizes_sensitive_ws_query_params_in_logs() {
        assert_eq!(
            sanitize_url_for_log(
                "wss://msg-frontier.feishu.cn/ws/v2?device_id=1&access_key=secret&ticket=secret"
            ),
            "wss://msg-frontier.feishu.cn/ws/v2"
        );
    }
}
