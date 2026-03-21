use serde_json::json;

use nanobot_rs::channels::{
    ParsedWecomTextCallback, build_wecom_ping_request, build_wecom_stream_reply_request,
    build_wecom_subscribe_request, parse_wecom_text_callback,
};

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
    let request =
        build_wecom_stream_reply_request("req-4", "stream-1", "working on it", true);

    assert_eq!(request["cmd"], "aibot_respond_msg");
    assert_eq!(request["headers"]["req_id"], "req-4");
    assert_eq!(request["body"]["msgtype"], "stream");
    assert_eq!(request["body"]["stream"]["id"], "stream-1");
    assert_eq!(request["body"]["stream"]["content"], "working on it");
    assert_eq!(request["body"]["stream"]["finish"], true);
}
