use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use chrono::Utc;
use serde_json::{Value, json};
use sidekick::bus::{MessageBus, OutboundMessage};
use sidekick::channels::weixin::{WeixinAccountState, WeixinAccountStore, WeixinChannel};
use sidekick::channels::{Channel, ChannelManager, FeishuChannel, TelegramChannel};
use sidekick::config::{Config, FeishuConfig, TelegramConfig, WeixinConfig};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify};

#[derive(Clone)]
struct MockChannel {
    name: &'static str,
    state: Arc<MockChannelState>,
}

#[derive(Default)]
struct MockChannelState {
    events: Mutex<Vec<String>>,
    block_chat_id: Option<String>,
    block_once: bool,
    blocked_once: AtomicBool,
    release_requested: AtomicBool,
    release: Notify,
}

impl MockChannel {
    fn new(name: &'static str, block_chat_id: Option<&str>) -> Self {
        Self {
            name,
            state: Arc::new(MockChannelState {
                block_chat_id: block_chat_id.map(|value| value.to_string()),
                block_once: true,
                blocked_once: AtomicBool::new(false),
                release_requested: AtomicBool::new(false),
                ..Default::default()
            }),
        }
    }
}

impl MockChannelState {
    async fn events(&self) -> Vec<String> {
        self.events.lock().await.clone()
    }

    fn release_blocked(&self) {
        self.release_requested.store(true, Ordering::SeqCst);
        self.release.notify_waiters();
    }
}

