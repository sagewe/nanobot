use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use nanobot_rs::bus::{MessageBus, OutboundMessage};
use nanobot_rs::channels::{Channel, FeishuChannel};
use nanobot_rs::config::FeishuConfig;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq)]
struct RecordedCreateMessage {
    receive_id_type: String,
    payload: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RecordedReplyMessage {
    message_id: String,
    payload: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RecordedReaction {
    message_id: String,
    payload: Value,
}

#[derive(Default)]
struct FeishuFixtureState {
    token_calls: AtomicUsize,
    bot_info_calls: AtomicUsize,
    create_messages: Mutex<Vec<RecordedCreateMessage>>,
    reply_messages: Mutex<Vec<RecordedReplyMessage>>,
    reactions: Mutex<Vec<RecordedReaction>>,
    fail_reply_once: AtomicBool,
}

#[derive(Clone)]
struct FeishuFixture {
    addr: SocketAddr,
    state: Arc<FeishuFixtureState>,
}

#[derive(Deserialize)]
struct ReceiveIdTypeQuery {
    receive_id_type: Option<String>,
}

async fn feishu_get_token(State(state): State<Arc<FeishuFixtureState>>) -> Json<Value> {
    state.token_calls.fetch_add(1, Ordering::SeqCst);
    Json(json!({
        "code": 0,
        "msg": "success",
        "tenant_access_token": "tenant-token",
        "expire": 7200
    }))
}

async fn feishu_get_bot_info(State(state): State<Arc<FeishuFixtureState>>) -> Json<Value> {
    state.bot_info_calls.fetch_add(1, Ordering::SeqCst);
    Json(json!({
        "code": 0,
        "msg": "success",
        "bot": {
            "open_id": "ou_bot_1"
        }
    }))
}

async fn feishu_create_message(
    State(state): State<Arc<FeishuFixtureState>>,
    Query(query): Query<ReceiveIdTypeQuery>,
    Json(payload): Json<Value>,
) -> Json<Value> {
    state.create_messages.lock().await.push(RecordedCreateMessage {
        receive_id_type: query.receive_id_type.unwrap_or_default(),
        payload,
    });
    Json(json!({"code": 0, "msg": "success"}))
}

async fn feishu_reply_message(
    State(state): State<Arc<FeishuFixtureState>>,
    Path(message_id): Path<String>,
    Json(payload): Json<Value>,
) -> Json<Value> {
    state.reply_messages.lock().await.push(RecordedReplyMessage {
        message_id,
        payload,
    });
    if state.fail_reply_once.swap(false, Ordering::SeqCst) {
        return Json(json!({"code": 1001, "msg": "reply failed"}));
    }
    Json(json!({"code": 0, "msg": "success"}))
}

async fn feishu_add_reaction(
    State(state): State<Arc<FeishuFixtureState>>,
    Path(message_id): Path<String>,
    Json(payload): Json<Value>,
) -> Json<Value> {
    state.reactions.lock().await.push(RecordedReaction { message_id, payload });
    Json(json!({"code": 0, "msg": "success"}))
}

impl FeishuFixture {
    async fn start() -> Self {
        let state = Arc::new(FeishuFixtureState::default());
        let app = Router::new()
            .route(
                "/open-apis/auth/v3/tenant_access_token/internal",
                post(feishu_get_token),
            )
            .route("/open-apis/bot/v3/info", get(feishu_get_bot_info))
            .route("/open-apis/im/v1/messages", post(feishu_create_message))
            .route(
                "/open-apis/im/v1/messages/{message_id}/reply",
                post(feishu_reply_message),
            )
            .route(
                "/open-apis/im/v1/messages/{message_id}/reactions",
                post(feishu_add_reaction),
            )
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        Self { addr, state }
    }

    fn channel(&self, bus: MessageBus, reply_to_message: bool) -> FeishuChannel {
        FeishuChannel::new(
            FeishuConfig {
                enabled: true,
                app_id: "cli_a1".to_string(),
                app_secret: "secret".to_string(),
                api_base: format!("http://{}/open-apis", self.addr),
                ws_base: "ws://127.0.0.1:9".to_string(),
                encrypt_key: String::new(),
                verification_token: String::new(),
                allow_from: vec!["*".to_string()],
                react_emoji: "THUMBSUP".to_string(),
                group_policy: "mention".to_string(),
                reply_to_message,
            },
            bus,
        )
    }

    async fn create_messages(&self) -> Vec<RecordedCreateMessage> {
        self.state.create_messages.lock().await.clone()
    }

    async fn reply_messages(&self) -> Vec<RecordedReplyMessage> {
        self.state.reply_messages.lock().await.clone()
    }

    fn fail_next_reply(&self) {
        self.state.fail_reply_once.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn feishu_channel_uses_open_id_for_direct_targets() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus, false);

    channel
        .send(OutboundMessage {
            channel: "feishu".to_string(),
            chat_id: "ou_user_123".to_string(),
            content: "hello".to_string(),
            metadata: HashMap::new(),
        })
        .await
        .expect("send");

    let sent = fixture.create_messages().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].receive_id_type, "open_id");
    assert_eq!(sent[0].payload["msg_type"], "text");
}

