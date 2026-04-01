use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use nanobot_rs::bus::{InboundMessage, MessageBus, OutboundMessage};
use nanobot_rs::channels::{Channel, FeishuChannel};
use nanobot_rs::config::FeishuConfig;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, broadcast};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

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
    http_addr: SocketAddr,
    ws_addr: SocketAddr,
    state: Arc<FeishuFixtureState>,
    ws_sender: broadcast::Sender<WsAction>,
}

#[derive(Clone, Debug)]
enum WsAction {
    Text(String),
    Close,
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
    state
        .create_messages
        .lock()
        .await
        .push(RecordedCreateMessage {
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
    state
        .reply_messages
        .lock()
        .await
        .push(RecordedReplyMessage {
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
    state.reactions.lock().await.push(RecordedReaction {
        message_id,
        payload,
    });
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
        let http_addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        let ws_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ws");
        let ws_addr = ws_listener.local_addr().expect("ws addr");
        let (ws_sender, _) = broadcast::channel(32);
        let ws_broadcast = ws_sender.clone();
        tokio::spawn(async move {
            loop {
                let (stream, _) = ws_listener.accept().await.expect("accept ws");
                let mut rx = ws_broadcast.subscribe();
                tokio::spawn(async move {
                    let ws_stream = accept_async(stream).await.expect("accept_async");
                    let (mut write, mut read) = ws_stream.split();
                    loop {
                        tokio::select! {
                            maybe_action = rx.recv() => {
                                match maybe_action {
                                    Ok(WsAction::Text(text)) => {
                                        if write.send(Message::Text(text.into())).await.is_err() {
                                            break;
                                        }
                                    }
                                    Ok(WsAction::Close) => {
                                        let _ = write.send(Message::Close(None)).await;
                                        break;
                                    }
                                    Err(_) => break,
                                }
                            }
                            incoming = read.next() => {
                                match incoming {
                                    Some(Ok(Message::Close(_))) | None => break,
                                    Some(Ok(_)) => {}
                                    Some(Err(_)) => break,
                                }
                            }
                        }
                    }
                });
            }
        });
        Self {
            http_addr,
            ws_addr,
            state,
            ws_sender,
        }
    }

    fn channel(&self, bus: MessageBus, reply_to_message: bool) -> FeishuChannel {
        self.channel_with_config(bus, {
            let mut config = self.base_config();
            config.reply_to_message = reply_to_message;
            config
        })
    }

    fn base_config(&self) -> FeishuConfig {
        FeishuConfig {
            enabled: true,
            app_id: "cli_a1".to_string(),
            app_secret: "secret".to_string(),
            api_base: format!("http://{}/open-apis", self.http_addr),
            ws_base: format!("ws://{}/open-apis/ws", self.ws_addr),
            encrypt_key: String::new(),
            verification_token: String::new(),
            allow_from: vec!["*".to_string()],
            react_emoji: "THUMBSUP".to_string(),
            group_policy: "mention".to_string(),
            reply_to_message: false,
        }
    }

    fn channel_with_config(&self, bus: MessageBus, config: FeishuConfig) -> FeishuChannel {
        FeishuChannel::new(config, bus)
    }

    async fn create_messages(&self) -> Vec<RecordedCreateMessage> {
        self.state.create_messages.lock().await.clone()
    }

    async fn reply_messages(&self) -> Vec<RecordedReplyMessage> {
        self.state.reply_messages.lock().await.clone()
    }

    async fn reactions(&self) -> Vec<RecordedReaction> {
        self.state.reactions.lock().await.clone()
    }

    fn fail_next_reply(&self) {
        self.state.fail_reply_once.store(true, Ordering::SeqCst);
    }

    fn send_ws_frame(&self, text: String) {
        let _ = self.ws_sender.send(WsAction::Text(text));
    }

    fn disconnect_ws_clients(&self) {
        let _ = self.ws_sender.send(WsAction::Close);
    }

    fn push_text_event(
        &self,
        message_id: &str,
        sender_open_id: &str,
        chat_id: &str,
        chat_type: &str,
        text: &str,
    ) {
        self.send_ws_frame(self.message_event(
            message_id,
            sender_open_id,
            chat_id,
            chat_type,
            "text",
            json!({ "text": text }),
            vec![],
            "user",
        ));
    }

    fn push_post_event(
        &self,
        message_id: &str,
        sender_open_id: &str,
        chat_id: &str,
        chat_type: &str,
        post: Value,
    ) {
        self.send_ws_frame(self.message_event(
            message_id,
            sender_open_id,
            chat_id,
            chat_type,
            "post",
            post,
            vec![],
            "user",
        ));
    }

    fn push_group_text_event(
        &self,
        message_id: &str,
        sender_open_id: &str,
        chat_id: &str,
        text: &str,
        mentions: Vec<Value>,
    ) {
        self.send_ws_frame(self.message_event(
            message_id,
            sender_open_id,
            chat_id,
            "group",
            "text",
            json!({ "text": text }),
            mentions,
            "user",
        ));
    }

    fn push_placeholder_event(
        &self,
        message_id: &str,
        sender_open_id: &str,
        chat_id: &str,
        chat_type: &str,
        msg_type: &str,
    ) {
        self.send_ws_frame(self.message_event(
            message_id,
            sender_open_id,
            chat_id,
            chat_type,
            msg_type,
            json!({}),
            vec![],
            "user",
        ));
    }

    fn push_unknown_event(
        &self,
        message_id: &str,
        sender_open_id: &str,
        chat_id: &str,
        chat_type: &str,
    ) {
        self.push_placeholder_event(
            message_id,
            sender_open_id,
            chat_id,
            chat_type,
            "unknown_type",
        );
    }

    fn push_bot_text_event(&self, message_id: &str, chat_id: &str, text: &str) {
        self.send_ws_frame(self.message_event(
            message_id,
            "ou_bot_1",
            chat_id,
            "p2p",
            "text",
            json!({ "text": text }),
            vec![],
            "bot",
        ));
    }

    fn push_malformed_frame(&self) {
        self.send_ws_frame("not json".to_string());
    }

    fn message_event(
        &self,
        message_id: &str,
        sender_open_id: &str,
        chat_id: &str,
        chat_type: &str,
        message_type: &str,
        content: Value,
        mentions: Vec<Value>,
        sender_type: &str,
    ) -> String {
        json!({
            "event": {
                "sender": {
                    "sender_type": sender_type,
                    "sender_id": {
                        "open_id": sender_open_id
                    }
                },
                "message": {
                    "message_id": message_id,
                    "chat_id": chat_id,
                    "chat_type": chat_type,
                    "message_type": message_type,
                    "content": content.to_string(),
                    "mentions": mentions
                }
            }
        })
        .to_string()
    }
}

async fn expect_inbound(bus: &MessageBus) -> InboundMessage {
    tokio::time::timeout(Duration::from_secs(2), bus.consume_inbound())
        .await
        .expect("timely inbound")
        .expect("message")
}

async fn start_channel(channel: FeishuChannel) -> tokio::task::JoinHandle<()> {
    let handle = tokio::spawn(async move { channel.start().await.expect("start") });
    tokio::time::sleep(Duration::from_millis(100)).await;
    handle
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
            content: "| a | b |\n|---|---|\n| 1 | 2 |\n\n| c | d |\n|---|---|\n| 3 | 4 |"
                .to_string(),
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
    assert!(fixture.create_messages().await.is_empty());
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
    assert!(fixture.create_messages().await.is_empty());
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

#[tokio::test]
async fn feishu_channel_publishes_direct_text_messages() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus.clone(), false);
    let handle = start_channel(channel).await;

    fixture.push_text_event("msg-1", "ou_user_1", "oc_any", "p2p", "hello");
    let inbound = expect_inbound(&bus).await;
    assert_eq!(inbound.channel, "feishu");
    assert_eq!(inbound.sender_id, "ou_user_1");
    assert_eq!(inbound.chat_id, "ou_user_1");
    assert_eq!(inbound.content, "hello");

    handle.abort();
}

#[tokio::test]
async fn feishu_channel_publishes_direct_post_messages() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus.clone(), false);
    let handle = start_channel(channel).await;

