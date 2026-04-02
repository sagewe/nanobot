mod feishu;
mod wecom;
pub mod weixin;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::fs;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::bus::{InboundMessage, MessageBus, OutboundMessage};
use crate::channels::weixin::WeixinAccountStore;
use crate::config::{Config, TelegramConfig};
use crate::presentation::{
    render_telegram_html, should_deliver_to_channel, split_telegram_html_chunks,
    telegram_message_limit,
};
pub use feishu::FeishuChannel;
pub use wecom::{
    ParsedWecomTextCallback, WecomBotChannel, WecomTiming, build_wecom_markdown_reply_request,
    build_wecom_ping_request, build_wecom_subscribe_request, parse_wecom_text_callback,
};
pub use weixin::WeixinChannel;

const DEFAULT_DELIVERY_QUEUE_CAPACITY: usize = 32;
const DEFAULT_DELIVERY_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

#[async_trait]
pub trait Channel: Send + Sync {
    fn name(&self) -> &'static str;
    async fn start(&self) -> Result<()>;
    async fn stop(&self) -> Result<()>;
    async fn send(&self, msg: OutboundMessage) -> Result<()>;
}

#[derive(Default)]
pub struct CliChannel;

#[async_trait]
impl Channel for CliChannel {
    fn name(&self) -> &'static str {
        "cli"
    }

    async fn start(&self) -> Result<()> {
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        Ok(())
    }

    async fn send(&self, _msg: OutboundMessage) -> Result<()> {
        Ok(())
    }
}

pub struct TelegramChannel {
    config: TelegramConfig,
    bus: MessageBus,
    client: reqwest::Client,
    running: AtomicBool,
    offset: Arc<Mutex<i64>>,
}

struct DeliveryWorkerEntry {
    sender: mpsc::UnboundedSender<OutboundMessage>,
    handle: JoinHandle<()>,
}

impl TelegramChannel {
    pub fn new(config: TelegramConfig, bus: MessageBus) -> Self {
        Self {
            config,
            bus,
            client: reqwest::Client::new(),
            running: AtomicBool::new(false),
            offset: Arc::new(Mutex::new(0)),
        }
    }

    fn is_allowed(&self, sender_id: &str, username: Option<&str>) -> bool {
        if self.config.allow_from.is_empty() {
            return false;
        }
        if self.config.allow_from.iter().any(|value| value == "*") {
            return true;
        }
        self.config.allow_from.iter().any(|allowed| {
            allowed == sender_id
                || username
                    .map(|username| allowed == username)
                    .unwrap_or(false)
        })
    }

    fn base_url(&self, method: &str) -> String {
        format!(
            "{}/bot{}/{}",
            self.config.api_base.trim_end_matches('/'),
            self.config.token,
            method
        )
    }

    fn media_dir(&self) -> PathBuf {
        std::env::temp_dir().join("sidekick-telegram-media")
    }