#[tokio::test]
async fn feishu_channel_uses_chat_id_for_group_targets() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus, false);

    channel
        .send(OutboundMessage {
            channel: "feishu".to_string(),
            chat_id: "oc_group_123".to_string(),
            content: "hello".to_string(),
            metadata: HashMap::new(),
        })
        .await
        .expect("send");

    let sent = fixture.create_messages().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].receive_id_type, "chat_id");
}

#[tokio::test]
async fn feishu_channel_sends_plain_long_content_as_post() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus, false);

    channel
        .send(OutboundMessage {
            channel: "feishu".to_string(),
            chat_id: "ou_user_123".to_string(),
            content: "a".repeat(300),
            metadata: HashMap::new(),
        })
        .await
        .expect("send");

    let sent = fixture.create_messages().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].payload["msg_type"], "post");
}

#[tokio::test]
async fn feishu_channel_sends_markdown_links_as_post() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus, false);

    channel
        .send(OutboundMessage {
            channel: "feishu".to_string(),
            chat_id: "ou_user_123".to_string(),
            content: "[docs](https://example.com)".to_string(),
            metadata: HashMap::new(),
        })
        .await
        .expect("send");

    let sent = fixture.create_messages().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].payload["msg_type"], "post");
}

#[tokio::test]
async fn feishu_channel_sends_code_blocks_as_interactive() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus, false);

    channel
        .send(OutboundMessage {
            channel: "feishu".to_string(),
            chat_id: "ou_user_123".to_string(),
            content: "```rust\nfn main() {}\n```".to_string(),
            metadata: HashMap::new(),
        })
        .await
        .expect("send");

    let sent = fixture.create_messages().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].payload["msg_type"], "interactive");
}

#[tokio::test]
async fn feishu_channel_replies_only_on_the_first_payload_per_send() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus, true);

    channel
        .send(OutboundMessage {
            channel: "feishu".to_string(),
            chat_id: "ou_user_123".to_string(),
            content: "| a | b |\n|---|---|\n| 1 | 2 |\n\n| c | d |\n|---|---|\n| 3 | 4 |".to_string(),
            metadata: HashMap::from([("message_id".to_string(), json!("msg-1"))]),
        })
        .await
        .expect("send");

    let replies = fixture.reply_messages().await;
    let creates = fixture.create_messages().await;
    assert_eq!(replies.len(), 1);
    assert_eq!(creates.len(), 1);
}

#[tokio::test]
async fn feishu_channel_skips_reply_api_for_progress_messages() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus, true);

    channel
        .send(OutboundMessage {
            channel: "feishu".to_string(),
            chat_id: "ou_user_123".to_string(),
            content: "hello".to_string(),
            metadata: HashMap::from([
                ("message_id".to_string(), json!("msg-1")),
                ("_progress".to_string(), json!(true)),
            ]),
        })
        .await
        .expect("send");

    assert!(fixture.reply_messages().await.is_empty());
    assert_eq!(fixture.create_messages().await.len(), 1);
}

#[tokio::test]
async fn feishu_channel_skips_reply_api_for_tool_hint_messages() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus, true);

    channel
        .send(OutboundMessage {
            channel: "feishu".to_string(),
            chat_id: "ou_user_123".to_string(),
            content: "hello".to_string(),
            metadata: HashMap::from([
                ("message_id".to_string(), json!("msg-1")),
                ("_tool_hint".to_string(), json!(true)),
            ]),
        })
        .await
        .expect("send");

    assert!(fixture.reply_messages().await.is_empty());
    assert_eq!(fixture.create_messages().await.len(), 1);
}

#[tokio::test]
async fn feishu_channel_falls_back_to_create_when_reply_fails() {
    let fixture = FeishuFixture::start().await;
    fixture.fail_next_reply();
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus, true);

    channel
        .send(OutboundMessage {
            channel: "feishu".to_string(),
            chat_id: "ou_user_123".to_string(),
            content: "hello".to_string(),
            metadata: HashMap::from([("message_id".to_string(), json!("msg-1"))]),
        })
        .await
        .expect("send");

    assert_eq!(fixture.reply_messages().await.len(), 1);
    assert_eq!(fixture.create_messages().await.len(), 1);
}
