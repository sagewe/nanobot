use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use nanobot_rs::bus::{MessageBus, OutboundMessage};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Notify, mpsc};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use nanobot_rs::channels::{
    Channel, ParsedWecomTextCallback, WecomBotChannel, WecomTiming, build_wecom_ping_request,
    build_wecom_stream_reply_request, build_wecom_subscribe_request, parse_wecom_text_callback,
};
use nanobot_rs::config::WecomConfig;

#[test]
fn subscribe_request_contains_bot_credentials() {
    let request = build_wecom_subscribe_request("bot-id", "secret", "req-1");

    assert_eq!(request["cmd"], "aibot_subscribe");
    assert_eq!(request["headers"]["req_id"], "req-1");
    assert_eq!(request["body"]["bot_id"], "bot-id");
    assert_eq!(request["body"]["secret"], "secret");
}

#[test]
fn ping_request_uses_wecom_heartbeat_shape() {
    let request = build_wecom_ping_request("req-2");

    assert_eq!(request["cmd"], "ping");
    assert_eq!(request["headers"]["req_id"], "req-2");
    assert!(request.get("body").is_none());
}

#[test]
fn parse_text_callback_extracts_sender_chat_and_content() {
    let payload = json!({
        "cmd": "aibot_msg_callback",
        "headers": { "req_id": "req-3" },
        "body": {
            "msgid": "msg-1",
            "aibotid": "bot-id",
            "chatid": "chat-1",
            "chattype": "group",
            "from": { "userid": "alice" },
            "msgtype": "text",
            "text": { "content": "@Robot hello" }
        }
    });

    let parsed = parse_wecom_text_callback(&payload).expect("text callback");

    assert_eq!(
        parsed,
        ParsedWecomTextCallback {
            req_id: "req-3".to_string(),
            sender_id: "alice".to_string(),
            chat_id: "chat-1".to_string(),
            content: "@Robot hello".to_string(),
        }
    );
}

#[test]
fn stream_reply_request_carries_req_id_and_text_content() {
    let request = build_wecom_stream_reply_request("req-4", "stream-1", "working on it", true);

    assert_eq!(request["cmd"], "aibot_respond_msg");
    assert_eq!(request["headers"]["req_id"], "req-4");
    assert_eq!(request["body"]["msgtype"], "stream");
    assert_eq!(request["body"]["stream"]["id"], "stream-1");
    assert_eq!(request["body"]["stream"]["content"], "working on it");
    assert_eq!(request["body"]["stream"]["finish"], true);
}

#[tokio::test]
async fn wecom_start_requires_bot_credentials() {
    let channel = WecomBotChannel::new(
        WecomConfig {
            enabled: true,
            bot_id: String::new(),
            secret: String::new(),
            ws_base: "ws://127.0.0.1:9".to_string(),
            allow_from: Vec::new(),
        },
        MessageBus::new(32),
    );

    let error = channel.start().await.expect_err("missing credentials");
    assert!(error.to_string().contains("bot_id"));
}

#[derive(Clone)]
struct MockWecomServer {
    addr: SocketAddr,
    received: Arc<Mutex<Vec<serde_json::Value>>>,
    accepted: Arc<AtomicUsize>,
    callback_tx: mpsc::UnboundedSender<serde_json::Value>,
    close_tx: mpsc::UnboundedSender<()>,
    second_connection: Arc<Notify>,
}

impl MockWecomServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let received = Arc::new(Mutex::new(Vec::new()));
        let accepted = Arc::new(AtomicUsize::new(0));
        let second_connection = Arc::new(Notify::new());
        let (callback_tx, mut callback_rx) = mpsc::unbounded_channel::<serde_json::Value>();
        let (close_tx, mut close_rx) = mpsc::unbounded_channel::<()>();

        let received_task = received.clone();
        let accepted_task = accepted.clone();
        let second_connection_task = second_connection.clone();
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.expect("accept");
                let mut ws = accept_async(stream).await.expect("websocket");
                let connection_number = accepted_task.fetch_add(1, Ordering::SeqCst) + 1;
                if connection_number >= 2 {
                    second_connection_task.notify_waiters();
                }

                let Some(Ok(Message::Text(subscribe_text))) = ws.next().await else {
                    continue;
                };
                let subscribe_payload: serde_json::Value =
                    serde_json::from_str(&subscribe_text).expect("subscribe payload");
                received_task.lock().await.push(subscribe_payload.clone());
                let req_id = subscribe_payload["headers"]["req_id"]
                    .as_str()
                    .expect("req id");
                ws.send(Message::Text(
                    json!({
                        "headers": { "req_id": req_id },
                        "errcode": 0,
                        "errmsg": "ok"
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .expect("subscribe ack");

                loop {
                    tokio::select! {
                        Some(callback) = callback_rx.recv() => {
                            ws.send(Message::Text(callback.to_string().into()))
                                .await
                                .expect("callback");
                        }
                        Some(_) = close_rx.recv() => {
                            let _ = ws.close(None).await;
                            break;
                        }
                        frame = ws.next() => {
                            match frame {
                                Some(Ok(Message::Text(text))) => {
                                    let payload: serde_json::Value =
                                        serde_json::from_str(&text).expect("client payload");
                                    received_task.lock().await.push(payload.clone());
                                    if payload["cmd"] == "ping" {
                                        let req_id = payload["headers"]["req_id"]
                                            .as_str()
                                            .expect("ping req id");
                                        ws.send(Message::Text(
                                            json!({
                                                "cmd": "pong",
                                                "headers": { "req_id": req_id }
                                            })
                                            .to_string()
                                            .into(),
                                        ))
                                        .await
                                        .expect("pong");
                                    }
                                }
                                Some(Ok(Message::Close(_))) | None => break,
                                Some(Ok(_)) => {}
                                Some(Err(_)) => break,
                            }
                        }
                    }
                }
            }
        });

        Self {
            addr,
            received,
            accepted,
            callback_tx,
            close_tx,
            second_connection,
        }
    }

    fn ws_base(&self) -> String {
        format!("ws://{}", self.addr)
    }

    fn send_callback(&self, payload: serde_json::Value) {
        self.callback_tx.send(payload).expect("send callback");
    }

    fn close_connection(&self) {
        self.close_tx.send(()).expect("close connection");
    }
}