    fixture.push_post_event(
        "msg-2",
        "ou_user_1",
        "oc_any",
        "p2p",
        json!({
            "zh_cn": {
                "title": "Title",
                "content": [[
                    {"tag": "text", "text": "hello"},
                    {"tag": "a", "text": "docs", "href": "https://example.com"}
                ]]
            }
        }),
    );
    let inbound = expect_inbound(&bus).await;
    assert_eq!(inbound.content, "Title\nhello docs");

    handle.abort();
}

#[tokio::test]
async fn feishu_channel_allowlist_blocks_non_matching_senders() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let mut config = fixture.base_config();
    config.allow_from = vec!["ou_allowed".to_string()];
    let channel = fixture.channel_with_config(bus.clone(), config);
    let handle = start_channel(channel).await;

    fixture.push_text_event("msg-3", "ou_blocked", "oc_any", "p2p", "hello");
    let inbound = tokio::time::timeout(Duration::from_millis(300), bus.consume_inbound()).await;
    assert!(inbound.is_err());

    handle.abort();
}

#[tokio::test]
async fn feishu_channel_allowlist_wildcard_accepts_senders() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus.clone(), false);
    let handle = start_channel(channel).await;

    fixture.push_text_event("msg-4", "ou_any", "oc_any", "p2p", "hello");
    let inbound = expect_inbound(&bus).await;
    assert_eq!(inbound.sender_id, "ou_any");

    handle.abort();
}

