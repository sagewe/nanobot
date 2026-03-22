use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use nanobot_rs::bus::{MessageBus, OutboundMessage};
use nanobot_rs::channels::{Channel, ChannelManager, TelegramChannel};
use nanobot_rs::config::{Config, TelegramConfig};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

#[derive(Clone, Default)]
struct TelegramState {
    updates: Arc<Mutex<Vec<Value>>>,
    sent: Arc<Mutex<Vec<Value>>>,
}

async fn get_updates(
    State(state): State<TelegramState>,
    Json(_payload): Json<Value>,
) -> Json<Value> {
    let updates = state.updates.lock().await.clone();
    Json(json!({"ok": true, "result": updates}))
}

async fn send_message(
    State(state): State<TelegramState>,
    Json(payload): Json<Value>,
) -> Json<Value> {
    state.sent.lock().await.push(payload);
    Json(json!({"ok": true, "result": {"message_id": 1}}))
}

async fn start_server(state: TelegramState) -> SocketAddr {
    let app = Router::new()
        .route("/bottoken/getUpdates", post(get_updates))
        .route("/bottoken/sendMessage", post(send_message))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    addr
}

#[tokio::test]
async fn channel_manager_only_enables_configured_channels() {
    let config = Config::default();
    let manager = ChannelManager::new(&config, MessageBus::new(32));
    assert_eq!(manager.enabled_channels(), vec!["cli".to_string()]);
}

#[tokio::test]
async fn channel_manager_registers_wecom_when_enabled() {
    let mut config = Config::default();
    config.channels.wecom.enabled = true;
    config.channels.wecom.bot_id = "bot".to_string();
    config.channels.wecom.secret = "secret".to_string();

    let manager = ChannelManager::new(&config, MessageBus::new(32));
    assert!(manager.enabled_channels().contains(&"wecom".to_string()));
}

#[tokio::test]
async fn channel_manager_registers_weixin_when_enabled() {
    let mut config = Config::default();
    config.channels.weixin.enabled = true;

    let manager = ChannelManager::new(&config, MessageBus::new(32));
    assert!(manager.enabled_channels().contains(&"weixin".to_string()));
}

#[tokio::test]
async fn telegram_channel_receives_allowed_text_messages() {
    let state = TelegramState {
        updates: Arc::new(Mutex::new(vec![json!({
            "update_id": 1,
            "message": {
                "message_id": 10,
                "text": "hello",
                "chat": {"id": 99},
                "from": {"id": 42, "username": "alice"}
            }
        })])),
        sent: Arc::new(Mutex::new(Vec::new())),
    };
    let addr = start_server(state.clone()).await;
    let bus = MessageBus::new(32);
    let channel = TelegramChannel::new(
        TelegramConfig {
            enabled: true,
            token: "token".to_string(),
            allow_from: vec!["42".to_string()],
            api_base: format!("http://{addr}"),
        },
        bus.clone(),
    );
    let handle = tokio::spawn({
        let channel = channel;
        async move { channel.start().await.expect("telegram start") }
    });
    let inbound = tokio::time::timeout(std::time::Duration::from_secs(2), bus.consume_inbound())
        .await
        .expect("timely inbound")
        .expect("message");
    assert_eq!(inbound.channel, "telegram");
    assert_eq!(inbound.content, "hello");
    handle.abort();
}

#[tokio::test]
async fn telegram_channel_sends_outbound_text() {
    let state = TelegramState::default();
    let addr = start_server(state.clone()).await;
    let bus = MessageBus::new(32);
    let channel = TelegramChannel::new(
        TelegramConfig {
            enabled: true,
            token: "token".to_string(),
            allow_from: vec!["*".to_string()],
            api_base: format!("http://{addr}"),
        },
        bus,
    );
    channel
        .send(OutboundMessage {
            channel: "telegram".to_string(),
            chat_id: "123".to_string(),
            content: "hello".to_string(),
            metadata: HashMap::new(),
        })
        .await
        .expect("send");
    let sent = state.sent.lock().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].get("text").and_then(Value::as_str), Some("hello"));
}

#[tokio::test]
async fn telegram_channel_drops_runtime_messages() {
    let state = TelegramState::default();
    let addr = start_server(state.clone()).await;
    let bus = MessageBus::new(32);
    let channel = TelegramChannel::new(
        TelegramConfig {
            enabled: true,
            token: "token".to_string(),
            allow_from: vec!["*".to_string()],
            api_base: format!("http://{addr}"),
        },
        bus,
    );

    channel
        .send(OutboundMessage {
            channel: "telegram".to_string(),
            chat_id: "123".to_string(),
            content: "message(\"telegram\")".to_string(),
            metadata: HashMap::from([("_progress".to_string(), json!(true))]),
        })
        .await
        .expect("send");

    let sent = state.sent.lock().await;
    assert!(sent.is_empty());
}

#[test]
fn default_config_includes_weixin_channel() {
    let config = nanobot_rs::config::Config::default();
    assert!(!config.channels.weixin.enabled);
    assert_eq!(
        config.channels.weixin.api_base,
        "https://ilinkai.weixin.qq.com"
    );
    assert_eq!(
        config.channels.weixin.cdn_base,
        "https://novac2c.cdn.weixin.qq.com/c2c"
    );
}

#[tokio::test]
async fn telegram_channel_sends_rendered_html() {
    let state = TelegramState::default();
    let addr = start_server(state.clone()).await;
    let bus = MessageBus::new(32);
    let channel = TelegramChannel::new(
        TelegramConfig {
            enabled: true,
            token: "token".to_string(),
            allow_from: vec!["*".to_string()],
            api_base: format!("http://{addr}"),
        },
        bus,
    );

    channel
        .send(OutboundMessage {
            channel: "telegram".to_string(),
            chat_id: "123".to_string(),
            content: "**hello** `code` [link](https://example.com)".to_string(),
            metadata: HashMap::new(),
        })
        .await
        .expect("send");

    let sent = state.sent.lock().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].get("parse_mode").and_then(Value::as_str),
        Some("HTML")
    );
    assert_eq!(
        sent[0].get("text").and_then(Value::as_str),
        Some("<b>hello</b> <code>code</code> <a href=\"https://example.com\">link</a>")
    );
}
