use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail, ensure};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc};
use tokio::time::MissedTickBehavior;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::bus::{InboundMessage, MessageBus, OutboundMessage};
use crate::channels::Channel;
use crate::config::WecomConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedWecomTextCallback {
    pub req_id: String,
    pub sender_id: String,
    pub chat_id: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy)]
pub struct WecomTiming {
    pub heartbeat_interval: Duration,
    pub heartbeat_timeout: Duration,
    pub reconnect_delay: Duration,
}

impl Default for WecomTiming {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(30),
            heartbeat_timeout: Duration::from_secs(90),
            reconnect_delay: Duration::from_secs(1),
        }
    }
}

impl WecomTiming {
    pub fn for_tests() -> Self {
        Self {
            heartbeat_interval: Duration::from_millis(100),
            heartbeat_timeout: Duration::from_millis(350),
            reconnect_delay: Duration::from_millis(100),
        }
    }
}

#[derive(Debug, Clone)]
struct ReplyContext {
    req_id: String,
}

pub struct WecomBotChannel {
    config: WecomConfig,
    bus: MessageBus,
    running: AtomicBool,
    timing: WecomTiming,
    writer: Mutex<Option<mpsc::UnboundedSender<Value>>>,
    reply_contexts: Mutex<HashMap<String, ReplyContext>>,
}

impl WecomBotChannel {
    pub fn new(config: WecomConfig, bus: MessageBus) -> Self {
        Self::new_with_timing(config, bus, WecomTiming::default())
    }

    pub fn new_with_timing(config: WecomConfig, bus: MessageBus, timing: WecomTiming) -> Self {
        Self {
            config,
            bus,
            running: AtomicBool::new(false),
            timing,
            writer: Mutex::new(None),
            reply_contexts: Mutex::new(HashMap::new()),
        }
    }

    fn validate_config(&self) -> Result<()> {
        ensure!(
            !self.config.bot_id.trim().is_empty(),
            "wecom bot_id is required when the channel is enabled"
        );
        ensure!(
            !self.config.secret.trim().is_empty(),
            "wecom secret is required when the channel is enabled"
        );
        ensure!(
            !self.config.ws_base.trim().is_empty(),
            "wecom ws_base is required when the channel is enabled"
        );
        Ok(())
    }

    fn is_allowed(&self, sender_id: &str) -> bool {
        self.config.allow_from.is_empty()
            || self
                .config
                .allow_from
                .iter()
                .any(|allowed| allowed == sender_id)
    }

    async fn run_session(&self) -> Result<()> {
        info!("wecom connecting to {}", self.config.ws_base);
        let (stream, _) = connect_async(self.config.ws_base.as_str())
            .await
            .with_context(|| format!("failed to connect to {}", self.config.ws_base))?;
        info!("wecom websocket connected");
        let (mut writer, mut reader) = stream.split();

        let subscribe_req_id = Uuid::new_v4().to_string();
        writer
            .send(Message::Text(
                build_wecom_subscribe_request(
                    &self.config.bot_id,
                    &self.config.secret,
                    &subscribe_req_id,
                )
                .to_string()
                .into(),
            ))
            .await
            .context("failed to send wecom subscribe request")?;

        let subscribe_frame = reader
            .next()
            .await
            .ok_or_else(|| anyhow!("wecom closed before subscribe completed"))??;
        let subscribe_payload = parse_json_frame(subscribe_frame)?;
        ensure!(
            subscribe_payload
                .get("errcode")
                .and_then(Value::as_i64)
                .unwrap_or_default()
                == 0,
            "wecom subscribe failed: {}",
            subscribe_payload
                .get("errmsg")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
        );
        info!("wecom subscribe acknowledged");

        let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
        *self.writer.lock().await = Some(tx);

        let mut heartbeat = tokio::time::interval(self.timing.heartbeat_interval);
        heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut last_pong = Instant::now();

        loop {
            tokio::select! {
                Some(outbound) = rx.recv() => {
                    writer.send(Message::Text(outbound.to_string().into())).await
                        .context("failed to send outbound wecom message")?;
                }
                _ = heartbeat.tick() => {
                    if last_pong.elapsed() > self.timing.heartbeat_timeout {
                        bail!("wecom heartbeat timed out");
                    }
                    writer.send(Message::Text(
                        build_wecom_ping_request(&Uuid::new_v4().to_string()).to_string().into()
                    ))
                    .await
                    .context("failed to send wecom ping")?;
                }
                frame = reader.next() => {
                    let frame = match frame {
                        Some(Ok(frame)) => frame,
                        Some(Err(error)) => return Err(error.into()),
                        None => bail!("wecom websocket closed"),
                    };
                    match frame {
                        Message::Text(text) => {
                            self.handle_text_frame(text.as_ref(), &mut last_pong).await?;
                        }
                        Message::Ping(payload) => {
                            writer.send(Message::Pong(payload)).await
                                .context("failed to respond to websocket ping")?;
                        }
                        Message::Close(_) => bail!("wecom websocket closed"),
                        _ => {}
                    }
                }
            }

            if !self.running.load(Ordering::SeqCst) {
                break;
            }
        }

        Ok(())
    }

