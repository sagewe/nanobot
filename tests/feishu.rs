use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path as StdPath;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use serde::Deserialize;
use serde_json::{Value, json};
use sidekick::bus::{InboundMessage, MessageBus, OutboundMessage};
use sidekick::channels::{Channel, FeishuChannel};
use sidekick::config::FeishuConfig;
use sidekick::tools::{MessageTool, Tool, ToolContext};
use tempfile::tempdir;
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct RecordedResourceDownload {
    message_id: String,
    resource_key: String,
    resource_type: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RecordedUpload {
    kind: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ProtoHeader {
    #[prost(string, tag = "1")]
    key: String,
    #[prost(string, tag = "2")]
    value: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ProtoFrame {
    #[prost(uint64, tag = "1")]
    seq_id: u64,
    #[prost(uint64, tag = "2")]
    log_id: u64,
    #[prost(int32, tag = "3")]
    service: i32,
    #[prost(int32, tag = "4")]
    method: i32,
    #[prost(message, repeated, tag = "5")]
    headers: Vec<ProtoHeader>,
    #[prost(string, tag = "6")]
    payload_encoding: String,
    #[prost(string, tag = "7")]
    payload_type: String,
    #[prost(bytes = "vec", tag = "8")]
    payload: Vec<u8>,
    #[prost(string, tag = "9")]
    log_id_new: String,
}

#[derive(Default)]
struct FeishuFixtureState {
    token_calls: AtomicUsize,
    bot_info_calls: AtomicUsize,
    ws_config_calls: AtomicUsize,
    create_messages: Mutex<Vec<RecordedCreateMessage>>,
    reply_messages: Mutex<Vec<RecordedReplyMessage>>,
    reactions: Mutex<Vec<RecordedReaction>>,
    resource_downloads: Mutex<Vec<RecordedResourceDownload>>,
    uploads: Mutex<Vec<RecordedUpload>>,
    fail_reply_once: AtomicBool,
    ws_connect_url: String,
}

#[derive(Clone)]
struct FeishuFixture {
    http_addr: SocketAddr,
    state: Arc<FeishuFixtureState>,
    ws_sender: broadcast::Sender<WsAction>,
}

#[derive(Clone, Debug)]
enum WsAction {
    Binary(Vec<u8>),
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

async fn feishu_get_ws_config(State(state): State<Arc<FeishuFixtureState>>) -> Json<Value> {
    state.ws_config_calls.fetch_add(1, Ordering::SeqCst);
    Json(json!({
        "code": 0,
        "msg": "success",
        "data": {
            "URL": state.ws_connect_url,
            "ClientConfig": {
                "PingInterval": 120,
                "ReconnectCount": -1,
                "ReconnectInterval": 1,
                "ReconnectNonce": 0
            }
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

#[derive(Deserialize)]
struct ResourceTypeQuery {
    r#type: Option<String>,
}

async fn feishu_download_message_resource(
    State(state): State<Arc<FeishuFixtureState>>,
    Path((message_id, resource_key)): Path<(String, String)>,
    Query(query): Query<ResourceTypeQuery>,
) -> impl IntoResponse {
    state
        .resource_downloads
        .lock()
        .await
        .push(RecordedResourceDownload {
            message_id,
            resource_key: resource_key.clone(),
            resource_type: query.r#type.unwrap_or_default(),
        });
    let bytes = format!("fixture-resource-{resource_key}").into_bytes();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/octet-stream")],
        bytes,
    )
}

async fn feishu_upload_image(State(state): State<Arc<FeishuFixtureState>>) -> Json<Value> {
    state.uploads.lock().await.push(RecordedUpload {
        kind: "image".to_string(),
    });
    Json(json!({
        "code": 0,
        "msg": "success",
        "data": {"image_key": "img_uploaded_1"}
    }))
}

async fn feishu_upload_file(State(state): State<Arc<FeishuFixtureState>>) -> Json<Value> {
    state.uploads.lock().await.push(RecordedUpload {
        kind: "file".to_string(),
    });
    Json(json!({
        "code": 0,
        "msg": "success",
        "data": {"file_key": "file_uploaded_1"}
    }))
}

impl FeishuFixture {
    async fn start() -> Self {
        let ws_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ws");
        let ws_addr = ws_listener.local_addr().expect("ws addr");
        let state = Arc::new(FeishuFixtureState {
            ws_connect_url: format!("ws://{}/ws/v2?device_id=device-1&service_id=7", ws_addr),
            ..FeishuFixtureState::default()
        });
        let app = Router::new()
            .route(
                "/open-apis/auth/v3/tenant_access_token/internal",
                post(feishu_get_token),
            )
            .route("/callback/ws/endpoint", post(feishu_get_ws_config))
            .route("/open-apis/bot/v3/info", get(feishu_get_bot_info))
            .route("/open-apis/im/v1/messages", post(feishu_create_message))
            .route(
                "/open-apis/im/v1/messages/{message_id}/reply",
                post(feishu_reply_message),
            )
            .route(
                "/open-apis/im/v1/messages/{message_id}/resources/{resource_key}",
                get(feishu_download_message_resource),
            )
            .route(
                "/open-apis/im/v1/messages/{message_id}/reactions",
                post(feishu_add_reaction),
            )
            .route("/open-apis/im/v1/images", post(feishu_upload_image))
            .route("/open-apis/im/v1/files", post(feishu_upload_file))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let http_addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
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
                                    Ok(WsAction::Binary(bytes)) => {
                                        if write.send(Message::Binary(bytes.into())).await.is_err() {
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
            ws_base: format!("http://{}", self.http_addr),
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

    async fn resource_downloads(&self) -> Vec<RecordedResourceDownload> {
        self.state.resource_downloads.lock().await.clone()
    }

    async fn uploads(&self) -> Vec<RecordedUpload> {
        self.state.uploads.lock().await.clone()
    }

    fn fail_next_reply(&self) {
        self.state.fail_reply_once.store(true, Ordering::SeqCst);
    }

    fn send_ws_frame(&self, bytes: Vec<u8>) {
        let _ = self.ws_sender.send(WsAction::Binary(bytes));
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

    fn push_media_event(
        &self,
        message_id: &str,
        sender_open_id: &str,
        chat_id: &str,
        chat_type: &str,
        msg_type: &str,
        content: Value,
        parent_id: Option<&str>,
        root_id: Option<&str>,
    ) {
        self.send_ws_frame(self.message_event_with_reply_context(
            message_id,
            sender_open_id,
            chat_id,
            chat_type,
            msg_type,
            content,
            vec![],
            "user",
            parent_id,
            root_id,
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
        self.send_ws_frame(vec![0xde, 0xad, 0xbe, 0xef]);
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
    ) -> Vec<u8> {
        self.message_event_with_reply_context(
            message_id,
            sender_open_id,
            chat_id,
            chat_type,
            message_type,
            content,
            mentions,
            sender_type,
            None,
            None,
        )
    }

    fn message_event_with_reply_context(
        &self,
        message_id: &str,
        sender_open_id: &str,
        chat_id: &str,
        chat_type: &str,
        message_type: &str,
        content: Value,
        mentions: Vec<Value>,
        sender_type: &str,
        parent_id: Option<&str>,
        root_id: Option<&str>,
    ) -> Vec<u8> {
        let payload = json!({
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
                    "mentions": mentions,
                    "parent_id": parent_id,
                    "root_id": root_id
                }
            }
        })
        .to_string()
        .into_bytes();

        ProtoFrame {
            seq_id: 0,
            log_id: 1,
            service: 7,
            method: 1,
            headers: vec![
                ProtoHeader {
                    key: "type".to_string(),
                    value: "event".to_string(),
                },
                ProtoHeader {
                    key: "message_id".to_string(),
                    value: format!("wire-{message_id}"),
                },
                ProtoHeader {
                    key: "sum".to_string(),
                    value: "1".to_string(),
                },
                ProtoHeader {
                    key: "seq".to_string(),
                    value: "0".to_string(),
                },
                ProtoHeader {
                    key: "trace_id".to_string(),
                    value: format!("trace-{message_id}"),
                },
            ],
            payload_encoding: String::new(),
            payload_type: String::new(),
            payload,
            log_id_new: String::new(),
        }
        .encode_to_vec()
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
            media: Vec::new(),
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
            media: Vec::new(),
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
            media: Vec::new(),
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
            media: Vec::new(),
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
            media: Vec::new(),
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
            media: Vec::new(),
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
            media: Vec::new(),
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
            media: Vec::new(),
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
            media: Vec::new(),
            metadata: HashMap::from([("message_id".to_string(), json!("msg-1"))]),
        })
        .await
        .expect("send");

    assert_eq!(fixture.reply_messages().await.len(), 1);
    assert_eq!(fixture.create_messages().await.len(), 1);
}

#[tokio::test]
async fn feishu_channel_uploads_media_before_sending_text() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus, false);
    let dir = tempdir().expect("tempdir");
    let image_path = dir.path().join("photo.png");
    let file_path = dir.path().join("notes.txt");
    std::fs::write(&image_path, b"fake-image").expect("write image");
    std::fs::write(&file_path, b"fake-file").expect("write file");

    channel
        .send(OutboundMessage {
            channel: "feishu".to_string(),
            chat_id: "ou_user_123".to_string(),
            content: "hello".to_string(),
            media: vec![
                image_path.display().to_string(),
                file_path.display().to_string(),
            ],
            metadata: HashMap::new(),
        })
        .await
        .expect("send");

    let uploads = fixture.uploads().await;
    assert_eq!(
        uploads
            .iter()
            .map(|upload| upload.kind.as_str())
            .collect::<Vec<_>>(),
        vec!["image", "file"]
    );

    let sent = fixture.create_messages().await;
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[0].payload["msg_type"], "image");
    assert_eq!(sent[1].payload["msg_type"], "file");
    assert_eq!(sent[2].payload["msg_type"], "text");

    let image_content: Value =
        serde_json::from_str(sent[0].payload["content"].as_str().expect("image content"))
            .expect("image content json");
    let file_content: Value =
        serde_json::from_str(sent[1].payload["content"].as_str().expect("file content"))
            .expect("file content json");
    let text_content: Value =
        serde_json::from_str(sent[2].payload["content"].as_str().expect("text content"))
            .expect("text content json");
    assert_eq!(image_content["image_key"], "img_uploaded_1");
    assert_eq!(file_content["file_key"], "file_uploaded_1");
    assert_eq!(text_content["text"], "hello");
}

#[tokio::test]
async fn message_tool_media_reaches_feishu_outbound_send() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let tool = MessageTool::new(bus.clone());
    let dir = tempdir().expect("tempdir");
    let image_path = dir.path().join("tool-photo.png");
    std::fs::write(&image_path, b"tool-image").expect("write image");

    tool.set_context(ToolContext {
        channel: "feishu".to_string(),
        chat_id: "ou_user_123".to_string(),
        session_key: "feishu:ou_user_123".to_string(),
        message_id: None,
        metadata: HashMap::new(),
        reply_to_caller: false,
        provider_request: None,
    })
    .await;

    let result = tool
        .execute(json!({
            "content": "tool says hi",
            "media": [image_path.display().to_string()]
        }))
        .await;
    assert!(result.contains("Message sent"));

    let outbound = bus.consume_outbound().await.expect("outbound");
    let channel = fixture.channel(MessageBus::new(4), false);
    channel.send(outbound).await.expect("send");

    let uploads = fixture.uploads().await;
    assert_eq!(uploads.len(), 1);
    assert_eq!(uploads[0].kind, "image");

    let sent = fixture.create_messages().await;
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].payload["msg_type"], "image");
    assert_eq!(sent[1].payload["msg_type"], "text");
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
async fn feishu_channel_downloads_image_audio_file_and_post_media() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus.clone(), false);
    let handle = start_channel(channel).await;

    fixture.push_media_event(
        "msg-image",
        "ou_user_1",
        "oc_any",
        "p2p",
        "image",
        json!({"image_key": "img_key_1"}),
        None,
        None,
    );
    let image_inbound = expect_inbound(&bus).await;
    assert!(!image_inbound.content.is_empty());
    assert_eq!(image_inbound.media.len(), 1);
    assert!(StdPath::new(&image_inbound.media[0]).exists());

    fixture.push_media_event(
        "msg-audio",
        "ou_user_1",
        "oc_any",
        "p2p",
        "audio",
        json!({"file_key": "audio_key_1"}),
        None,
        None,
    );
    let audio_inbound = expect_inbound(&bus).await;
    assert!(!audio_inbound.content.is_empty());
    assert_eq!(audio_inbound.media.len(), 1);
    assert!(StdPath::new(&audio_inbound.media[0]).exists());

    fixture.push_media_event(
        "msg-file",
        "ou_user_1",
        "oc_any",
        "p2p",
        "file",
        json!({"file_key": "file_key_1"}),
        None,
        None,
    );
    let file_inbound = expect_inbound(&bus).await;
    assert!(!file_inbound.content.is_empty());
    assert_eq!(file_inbound.media.len(), 1);
    assert!(StdPath::new(&file_inbound.media[0]).exists());

    fixture.push_media_event(
        "msg-media",
        "ou_user_1",
        "oc_any",
        "p2p",
        "media",
        json!({"file_key": "media_key_1"}),
        None,
        None,
    );
    let media_inbound = expect_inbound(&bus).await;
    assert!(!media_inbound.content.is_empty());
    assert_eq!(media_inbound.media.len(), 1);
    assert!(StdPath::new(&media_inbound.media[0]).exists());

    fixture.push_post_event(
        "msg-post-media",
        "ou_user_1",
        "oc_any",
        "p2p",
        json!({
            "zh_cn": {
                "title": "Gallery",
                "content": [[
                    {"tag": "text", "text": "hello"},
                    {"tag": "img", "image_key": "post_img_key_1"}
                ]]
            }
        }),
    );
    let post_inbound = expect_inbound(&bus).await;
    assert_eq!(post_inbound.content, "Gallery\nhello");
    assert_eq!(post_inbound.media.len(), 1);
    assert!(StdPath::new(&post_inbound.media[0]).exists());

    let downloads = fixture.resource_downloads().await;
    assert_eq!(downloads.len(), 5);
    assert_eq!(
        downloads[0],
        RecordedResourceDownload {
            message_id: "msg-image".to_string(),
            resource_key: "img_key_1".to_string(),
            resource_type: "image".to_string(),
        }
    );
    assert_eq!(
        downloads[1],
        RecordedResourceDownload {
            message_id: "msg-audio".to_string(),
            resource_key: "audio_key_1".to_string(),
            resource_type: "file".to_string(),
        }
    );
    assert_eq!(
        downloads[2],
        RecordedResourceDownload {
            message_id: "msg-file".to_string(),
            resource_key: "file_key_1".to_string(),
            resource_type: "file".to_string(),
        }
    );
    assert_eq!(
        downloads[3],
        RecordedResourceDownload {
            message_id: "msg-media".to_string(),
            resource_key: "media_key_1".to_string(),
            resource_type: "file".to_string(),
        }
    );
    assert_eq!(
        downloads[4],
        RecordedResourceDownload {
            message_id: "msg-post-media".to_string(),
            resource_key: "post_img_key_1".to_string(),
            resource_type: "image".to_string(),
        }
    );

    handle.abort();
}

#[tokio::test]
async fn feishu_channel_keeps_reply_context_for_media_messages() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let channel = fixture.channel(bus.clone(), false);
    let handle = start_channel(channel).await;

    fixture.push_media_event(
        "msg-media-context",
        "ou_user_1",
        "oc_any",
        "p2p",
        "image",
        json!({"image_key": "ctx_img_1"}),
        Some("parent-123"),
        Some("root-456"),
    );
    let inbound = expect_inbound(&bus).await;
    assert_eq!(
        inbound.metadata.get("message_id"),
        Some(&json!("msg-media-context"))
    );
    assert_eq!(
        inbound.metadata.get("parent_id"),
        Some(&json!("parent-123"))
    );
    assert_eq!(inbound.metadata.get("root_id"), Some(&json!("root-456")));
    assert_eq!(inbound.media.len(), 1);
    assert!(StdPath::new(&inbound.media[0]).exists());

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

#[tokio::test]
async fn feishu_channel_supports_legacy_ws_style_base_config() {
    let fixture = FeishuFixture::start().await;
    let bus = MessageBus::new(32);
    let mut config = fixture.base_config();
    config.ws_base = format!("ws://{}/open-apis/ws", fixture.http_addr);
    let channel = fixture.channel_with_config(bus.clone(), config);
    let handle = start_channel(channel).await;

    fixture.push_text_event("msg-legacy", "ou_user_1", "oc_any", "p2p", "hello");
    let inbound = expect_inbound(&bus).await;
    assert_eq!(inbound.content, "hello");
    assert_eq!(fixture.state.ws_config_calls.load(Ordering::SeqCst), 1);

    handle.abort();
}