#[tokio::test]
async fn feishu_channel_empty_allowlist_denies_all() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let mut config = fixture.base_config();
    config.allow_from = vec![];
    let channel = fixture.channel_with_config(bus.clone(), config);
    let handle = start_channel(channel).await;

    fixture.push_text_event("msg-5", "ou_any", "oc_any", "p2p", "hello");
    let inbound = tokio::time::timeout(Duration::from_millis(300), bus.consume_inbound()).await;
    assert!(inbound.is_err());

    handle.abort();
}

#[tokio::test]
async fn feishu_channel_mention_mode_rejects_unmentioned_group_messages() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus.clone(), false);
    let handle = start_channel(channel).await;

    fixture.push_group_text_event("msg-6", "ou_user_1", "oc_group_1", "hello", vec![]);
    let inbound = tokio::time::timeout(Duration::from_millis(300), bus.consume_inbound()).await;
    assert!(inbound.is_err());

    handle.abort();
}

#[tokio::test]
async fn feishu_channel_mention_mode_accepts_at_all() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus.clone(), false);
    let handle = start_channel(channel).await;

    fixture.push_group_text_event("msg-7", "ou_user_1", "oc_group_1", "@_all hello", vec![]);
    let inbound = expect_inbound(&bus).await;
    assert_eq!(inbound.chat_id, "oc_group_1");

    handle.abort();
}

#[tokio::test]
async fn feishu_channel_mention_mode_accepts_bot_mentions() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus.clone(), false);
    let handle = start_channel(channel).await;

    fixture.push_group_text_event(
        "msg-8",
        "ou_user_1",
        "oc_group_1",
        "hello",
        vec![json!({"id": {"open_id": "ou_bot_1"}})],
    );
    let inbound = expect_inbound(&bus).await;
    assert_eq!(inbound.chat_id, "oc_group_1");

    handle.abort();
}

#[tokio::test]
async fn feishu_channel_ignores_bot_originated_messages() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus.clone(), false);
    let handle = start_channel(channel).await;

    fixture.push_bot_text_event("msg-9", "oc_any", "hello");
    let inbound = tokio::time::timeout(Duration::from_millis(300), bus.consume_inbound()).await;
    assert!(inbound.is_err());

    handle.abort();
}

#[tokio::test]
async fn feishu_channel_ignores_duplicate_message_ids() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus.clone(), false);
    let handle = start_channel(channel).await;

    fixture.push_text_event("msg-10", "ou_user_1", "oc_any", "p2p", "first");
    let first = expect_inbound(&bus).await;
    assert_eq!(first.content, "first");

    fixture.push_text_event("msg-10", "ou_user_1", "oc_any", "p2p", "second");
    let duplicate = tokio::time::timeout(Duration::from_millis(300), bus.consume_inbound()).await;
    assert!(duplicate.is_err());

    handle.abort();
}

#[tokio::test]
async fn feishu_channel_publishes_placeholder_message_types() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus.clone(), false);
    let handle = start_channel(channel).await;

    fixture.push_placeholder_event("msg-11", "ou_user_1", "oc_any", "p2p", "image");
    let inbound = expect_inbound(&bus).await;
    assert_eq!(inbound.content, "[image]");

    handle.abort();
}

#[tokio::test]
async fn feishu_channel_ignores_unknown_message_types() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus.clone(), false);
    let handle = start_channel(channel).await;

    fixture.push_unknown_event("msg-12", "ou_user_1", "oc_any", "p2p");
    let inbound = tokio::time::timeout(Duration::from_millis(300), bus.consume_inbound()).await;
    assert!(inbound.is_err());

    handle.abort();
}

#[tokio::test]
async fn feishu_channel_skips_malformed_frames_and_keeps_processing() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus.clone(), false);
    let handle = start_channel(channel).await;

    fixture.push_malformed_frame();
    fixture.push_text_event("msg-13", "ou_user_1", "oc_any", "p2p", "hello");
    let inbound = expect_inbound(&bus).await;
    assert_eq!(inbound.content, "hello");

    handle.abort();
}

#[tokio::test]
async fn feishu_channel_adds_reactions_for_accepted_messages() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus.clone(), false);
    let handle = start_channel(channel).await;

    fixture.push_text_event("msg-14", "ou_user_1", "oc_any", "p2p", "hello");
    let _ = expect_inbound(&bus).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let reactions = fixture.reactions().await;
    assert_eq!(reactions.len(), 1);
    assert_eq!(reactions[0].message_id, "msg-14");

    handle.abort();
}

#[tokio::test]
async fn feishu_channel_reconnects_after_disconnect() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus.clone(), false);
    let handle = start_channel(channel).await;

    fixture.disconnect_ws_clients();
    tokio::time::sleep(Duration::from_millis(200)).await;
    fixture.push_text_event("msg-15", "ou_user_1", "oc_any", "p2p", "hello again");
    let inbound = expect_inbound(&bus).await;
    assert_eq!(inbound.content, "hello again");

    handle.abort();
}