    fn attachment_method(media_path: &str) -> &'static str {
        let extension = Path::new(media_path)
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase());
        match extension.as_deref() {
            Some("jpg") | Some("jpeg") | Some("png") | Some("gif") | Some("webp") => "sendPhoto",
            Some("ogg") | Some("oga") | Some("opus") => "sendVoice",
            Some("mp3") | Some("m4a") | Some("wav") | Some("flac") | Some("aac") => "sendAudio",
            _ => "sendDocument",
        }
    }

    fn message_field_as_string(message: &Value, path: &str) -> Option<String> {
        message
            .pointer(path)
            .and_then(Value::as_i64)
            .map(|value| value.to_string())
            .or_else(|| {
                message
                    .pointer(path)
                    .and_then(Value::as_str)
                    .map(|value| value.to_string())
            })
    }

    async fn download_media(&self, file_id: &str, message_id: &str, label: &str) -> Result<String> {
        let response = self
            .client
            .post(self.base_url("getFile"))
            .json(&json!({ "file_id": file_id }))
            .send()
            .await?
            .error_for_status()?;
        let payload: Value = response.json().await?;
        let file_path = payload
            .pointer("/result/file_path")
            .and_then(Value::as_str)
            .unwrap_or(file_id);
        let download_url = format!(
            "{}/file/bot{}/{}",
            self.config.api_base.trim_end_matches('/'),
            self.config.token,
            file_path
        );
        let bytes = self
            .client
            .get(download_url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        let mut local_path = self.media_dir();
        let file_name = Path::new(file_path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("media.bin");
        local_path.push(format!("{message_id}-{label}-{file_name}"));
        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&local_path, bytes).await?;
        Ok(local_path.display().to_string())
    }

    async fn publish_inbound_telegram_message(&self, message: &Value) -> Result<()> {
        let chat = message.get("chat");
        let chat_id = chat
            .and_then(|chat| chat.get("id"))
            .and_then(Value::as_i64)
            .map(|id| id.to_string());
        let sender_id = message
            .get("from")
            .and_then(|from| from.get("id"))
            .and_then(Value::as_i64)
            .map(|id| id.to_string());
        let username = message
            .get("from")
            .and_then(|from| from.get("username"))
            .and_then(Value::as_str);
        let Some(chat_id) = chat_id else {
            return Ok(());
        };
        let Some(sender_id) = sender_id else {
            return Ok(());
        };
        if !self.is_allowed(&sender_id, username) {
            return Ok(());
        }

        let message_id = Self::message_field_as_string(message, "/message_id").unwrap_or_default();
        let content = message
            .get("caption")
            .and_then(Value::as_str)
            .or_else(|| message.get("text").and_then(Value::as_str))
            .unwrap_or_default()
            .to_string();

        let mut media = Vec::new();
        if let Some(file_id) = message
            .get("photo")
            .and_then(Value::as_array)
            .and_then(|items| items.last())
            .and_then(|item| item.get("file_id"))
            .and_then(Value::as_str)
        {
            media.push(self.download_media(file_id, &message_id, "photo").await?);
        } else if let Some(file_id) = message
            .get("voice")
            .and_then(|value| value.get("file_id"))
            .and_then(Value::as_str)
        {
            media.push(self.download_media(file_id, &message_id, "voice").await?);
        } else if let Some(file_id) = message
            .get("audio")
            .and_then(|value| value.get("file_id"))
            .and_then(Value::as_str)
        {
            media.push(self.download_media(file_id, &message_id, "audio").await?);
        } else if let Some(file_id) = message
            .get("document")
            .and_then(|value| value.get("file_id"))
            .and_then(Value::as_str)
        {
            media.push(
                self.download_media(file_id, &message_id, "document")
                    .await?,
            );
        }

        let reply_to_message_id = message
            .pointer("/reply_to_message/message_id")
            .and_then(Value::as_i64)
            .map(|value| value.to_string())
            .or_else(|| {
                message
                    .get("reply_to_message_id")
                    .and_then(Value::as_i64)
                    .map(|value| value.to_string())
            });
        let message_thread_id = Self::message_field_as_string(message, "/message_thread_id");
        let media_group_id = message
            .get("media_group_id")
            .and_then(Value::as_str)
            .map(|value| value.to_string());
        let chat_is_forum = chat
            .and_then(|chat| chat.get("is_forum"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let is_topic_message = message
            .get("is_topic_message")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let session_key_override = if chat_is_forum {
            message_thread_id
                .as_ref()
                .map(|thread_id| format!("telegram:{chat_id}:{thread_id}"))
        } else {
            None
        };

        let mut metadata = HashMap::new();
        metadata.insert("message_id".to_string(), json!(message_id));
        if let Some(reply_to_message_id) = reply_to_message_id {
            metadata.insert(
                "reply_to_message_id".to_string(),
                json!(reply_to_message_id),
            );
        }
        if let Some(message_thread_id) = message_thread_id {
            metadata.insert("message_thread_id".to_string(), json!(message_thread_id));
        }
        if let Some(media_group_id) = media_group_id {
            metadata.insert("media_group_id".to_string(), json!(media_group_id));
        }
        if let Some(username) = username {
            metadata.insert("username".to_string(), json!(username));
        }
        metadata.insert("chat_is_forum".to_string(), json!(chat_is_forum));
        metadata.insert("is_topic_message".to_string(), json!(is_topic_message));

        self.bus
            .publish_inbound(InboundMessage {
                channel: "telegram".to_string(),
                sender_id,
                chat_id,
                content,
                media,
                timestamp: chrono::Utc::now(),
                metadata,
                session_key_override,
            })
            .await?;
        Ok(())
    }

    fn media_payload(
        msg: &OutboundMessage,
        media_key: &str,
        media_path: &str,
        caption: Option<&str>,
    ) -> Value {
        let mut payload = serde_json::Map::new();
        payload.insert("chat_id".to_string(), json!(msg.chat_id));
        payload.insert(media_key.to_string(), json!(media_path));
        if let Some(caption) = caption {
            payload.insert("caption".to_string(), json!(caption));
            payload.insert("parse_mode".to_string(), json!("HTML"));
        }
        if let Some(message_thread_id) = msg.metadata.get("message_thread_id") {
            payload.insert("message_thread_id".to_string(), message_thread_id.clone());
        }
        if let Some(reply_to_message_id) = msg.metadata.get("reply_to_message_id") {
            payload.insert(
                "reply_to_message_id".to_string(),
                reply_to_message_id.clone(),
            );
        }
        Value::Object(payload)
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &'static str {
        "telegram"
    }

    async fn start(&self) -> Result<()> {
        self.running.store(true, Ordering::SeqCst);
        while self.running.load(Ordering::SeqCst) {
            let offset = *self.offset.lock().await;
            let response = self
                .client
                .post(self.base_url("getUpdates"))
                .json(&json!({
                    "timeout": 20,
                    "offset": offset,
                    "allowed_updates": ["message"]
                }))
                .send()
                .await;
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    warn!("telegram polling failed: {error}");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
            };
            let payload: Value = match response.json().await {
                Ok(value) => value,
                Err(error) => {
                    warn!("telegram returned invalid payload: {error}");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
            };
            let Some(results) = payload.get("result").and_then(Value::as_array) else {
                continue;
            };
            for update in results {
                if let Some(update_id) = update.get("update_id").and_then(Value::as_i64) {
                    *self.offset.lock().await = update_id + 1;
                }
                let Some(message) = update.get("message") else {
                    continue;
                };
                if let Err(error) = self.publish_inbound_telegram_message(message).await {
                    warn!("telegram inbound handling failed: {error}");
                }
            }
        }
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    async fn send(&self, msg: OutboundMessage) -> Result<()> {
        if !should_deliver_to_channel("telegram", &msg.metadata) {
            return Ok(());
        }
        if msg.media.is_empty() {
            let rendered = render_telegram_html(&msg.content);
            for chunk in split_telegram_html_chunks(&rendered, telegram_message_limit()) {
                let mut payload = serde_json::Map::new();
                payload.insert("chat_id".to_string(), json!(msg.chat_id));
                payload.insert("text".to_string(), json!(chunk));
                payload.insert("parse_mode".to_string(), json!("HTML"));
                if let Some(message_thread_id) = msg.metadata.get("message_thread_id") {
                    payload.insert("message_thread_id".to_string(), message_thread_id.clone());
                }
                if let Some(reply_to_message_id) = msg.metadata.get("reply_to_message_id") {
                    payload.insert(
                        "reply_to_message_id".to_string(),
                        reply_to_message_id.clone(),
                    );
                }
                self.client
                    .post(self.base_url("sendMessage"))
                    .json(&Value::Object(payload))
                    .send()
                    .await?
                    .error_for_status()?;
            }
            return Ok(());
        }

        for (index, media_path) in msg.media.iter().enumerate() {
            let method = Self::attachment_method(media_path);
            let media_key = match method {
                "sendPhoto" => "photo",
                "sendVoice" => "voice",
                "sendAudio" => "audio",
                _ => "document",
            };
            let caption = if index == 0 && !msg.content.trim().is_empty() {
                Some(render_telegram_html(&msg.content))
            } else {
                None
            };
            self.client
                .post(self.base_url(method))
                .json(&Self::media_payload(
                    &msg,
                    media_key,
                    media_path,
                    caption.as_deref(),
                ))
                .send()
                .await?
                .error_for_status()?;
        }
        Ok(())
    }
}

pub struct ChannelManager {
    bus: MessageBus,
    channels: HashMap<String, Arc<dyn Channel>>,
    dispatch_handle: Mutex<Option<JoinHandle<()>>>,
    start_handles: Mutex<Vec<JoinHandle<()>>>,
    delivery_workers: Arc<Mutex<HashMap<String, DeliveryWorkerEntry>>>,
    delivery_worker_seq: Arc<AtomicU64>,
    delivery_worker_queue_capacity: usize,
    delivery_worker_idle_timeout: Duration,
}

impl ChannelManager {
    pub fn new(config: &Config, bus: MessageBus) -> Self {
        let mut channels: HashMap<String, Arc<dyn Channel>> = HashMap::new();
        channels.insert("cli".to_string(), Arc::new(CliChannel));
        if config.channels.telegram.enabled {
            channels.insert(
                "telegram".to_string(),
                Arc::new(TelegramChannel::new(
                    config.channels.telegram.clone(),
                    bus.clone(),
                )),
            );
        }
        if config.channels.wecom.enabled {
            channels.insert(
                "wecom".to_string(),
                Arc::new(WecomBotChannel::new(
                    config.channels.wecom.clone(),
                    bus.clone(),
                )),
            );
        }
        if config.channels.feishu.enabled {
            channels.insert(
                "feishu".to_string(),
                Arc::new(FeishuChannel::new(
                    config.channels.feishu.clone(),
                    bus.clone(),
                )),
            );
        }
        if config.channels.weixin.enabled {
            match WeixinAccountStore::new(&config.workspace_path()) {
                Ok(store) => {
                    channels.insert(
                        "weixin".to_string(),
                        Arc::new(WeixinChannel::new(
                            config.channels.weixin.clone(),
                            store,
                            bus.clone(),
                        )),
                    );
                }
                Err(error) => {
                    error!("failed to initialize weixin channel store: {error}");
                }
            }
        }
        Self::with_channels_for_test(
            channels,
            bus,
            DEFAULT_DELIVERY_QUEUE_CAPACITY,
            DEFAULT_DELIVERY_IDLE_TIMEOUT,
        )
    }

    pub fn with_channels_for_test(
        channels: HashMap<String, Arc<dyn Channel>>,
        bus: MessageBus,
        worker_queue_capacity: usize,
        worker_idle_timeout: Duration,
    ) -> Self {
        Self {
            bus,
            channels,
            dispatch_handle: Mutex::new(None),
            start_handles: Mutex::new(Vec::new()),
            delivery_workers: Arc::new(Mutex::new(HashMap::new())),
            delivery_worker_seq: Arc::new(AtomicU64::new(0)),
            delivery_worker_queue_capacity: worker_queue_capacity.max(1),
            delivery_worker_idle_timeout: worker_idle_timeout,
        }
    }

    pub fn enabled_channels(&self) -> Vec<String> {
        self.channels.keys().cloned().collect()
    }

    pub async fn delivery_worker_count(&self) -> usize {
        self.delivery_workers.lock().await.len()
    }

    pub async fn start_all(&self) {
        let channels = self.channels.values().cloned().collect::<Vec<_>>();
        let mut handles = self.start_handles.lock().await;
        for channel in channels {
            let name = channel.name();
            handles.push(tokio::spawn(async move {
                if let Err(error) = channel.start().await {
                    error!("channel {name} failed: {error}");
                }
            }));
        }
        let bus = self.bus.clone();
        let channels = self.channels.clone();
        let delivery_workers = self.delivery_workers.clone();
        let delivery_worker_seq = self.delivery_worker_seq.clone();
        let delivery_worker_queue_capacity = self.delivery_worker_queue_capacity;
        let delivery_worker_idle_timeout = self.delivery_worker_idle_timeout;
        *self.dispatch_handle.lock().await = Some(tokio::spawn(async move {
            loop {
                let Some(msg) = bus.consume_outbound().await else {
                    continue;
                };
                if let Some(channel) = channels.get(&msg.channel) {
                    Self::dispatch_outbound(
                        channel.clone(),
                        msg,
                        delivery_workers.clone(),
                        delivery_worker_seq.clone(),
                        delivery_worker_queue_capacity,
                        delivery_worker_idle_timeout,
                    )
                    .await;
                } else {
                    warn!("unknown channel: {}", msg.channel);
                }
            }
        }));
        info!("channels started: {:?}", self.enabled_channels());
    }

    pub async fn stop_all(&self) {
        for channel in self.channels.values() {
            let _ = channel.stop().await;
        }
        if let Some(handle) = self.dispatch_handle.lock().await.take() {
            handle.abort();
        }
        let mut handles = self.start_handles.lock().await;
        for handle in handles.drain(..) {
            handle.abort();
        }
        let mut workers = self.delivery_workers.lock().await;
        for (_, worker) in workers.drain() {
            worker.handle.abort();
        }
    }

    async fn dispatch_outbound(
        channel: Arc<dyn Channel>,
        msg: OutboundMessage,
        delivery_workers: Arc<Mutex<HashMap<String, DeliveryWorkerEntry>>>,
        delivery_worker_seq: Arc<AtomicU64>,
        delivery_worker_queue_capacity: usize,
        delivery_worker_idle_timeout: Duration,
    ) {
        let delivery_key = format!("{}:{}", msg.channel, msg.chat_id);
        let sender = Self::delivery_worker_for_key(
            delivery_key.clone(),
            channel.clone(),
            delivery_workers.clone(),
            delivery_worker_seq.clone(),
            delivery_worker_queue_capacity,
            delivery_worker_idle_timeout,
        )
        .await;
        if sender.send(msg).is_err() {
            warn!("delivery worker closed for {delivery_key}; outbound message was not delivered");
        }
    }

    async fn delivery_worker_for_key(
        delivery_key: String,
        channel: Arc<dyn Channel>,
        delivery_workers: Arc<Mutex<HashMap<String, DeliveryWorkerEntry>>>,
        delivery_worker_seq: Arc<AtomicU64>,
        delivery_worker_queue_capacity: usize,
        delivery_worker_idle_timeout: Duration,
    ) -> mpsc::UnboundedSender<OutboundMessage> {
        let mut workers = delivery_workers.lock().await;
        if let Some(existing) = workers.get(&delivery_key) {
            return existing.sender.clone();
        }

        let _ = delivery_worker_seq;
        let _ = delivery_worker_queue_capacity;
        let _ = delivery_worker_idle_timeout;
        let (sender, receiver) = mpsc::unbounded_channel();
        let worker_key = delivery_key.clone();
        let workers_for_task = delivery_workers.clone();
        let channel_for_task = channel.clone();
        let handle = tokio::spawn(async move {
            Self::run_delivery_worker(worker_key, channel_for_task, receiver, workers_for_task)
                .await;
        });
        workers.insert(
            delivery_key,
            DeliveryWorkerEntry {
                sender: sender.clone(),
                handle,
            },
        );
        sender
    }

    async fn run_delivery_worker(
        delivery_key: String,
        channel: Arc<dyn Channel>,
        mut receiver: mpsc::UnboundedReceiver<OutboundMessage>,
        delivery_workers: Arc<Mutex<HashMap<String, DeliveryWorkerEntry>>>,
    ) {
        while let Some(message) = receiver.recv().await {
            if let Err(error) = channel.send(message).await {
                error!("failed to send outbound message: {error}");
            }
        }

        let mut workers = delivery_workers.lock().await;
        workers.remove(&delivery_key);
    }
}
