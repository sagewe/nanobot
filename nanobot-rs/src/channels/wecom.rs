use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use serde_json::{Value, json};

use crate::bus::{MessageBus, OutboundMessage};
use crate::channels::Channel;
use crate::config::WecomConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedWecomTextCallback {
    pub req_id: String,
    pub sender_id: String,
    pub chat_id: String,
    pub content: String,
}

pub struct WecomBotChannel {
    #[allow(dead_code)]
    config: WecomConfig,
    #[allow(dead_code)]
    bus: MessageBus,
    running: AtomicBool,
}

impl WecomBotChannel {
    pub fn new(config: WecomConfig, bus: MessageBus) -> Self {
        Self {
            config,
            bus,
            running: AtomicBool::new(false),
        }
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
    let sender_id = payload.pointer("/body/from/userid").and_then(Value::as_str)?;
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
        self.running.store(true, Ordering::SeqCst);
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