#[async_trait::async_trait]
impl Channel for MockChannel {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn start(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn send(&self, msg: OutboundMessage) -> anyhow::Result<()> {
        self.state
            .events
            .lock()
            .await
            .push(format!("start:{}:{}", msg.chat_id, msg.content));

        let should_block = self
            .state
            .block_chat_id
            .as_ref()
            .map(|chat_id| chat_id == &msg.chat_id)
            .unwrap_or(false)
            && (!self.state.block_once || !self.state.blocked_once.swap(true, Ordering::SeqCst));
        if should_block {
            while !self.state.release_requested.load(Ordering::SeqCst) {
                self.state.release.notified().await;
            }
        }

        self.state
            .events
            .lock()
            .await
            .push(format!("done:{}:{}", msg.chat_id, msg.content));
        Ok(())
    }
}

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

async fn weixin_send_message(
    State(state): State<TelegramState>,
    Json(payload): Json<Value>,
) -> Json<Value> {
    state.sent.lock().await.push(payload);
    Json(json!({"errcode": 0}))
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

async fn start_weixin_server(state: TelegramState) -> SocketAddr {
    let app = Router::new()
        .route("/ilink/bot/sendmessage", post(weixin_send_message))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    addr
}

async fn start_mock_manager(
    block_chat_id: Option<&str>,
    worker_queue_capacity: usize,
    worker_idle_timeout: Duration,
) -> (ChannelManager, MessageBus, Arc<MockChannelState>) {
    let bus = MessageBus::new(32);
    let channel = MockChannel::new("mock", block_chat_id);
    let state = channel.state.clone();
    let manager = ChannelManager::with_channels_for_test(
        HashMap::from([("mock".to_string(), Arc::new(channel) as Arc<dyn Channel>)]),
        bus.clone(),
        worker_queue_capacity,
        worker_idle_timeout,
    );
    (manager, bus, state)
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
async fn channel_manager_registers_feishu_when_enabled() {
    let mut config = Config::default();
    config.channels.feishu.enabled = true;
    config.channels.feishu.app_id = "cli_a1".to_string();
    config.channels.feishu.app_secret = "secret".to_string();

    let manager = ChannelManager::new(&config, MessageBus::new(32));
    assert!(manager.enabled_channels().contains(&"feishu".to_string()));
}

fn base_feishu_config() -> FeishuConfig {
    FeishuConfig {
        enabled: true,
        app_id: "cli_a1".to_string(),
        app_secret: "secret".to_string(),
        api_base: "https://open.feishu.cn/open-apis".to_string(),
        ws_base: "wss://open.feishu.cn/open-apis/ws".to_string(),
        encrypt_key: String::new(),
        verification_token: String::new(),
        allow_from: vec!["*".to_string()],
        react_emoji: "THUMBSUP".to_string(),
        group_policy: "mention".to_string(),
        reply_to_message: false,
    }
}

#[tokio::test]
async fn feishu_channel_start_requires_credentials() {
    let bus = MessageBus::new(32);
    let mut config = base_feishu_config();
    config.app_id.clear();
    let channel = FeishuChannel::new(config, bus);

    let error = channel.start().await.expect_err("missing credentials");
    assert!(error.to_string().contains("feishu"));
}

#[tokio::test]
async fn feishu_channel_start_rejects_malformed_api_base() {
    let bus = MessageBus::new(32);
    let mut config = base_feishu_config();
    config.api_base = "not a url".to_string();
    let channel = FeishuChannel::new(config, bus);

    let error = channel.start().await.expect_err("bad api base");
    assert!(error.to_string().contains("api"));
}

#[tokio::test]
async fn feishu_channel_start_rejects_malformed_ws_base() {
    let bus = MessageBus::new(32);
    let mut config = base_feishu_config();
    config.ws_base = "still not a url".to_string();
    let channel = FeishuChannel::new(config, bus);

    let error = channel.start().await.expect_err("bad ws base");
    assert!(error.to_string().contains("ws"));
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
    let config = sidekick::config::Config::default();
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

#[tokio::test]
async fn outbound_delivery_to_different_keys_is_parallel() {
    let (manager, bus, state) =
        start_mock_manager(Some("chat-a"), 4, Duration::from_millis(100)).await;
    manager.start_all().await;

    bus.publish_outbound(OutboundMessage {
        channel: "mock".to_string(),
        chat_id: "chat-a".to_string(),
        content: "first".to_string(),
        metadata: HashMap::new(),
    })
    .await
    .expect("publish first");
    bus.publish_outbound(OutboundMessage {
        channel: "mock".to_string(),
        chat_id: "chat-b".to_string(),
        content: "second".to_string(),
        metadata: HashMap::new(),
    })
    .await
    .expect("publish second");

    tokio::time::sleep(Duration::from_millis(100)).await;
    let events = state.events().await;
    assert!(events.iter().any(|event| event == "done:chat-b:second"));
    assert!(!events.iter().any(|event| event == "done:chat-a:first"));

    state.release_blocked();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let events = state.events().await;
    assert!(events.iter().any(|event| event == "done:chat-a:first"));

    manager.stop_all().await;
}

#[tokio::test]
async fn outbound_delivery_preserves_fifo_for_one_key() {
    let (manager, bus, state) =
        start_mock_manager(Some("chat-a"), 4, Duration::from_millis(100)).await;
    manager.start_all().await;

    bus.publish_outbound(OutboundMessage {
        channel: "mock".to_string(),
        chat_id: "chat-a".to_string(),
        content: "first".to_string(),
        metadata: HashMap::new(),
    })
    .await
    .expect("publish first");
    tokio::time::sleep(Duration::from_millis(50)).await;
    let events = state.events().await;
    assert!(events.iter().any(|event| event == "start:chat-a:first"));
    bus.publish_outbound(OutboundMessage {
        channel: "mock".to_string(),
        chat_id: "chat-a".to_string(),
        content: "second".to_string(),
        metadata: HashMap::new(),
    })
    .await
    .expect("publish second");

    state.release_blocked();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let events = state.events().await;
    assert_eq!(
        events,
        vec![
            "start:chat-a:first".to_string(),
            "done:chat-a:first".to_string(),
            "start:chat-a:second".to_string(),
            "done:chat-a:second".to_string(),
        ]
    );

    manager.stop_all().await;
}

#[tokio::test]
async fn outbound_delivery_buffers_same_key_messages_without_blocking_other_keys() {
    let (manager, bus, state) =
        start_mock_manager(Some("chat-a"), 2, Duration::from_millis(100)).await;
    manager.start_all().await;

    bus.publish_outbound(OutboundMessage {
        channel: "mock".to_string(),
        chat_id: "chat-a".to_string(),
        content: "first".to_string(),
        metadata: HashMap::new(),
    })
    .await
    .expect("publish first");
    bus.publish_outbound(OutboundMessage {
        channel: "mock".to_string(),
        chat_id: "chat-a".to_string(),
        content: "second".to_string(),
        metadata: HashMap::new(),
    })
    .await
    .expect("publish second");
    bus.publish_outbound(OutboundMessage {
        channel: "mock".to_string(),
        chat_id: "chat-a".to_string(),
        content: "third".to_string(),
        metadata: HashMap::new(),
    })
    .await
    .expect("publish third");
    bus.publish_outbound(OutboundMessage {
        channel: "mock".to_string(),
        chat_id: "chat-b".to_string(),
        content: "other".to_string(),
        metadata: HashMap::new(),
    })
    .await
    .expect("publish other");

    tokio::time::sleep(Duration::from_millis(100)).await;
    let events = state.events().await;
    assert!(!events.iter().any(|event| event == "start:chat-a:third"));
    assert!(events.iter().any(|event| event == "done:chat-b:other"));

    state.release_blocked();
    tokio::time::sleep(Duration::from_millis(250)).await;
    let events = state.events().await;
    assert!(events.iter().any(|event| event == "done:chat-a:second"));
    assert!(events.iter().any(|event| event == "done:chat-a:third"));

    manager.stop_all().await;
}

#[tokio::test]
async fn stop_all_aborts_active_delivery_workers() {
    let (manager, bus, state) =
        start_mock_manager(Some("chat-a"), 4, Duration::from_millis(100)).await;
    manager.start_all().await;

    bus.publish_outbound(OutboundMessage {
        channel: "mock".to_string(),
        chat_id: "chat-a".to_string(),
        content: "blocked".to_string(),
        metadata: HashMap::new(),
    })
    .await
    .expect("publish");

    tokio::time::sleep(Duration::from_millis(50)).await;
    manager.stop_all().await;
    state.release_blocked();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let events = state.events().await;
    assert!(!events.iter().any(|event| event == "done:chat-a:blocked"));
    assert_eq!(manager.delivery_worker_count().await, 0);
}

#[tokio::test]
async fn weixin_channel_sends_outbound_text() {
    let state = TelegramState::default();
    let addr = start_weixin_server(state.clone()).await;
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    store
        .save_account(&WeixinAccountState {
            bot_token: "bot-token".to_string(),
            ilink_bot_id: "bot@im.bot".to_string(),
            baseurl: format!("http://{addr}"),
            ilink_user_id: Some("operator@im.wechat".to_string()),
            get_updates_buf: String::new(),
            longpolling_timeout_ms: 35_000,
            status: "confirmed".to_string(),
            updated_at: Utc::now(),
        })
        .unwrap();
    store.save_context_token("user@im.wechat", "ctx-1").unwrap();
    let channel = WeixinChannel::new(
        WeixinConfig {
            enabled: true,
            api_base: format!("http://{addr}"),
            cdn_base: "https://cdn.example.com".to_string(),
        },
        store,
        MessageBus::new(32),
    );

    channel
        .send(OutboundMessage {
            channel: "weixin".to_string(),
            chat_id: "user@im.wechat".to_string(),
            content: "hello".to_string(),
            metadata: HashMap::new(),
        })
        .await
        .expect("send");

    let sent = state.sent.lock().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0]
            .pointer("/msg/context_token")
            .and_then(Value::as_str),
        Some("ctx-1")
    );
    assert_eq!(
        sent[0]
            .pointer("/msg/item_list/0/text_item/text")
            .and_then(Value::as_str),
        Some("hello")
    );
}