    async fn handle_text_frame(&self, text: &str, last_pong: &mut Instant) -> Result<()> {
        let payload: Value = serde_json::from_str(text).context("invalid wecom json payload")?;

        if payload.get("cmd").and_then(Value::as_str) == Some("pong") {
            *last_pong = Instant::now();
            debug!("wecom pong received");
            return Ok(());
        }

        if let Some(parsed) = parse_wecom_text_callback(&payload) {
            if !self.is_allowed(&parsed.sender_id) {
                debug!(
                    "dropping wecom message from blocked sender {}",
                    parsed.sender_id
                );
                return Ok(());
            }

            info!(
                "wecom text callback sender={} chat={}",
                parsed.sender_id, parsed.chat_id
            );

            self.reply_contexts.lock().await.insert(
                parsed.chat_id.clone(),
                ReplyContext {
                    req_id: parsed.req_id.clone(),
                },
            );
            debug!(
                "wecom reply context updated chat={} req_id={}",
                parsed.chat_id, parsed.req_id
            );

            let mut metadata = HashMap::new();
            metadata.insert("req_id".to_string(), json!(parsed.req_id));
            if let Some(msg_id) = payload.pointer("/body/msgid").and_then(Value::as_str) {
                metadata.insert("msg_id".to_string(), json!(msg_id));
            }

            self.bus
                .publish_inbound(InboundMessage {
                    channel: "wecom".to_string(),
                    sender_id: parsed.sender_id,
                    chat_id: parsed.chat_id,
                    content: parsed.content,
                    timestamp: chrono::Utc::now(),
                    metadata,
                    session_key_override: None,
                })
                .await?;
        }

        Ok(())
    }

    async fn clear_connection_state(&self) {
        *self.writer.lock().await = None;
    }
}

pub fn build_wecom_subscribe_request(bot_id: &str, secret: &str, req_id: &str) -> Value {
    json!({
        "cmd": "aibot_subscribe",
        "headers": {
            "req_id": req_id,
        },
        "body": {
            "bot_id": bot_id,
            "secret": secret,
        }
    })
}

pub fn build_wecom_ping_request(req_id: &str) -> Value {
    json!({
        "cmd": "ping",
        "headers": {
            "req_id": req_id,
        }
    })
}

pub fn build_wecom_stream_reply_request(
    req_id: &str,
    stream_id: &str,
    content: &str,
    finish: bool,
) -> Value {
    json!({
        "cmd": "aibot_respond_msg",
        "headers": {
            "req_id": req_id,
        },
        "body": {
            "msgtype": "stream",
            "stream": {
                "id": stream_id,
                "finish": finish,
                "content": content,
            }
        }
    })
}

pub fn parse_wecom_text_callback(payload: &Value) -> Option<ParsedWecomTextCallback> {
    if payload.get("cmd").and_then(Value::as_str) != Some("aibot_msg_callback") {
        return None;
    }

    if payload.pointer("/body/msgtype").and_then(Value::as_str) != Some("text") {
        return None;
    }

    let req_id = payload.pointer("/headers/req_id").and_then(Value::as_str)?;
    let sender_id = payload
        .pointer("/body/from/userid")
        .and_then(Value::as_str)?;
    let content = payload
        .pointer("/body/text/content")
        .and_then(Value::as_str)?;
    let chat_id = payload
        .pointer("/body/chatid")
        .and_then(Value::as_str)
        .unwrap_or(sender_id);

    Some(ParsedWecomTextCallback {
        req_id: req_id.to_string(),
        sender_id: sender_id.to_string(),
        chat_id: chat_id.to_string(),
        content: content.to_string(),
    })
}

#[async_trait::async_trait]
impl Channel for WecomBotChannel {
    fn name(&self) -> &'static str {
        "wecom"
    }

    async fn start(&self) -> Result<()> {
        self.validate_config()?;
        self.running.store(true, Ordering::SeqCst);
        while self.running.load(Ordering::SeqCst) {
            match self.run_session().await {
                Ok(()) => {}
                Err(error) => {
                    warn!("wecom channel session ended: {error}");
                }
            }
            self.clear_connection_state().await;
            if !self.running.load(Ordering::SeqCst) {
                break;
            }
            info!(
                "wecom reconnecting in {:?} after: previous session ended",
                self.timing.reconnect_delay
            );
            tokio::time::sleep(self.timing.reconnect_delay).await;
        }
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);
        self.clear_connection_state().await;
        info!("wecom channel stopped");
        Ok(())
    }

    async fn send(&self, msg: OutboundMessage) -> Result<()> {
        let reply_context = self
            .reply_contexts
            .lock()
            .await
            .get(&msg.chat_id)
            .cloned()
            .ok_or_else(|| anyhow!("missing reply context for wecom chat {}", msg.chat_id))?;

        let writer = self
            .writer
            .lock()
            .await
            .clone()
            .ok_or_else(|| anyhow!("wecom channel is not connected"))?;

        writer
            .send(build_wecom_stream_reply_request(
                &reply_context.req_id,
                &Uuid::new_v4().to_string(),
                &msg.content,
                true,
            ))
            .map_err(|_| anyhow!("wecom writer is closed"))?;
        info!("wecom reply sent chat={}", msg.chat_id);
        Ok(())
    }
}

fn parse_json_frame(frame: Message) -> Result<Value> {
    match frame {
        Message::Text(text) => serde_json::from_str(text.as_ref()).context("invalid wecom json"),
        other => Err(anyhow!("unexpected websocket frame: {other:?}")),
    }
}