fn runtime_config(ws_base: String) -> WecomConfig {
    WecomConfig {
        enabled: true,
        bot_id: "bot-id".to_string(),
        secret: "secret".to_string(),
        ws_base,
        allow_from: Vec::new(),
    }
}

#[tokio::test]
async fn wecom_channel_publishes_text_callback_and_replies() {
    let server = MockWecomServer::start().await;
    let bus = MessageBus::new(32);
    let channel = Arc::new(WecomBotChannel::new_with_timing(
        runtime_config(server.ws_base()),
        bus.clone(),
        WecomTiming::for_tests(),
    ));

    let start_task = tokio::spawn({
        let channel = channel.clone();
        async move { channel.start().await.expect("start") }
    });

    server.send_callback(json!({
        "cmd": "aibot_msg_callback",
        "headers": { "req_id": "reply-1" },
        "body": {
            "msgid": "msg-1",
            "aibotid": "bot-id",
            "chatid": "chat-1",
            "chattype": "group",
            "from": { "userid": "alice" },
            "msgtype": "text",
            "text": { "content": "hello from wecom" }
        }
    }));

    let inbound = tokio::time::timeout(Duration::from_secs(2), bus.consume_inbound())
        .await
        .expect("timely inbound")
        .expect("inbound");
    assert_eq!(inbound.channel, "wecom");
    assert_eq!(inbound.sender_id, "alice");
    assert_eq!(inbound.chat_id, "chat-1");
    assert_eq!(inbound.content, "hello from wecom");

    channel
        .send(OutboundMessage {
            channel: "wecom".to_string(),
            chat_id: "chat-1".to_string(),
            content: "reply body".to_string(),
            metadata: Default::default(),
        })
        .await
        .expect("send reply");

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let received = server.received.lock().await.clone();
            if received
                .iter()
                .any(|payload| payload["cmd"] == "aibot_respond_msg")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("reply observed");

    let received = server.received.lock().await;
    let reply = received
        .iter()
        .find(|payload| payload["cmd"] == "aibot_respond_msg")
        .expect("reply payload");
    assert_eq!(reply["headers"]["req_id"], "reply-1");
    assert_eq!(reply["body"]["stream"]["content"], "reply body");

    channel.stop().await.expect("stop");
    start_task.abort();
}

#[tokio::test]
async fn wecom_channel_respects_allowlist_and_reconnects_after_disconnect() {
    let server = MockWecomServer::start().await;
    let mut config = runtime_config(server.ws_base());
    config.allow_from = vec!["alice".to_string()];
    let bus = MessageBus::new(32);
    let channel = Arc::new(WecomBotChannel::new_with_timing(
        config,
        bus.clone(),
        WecomTiming::for_tests(),
    ));

    let start_task = tokio::spawn({
        let channel = channel.clone();
        async move { channel.start().await.expect("start") }
    });

    server.send_callback(json!({
        "cmd": "aibot_msg_callback",
        "headers": { "req_id": "reply-2" },
        "body": {
            "msgid": "msg-2",
            "aibotid": "bot-id",
            "chatid": "chat-2",
            "chattype": "group",
            "from": { "userid": "bob" },
            "msgtype": "text",
            "text": { "content": "blocked" }
        }
    }));

    assert!(
        tokio::time::timeout(Duration::from_millis(300), bus.consume_inbound())
            .await
            .is_err()
    );

    server.close_connection();
    tokio::time::timeout(Duration::from_secs(2), server.second_connection.notified())
        .await
        .expect("reconnected");
    assert!(server.accepted.load(Ordering::SeqCst) >= 2);

    channel.stop().await.expect("stop");
    start_task.abort();
}
