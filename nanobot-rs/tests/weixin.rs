use std::collections::{BTreeSet, HashMap, VecDeque};
use std::fs;
use std::io;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router, response::IntoResponse};
use chrono::{TimeZone, Utc};
use nanobot_rs::bus::{MessageBus, OutboundMessage};
use nanobot_rs::channels::Channel;
use nanobot_rs::channels::weixin::{
    WeixinAccountState, WeixinAccountStore, WeixinChannel, WeixinLoginManager,
};
use nanobot_rs::config::WeixinConfig;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn sample_account() -> WeixinAccountState {
    WeixinAccountState {
        bot_token: "bot-token".to_string(),
        ilink_bot_id: "ilink-bot-id".to_string(),
        baseurl: "https://weixin.example.com".to_string(),
        ilink_user_id: Some("user@im.wechat".to_string()),
        get_updates_buf: "get-updates-buffer".to_string(),
        longpolling_timeout_ms: 35000,
        status: "active".to_string(),
        updated_at: Utc.with_ymd_and_hms(2026, 3, 22, 10, 11, 12).unwrap(),
    }
}

#[derive(Clone, Default)]
struct SharedWriter {
    buffer: Arc<StdMutex<Vec<u8>>>,
}

struct SharedWriterHandle {
    buffer: Arc<StdMutex<Vec<u8>>>,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SharedWriter {
    type Writer = SharedWriterHandle;

    fn make_writer(&'a self) -> Self::Writer {
        SharedWriterHandle {
            buffer: self.buffer.clone(),
        }
    }
}

impl Write for SharedWriterHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.lock().expect("buffer").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Default)]
struct WeixinTestState {
    responses: Arc<Mutex<VecDeque<Value>>>,
    requests: Arc<Mutex<Vec<WeixinRequestRecord>>>,
    attempts: Arc<AtomicUsize>,
}

impl WeixinTestState {
    fn with_responses(responses: Vec<Value>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into())),
            requests: Arc::new(Mutex::new(Vec::new())),
            attempts: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WeixinRequestRecord {
    path: &'static str,
    query: HashMap<String, String>,
    x_wechat_uin: String,
    authorization_type: String,
    authorization: String,
    body: Value,
    observed_at: Instant,
}

async fn pop_weixin_response(state: &WeixinTestState) -> Json<Value> {
    let response = state.responses.lock().await.pop_front().unwrap_or_else(|| {
        json!({
            "errcode": 0,
            "data": {
                "message_type": 1,
                "from_user_id": "alice@im.wechat",
                "context_token": "ctx-1",
                "item_list": [],
                "get_updates_buf": "",
                "longpolling_timeout_ms": 35000,
            }
        })
    });
    Json(response)
}

async fn pop_weixin_response_strict(state: &WeixinTestState) -> Json<Value> {
    let response = state
        .responses
        .lock()
        .await
        .pop_front()
        .expect("unexpected extra poll");
    Json(response)
}

async fn record_weixin_request(
    state: &WeixinTestState,
    path: &'static str,
    query: HashMap<String, String>,
    headers: HeaderMap,
    body: Value,
) {
    state.requests.lock().await.push(WeixinRequestRecord {
        path,
        query,
        x_wechat_uin: headers
            .get("X-WECHAT-UIN")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string(),
        authorization_type: headers
            .get("AuthorizationType")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string(),
        authorization: headers
            .get("Authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string(),
        body,
        observed_at: Instant::now(),
    });
}

async fn weixin_qr_response(
    State(state): State<WeixinTestState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Json<Value> {
    record_weixin_request(
        &state,
        "/ilink/bot/get_bot_qrcode",
        query,
        headers,
        json!({}),
    )
    .await;
    pop_weixin_response(&state).await
}

async fn weixin_qr_status_response(
    State(state): State<WeixinTestState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Json<Value> {
    record_weixin_request(
        &state,
        "/ilink/bot/get_qrcode_status",
        query,
        headers,
        json!({}),
    )
    .await;
    pop_weixin_response(&state).await
}

async fn weixin_getupdates_response(
    State(state): State<WeixinTestState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    record_weixin_request(&state, "/ilink/bot/getupdates", query, headers, body).await;
    pop_weixin_response(&state).await
}

async fn weixin_sendmessage_response(
    State(state): State<WeixinTestState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    record_weixin_request(&state, "/ilink/bot/sendmessage", query, headers, body).await;
    pop_weixin_response(&state).await
}

async fn weixin_getupdates_flaky_response(
    State(state): State<WeixinTestState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> axum::response::Response {
    record_weixin_request(&state, "/ilink/bot/getupdates", query, headers, body).await;
    let attempt = state.attempts.fetch_add(1, Ordering::SeqCst);
    if attempt == 0 {
        (axum::http::StatusCode::OK, "not json").into_response()
    } else {
        pop_weixin_response(&state).await.into_response()
    }
}

async fn weixin_getupdates_slow_response(
    State(state): State<WeixinTestState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    record_weixin_request(&state, "/ilink/bot/getupdates", query, headers, body).await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    pop_weixin_response(&state).await
}

async fn spawn_weixin_test_server(responses: Vec<Value>) -> WeixinTestServer {
    let state = WeixinTestState::with_responses(responses);
    let requests = state.requests.clone();
    let app = Router::new()
        .route("/ilink/bot/get_bot_qrcode", get(weixin_qr_response))
        .route(
            "/ilink/bot/get_qrcode_status",
            get(weixin_qr_status_response),
        )
        .route("/ilink/bot/getupdates", post(weixin_getupdates_response))
        .route("/ilink/bot/sendmessage", post(weixin_sendmessage_response))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    WeixinTestServer {
        api_base: format!("http://{addr}"),
        requests,
    }
}

async fn spawn_weixin_strict_test_server(responses: Vec<Value>) -> WeixinTestServer {
    let state = WeixinTestState::with_responses(responses);
    let requests = state.requests.clone();
    let app = Router::new()
        .route(
            "/ilink/bot/getupdates",
            post(weixin_getupdates_strict_response),
        )
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    WeixinTestServer {
        api_base: format!("http://{addr}"),
        requests,
    }
}

async fn spawn_weixin_flaky_test_server(responses: Vec<Value>) -> WeixinTestServer {
    let state = WeixinTestState::with_responses(responses);
    let requests = state.requests.clone();
    let app = Router::new()
        .route(
            "/ilink/bot/getupdates",
            post(weixin_getupdates_flaky_response),
        )
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    WeixinTestServer {
        api_base: format!("http://{addr}"),
        requests,
    }
}

struct WeixinTestServer {
    api_base: String,
    requests: Arc<Mutex<Vec<WeixinRequestRecord>>>,
}

impl WeixinTestServer {
    fn api_base(&self) -> &str {
        &self.api_base
    }

    async fn take_requests(&self) -> Vec<WeixinRequestRecord> {
        std::mem::take(&mut *self.requests.lock().await)
    }
}

fn qr_response(qrcode: &str, qrcode_img_content: &str) -> Value {
    json!({
        "qrcode": qrcode,
        "qrcode_img_content": qrcode_img_content,
    })
}

fn qr_status_wait() -> Value {
    json!({
        "status": "wait",
    })
}

fn qr_status_scanned() -> Value {
    json!({
        "status": "scaned",
    })
}

fn qr_status_expired() -> Value {
    json!({
        "status": "expired",
    })
}

fn qr_status_confirmed(
    bot_token: &str,
    ilink_bot_id: &str,
    baseurl: &str,
    ilink_user_id: &str,
    get_updates_buf: &str,
) -> Value {
    json!({
        "status": "confirmed",
        "bot_token": bot_token,
        "ilink_bot_id": ilink_bot_id,
        "baseurl": baseurl,
        "ilink_user_id": ilink_user_id,
        "get_updates_buf": get_updates_buf,
    })
}

fn text_item(content: &str) -> Value {
    json!({
        "item_type": 1,
        "text": {
            "content": content,
        }
    })
}

fn image_item() -> Value {
    json!({
        "item_type": 2,
        "image": {
            "media_id": "image-1",
        }
    })
}

fn protocol_text_item(content: &str) -> Value {
    json!({
        "type": 1,
        "text_item": {
            "text": content,
        }
    })
}

fn message_envelope(
    message_type: i64,
    from_user_id: &str,
    group_id: Option<&str>,
    context_token: &str,
    item_list: Vec<Value>,
    get_updates_buf: &str,
    longpolling_timeout_ms: u64,
) -> Value {
    let mut envelope = json!({
        "message_type": message_type,
        "from_user_id": from_user_id,
        "context_token": context_token,
        "item_list": item_list,
        "get_updates_buf": get_updates_buf,
        "longpolling_timeout_ms": longpolling_timeout_ms,
    });
    if let Some(group_id) = group_id {
        envelope["group_id"] = json!(group_id);
    }
    envelope
}

fn direct_text_message(
    from_user_id: &str,
    context_token: &str,
    get_updates_buf: &str,
    longpolling_timeout_ms: u64,
    item_list: Vec<Value>,
) -> Value {
    json!({
        "errcode": 0,
        "data": message_envelope(
            1,
            from_user_id,
            None,
            context_token,
            item_list,
            get_updates_buf,
            longpolling_timeout_ms,
        )
    })
}

fn protocol_direct_text_poll_response(
    from_user_id: &str,
    context_token: &str,
    get_updates_buf: &str,
    longpolling_timeout_ms: u64,
    text: &str,
) -> Value {
    json!({
        "ret": 0,
        "msgs": [
            {
                "seq": 429,
                "message_id": 9812451782375u64,
                "from_user_id": from_user_id,
                "to_user_id": "ilink-bot-id",
                "create_time_ms": 1774158905123u64,
                "update_time_ms": 1774158905123u64,
                "session_id": format!("{from_user_id}#ilink-bot-id"),
                "message_type": 1,
                "message_state": 2,
                "context_token": context_token,
                "item_list": [protocol_text_item(text)],
            }
        ],
        "get_updates_buf": get_updates_buf,
        "longpolling_timeout_ms": longpolling_timeout_ms
    })
}

fn group_text_message(
    from_user_id: &str,
    group_id: &str,
    context_token: &str,
    get_updates_buf: &str,
    longpolling_timeout_ms: u64,
) -> Value {
    json!({
        "errcode": 0,
        "data": message_envelope(
            1,
            from_user_id,
            Some(group_id),
            context_token,
            vec![text_item("hello")],
            get_updates_buf,
            longpolling_timeout_ms,
        )
    })
}

fn non_text_message(
    from_user_id: &str,
    get_updates_buf: &str,
    longpolling_timeout_ms: u64,
) -> Value {
    json!({
        "errcode": 0,
        "data": message_envelope(
            2,
            from_user_id,
            None,
            "ctx-image",
            vec![image_item()],
            get_updates_buf,
            longpolling_timeout_ms,
        )
    })
}

fn expired_message() -> Value {
    json!({
        "errcode": -14,
        "errmsg": "account expired",
    })
}

fn expired_message_with_ret_only() -> Value {
    json!({
        "ret": -14,
        "errmsg": "account expired",
    })
}

fn unexpected_errcode_message() -> Value {
    json!({
        "errcode": 7,
        "errmsg": "temporary upstream failure",
    })
}

async fn weixin_getupdates_strict_response(
    State(state): State<WeixinTestState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    record_weixin_request(&state, "/ilink/bot/getupdates", query, headers, body).await;
    pop_weixin_response_strict(&state).await
}

#[test]
fn weixin_account_store_round_trips_account_and_context_tokens() {
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();

    store.save_account(&sample_account()).unwrap();
    store.save_context_token("user@im.wechat", "ctx-1").unwrap();

    let account = store.load_account().unwrap().unwrap();
    let token = store.load_context_token("user@im.wechat").unwrap();

    assert_eq!(account.bot_token, "bot-token");
    assert_eq!(account.ilink_bot_id, "ilink-bot-id");
    assert_eq!(account.baseurl, "https://weixin.example.com");
    assert_eq!(account.ilink_user_id.as_deref(), Some("user@im.wechat"));
    assert_eq!(account.get_updates_buf, "get-updates-buffer");
    assert_eq!(account.status, "active");
    assert_eq!(account.updated_at, sample_account().updated_at);
    assert_eq!(token.as_deref(), Some("ctx-1"));
}

#[tokio::test]
async fn start_login_parses_qr_payload() {
    let server =
        spawn_weixin_test_server(vec![qr_response("qr-token", "data:image/png;base64,abc")]).await;
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let manager = WeixinLoginManager::new(server.api_base(), store, "1.0.2");

    let login = manager.start_login().await.unwrap();
    let requests = server.take_requests().await;

    assert_eq!(login.qrcode, "qr-token");
    assert_eq!(login.qrcode_img_content, "data:image/png;base64,abc");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/ilink/bot/get_bot_qrcode");
    assert_eq!(
        requests[0].query.get("bot_type").map(String::as_str),
        Some("3")
    );
    assert!(!requests[0].x_wechat_uin.is_empty());
}

#[tokio::test]
async fn poll_login_status_handles_wait() {
    let server = spawn_weixin_test_server(vec![
        qr_response("qr-token", "data:image/png;base64,abc"),
        qr_status_wait(),
    ])
    .await;
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let manager = WeixinLoginManager::new(server.api_base(), store, "1.0.2");

    manager.start_login().await.unwrap();
    let status = manager.poll_login_status().await.unwrap();
    let requests = server.take_requests().await;

    assert_eq!(status.status, "wait");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].path, "/ilink/bot/get_qrcode_status");
    assert_eq!(
        requests[1].query.get("qrcode").map(String::as_str),
        Some("qr-token")
    );
    assert!(!requests[1].x_wechat_uin.is_empty());
}

#[tokio::test]
async fn poll_login_status_handles_scaned() {
    let server = spawn_weixin_test_server(vec![
        qr_response("qr-token", "data:image/png;base64,abc"),
        qr_status_scanned(),
    ])
    .await;
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let manager = WeixinLoginManager::new(server.api_base(), store, "1.0.2");

    manager.start_login().await.unwrap();
    let status = manager.poll_login_status().await.unwrap();
    let requests = server.take_requests().await;

    assert_eq!(status.status, "scaned");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].path, "/ilink/bot/get_qrcode_status");
    assert_eq!(
        requests[1].query.get("qrcode").map(String::as_str),
        Some("qr-token")
    );
    assert!(!requests[1].x_wechat_uin.is_empty());
}

#[tokio::test]
async fn poll_login_status_handles_expired() {
    let server = spawn_weixin_test_server(vec![
        qr_response("qr-token", "data:image/png;base64,abc"),
        qr_status_expired(),
    ])
    .await;
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let manager = WeixinLoginManager::new(server.api_base(), store, "1.0.2");

    manager.start_login().await.unwrap();
    let status = manager.poll_login_status().await.unwrap();
    let requests = server.take_requests().await;

    assert_eq!(status.status, "expired");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].path, "/ilink/bot/get_qrcode_status");
    assert_eq!(
        requests[1].query.get("qrcode").map(String::as_str),
        Some("qr-token")
    );
    assert!(!requests[1].x_wechat_uin.is_empty());
}

#[tokio::test]
async fn confirmed_login_persists_account_state() {
    let temp = tempdir().unwrap();
    let server = spawn_weixin_test_server(vec![
        qr_response("qr-token", "data:image/png;base64,abc"),
        qr_status_confirmed(
            "bot-token",
            "bot@im.bot",
            "https://alt.example",
            "user@im.wechat",
            "server-cursor",
        ),
    ])
    .await;
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let manager = WeixinLoginManager::new(server.api_base(), store.clone(), "1.0.2");

    manager.start_login().await.unwrap();
    let status = manager.poll_login_status().await.unwrap();
    let requests = server.take_requests().await;

    assert_eq!(status.status, "confirmed");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].path, "/ilink/bot/get_bot_qrcode");
    assert_eq!(
        requests[0].query.get("bot_type").map(String::as_str),
        Some("3")
    );
    assert!(!requests[0].x_wechat_uin.is_empty());
    assert_eq!(requests[1].path, "/ilink/bot/get_qrcode_status");
    assert_eq!(
        requests[1].query.get("qrcode").map(String::as_str),
        Some("qr-token")
    );
    assert!(!requests[1].x_wechat_uin.is_empty());
    let account = store.load_account().unwrap().unwrap();
    assert_eq!(account.bot_token, "bot-token");
    assert_eq!(account.ilink_bot_id, "bot@im.bot");
    assert_eq!(account.baseurl, "https://alt.example");
    assert_eq!(account.ilink_user_id.as_deref(), Some("user@im.wechat"));
    assert_eq!(account.get_updates_buf, "");
    assert_eq!(account.status, "confirmed");
}

#[tokio::test]
async fn confirmed_login_falls_back_to_configured_api_base_without_baseurl() {
    let temp = tempdir().unwrap();
    let server = spawn_weixin_test_server(vec![
        qr_response("qr-token", "data:image/png;base64,abc"),
        json!({
            "status": "confirmed",
            "bot_token": "bot-token",
            "ilink_bot_id": "bot@im.bot",
            "ilink_user_id": "user@im.wechat",
            "get_updates_buf": "server-cursor",
        }),
    ])
    .await;
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let manager = WeixinLoginManager::new(server.api_base(), store.clone(), "1.0.2");

    manager.start_login().await.unwrap();
    let status = manager.poll_login_status().await.unwrap();

    assert_eq!(status.status, "confirmed");
    let account = store.load_account().unwrap().unwrap();
    assert_eq!(account.baseurl, server.api_base());
    assert_eq!(account.get_updates_buf, "");
}

async fn start_weixin_channel(
    server: &WeixinTestServer,
    account: Option<WeixinAccountState>,
) -> (
    tempfile::TempDir,
    MessageBus,
    WeixinAccountStore,
    tokio::task::JoinHandle<()>,
) {
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    if let Some(mut account) = account {
        account.baseurl = server.api_base().to_string();
        store.save_account(&account).unwrap();
    }
    let bus = MessageBus::new(32);
    let channel = WeixinChannel::new(
        WeixinConfig {
            enabled: true,
            api_base: server.api_base().to_string(),
            cdn_base: "https://cdn.example.com".to_string(),
        },
        store.clone(),
        bus.clone(),
    );
    let handle = tokio::spawn(async move {
        channel.start().await.expect("weixin start");
    });
    (temp, bus, store, handle)
}

fn sample_outbound(channel: &str, chat_id: &str, content: &str) -> OutboundMessage {
    OutboundMessage {
        channel: channel.to_string(),
        chat_id: chat_id.to_string(),
        content: content.to_string(),
        metadata: HashMap::new(),
    }
}

#[tokio::test]
async fn outbound_send_includes_cached_context_token() {
    let server = spawn_weixin_test_server(vec![]).await;
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let mut account = sample_account();
    account.baseurl = server.api_base().to_string();
    store.save_account(&account).unwrap();
    store.save_context_token("user@im.wechat", "ctx-1").unwrap();
    let channel = WeixinChannel::new(
        WeixinConfig {
            enabled: true,
            api_base: server.api_base().to_string(),
            cdn_base: "https://cdn.example.com".to_string(),
        },
        store,
        MessageBus::new(32),
    );

    channel
        .send(sample_outbound(
            "weixin",
            "user@im.wechat",
            "**bold** `code` [link](https://example.com)\n# heading\n- item",
        ))
        .await
        .unwrap();

    let requests = server.take_requests().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/ilink/bot/sendmessage");
    assert_eq!(
        requests[0]
            .body
            .pointer("/msg/to_user_id")
            .and_then(Value::as_str),
        Some("user@im.wechat")
    );
    assert_eq!(
        requests[0]
            .body
            .pointer("/msg/context_token")
            .and_then(Value::as_str),
        Some("ctx-1")
    );
    assert_eq!(
        requests[0]
            .body
            .pointer("/msg/item_list/0/text_item/text")
            .and_then(Value::as_str),
        Some("bold code link (https://example.com) heading item")
    );
    assert_eq!(
        requests[0]
            .body
            .pointer("/base_info/channel_version")
            .and_then(Value::as_str),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert!(
        requests[0]
            .body
            .pointer("/msg/client_id")
            .and_then(Value::as_str)
            .is_some_and(|client_id| !client_id.is_empty())
    );
}

#[tokio::test]
async fn outbound_send_includes_required_auth_headers() {
    let server = spawn_weixin_test_server(vec![]).await;
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let mut account = sample_account();
    account.baseurl = server.api_base().to_string();
    store.save_account(&account).unwrap();
    store.save_context_token("user@im.wechat", "ctx-1").unwrap();
    let channel = WeixinChannel::new(
        WeixinConfig {
            enabled: true,
            api_base: server.api_base().to_string(),
            cdn_base: "https://cdn.example.com".to_string(),
        },
        store,
        MessageBus::new(32),
    );

    channel
        .send(sample_outbound("weixin", "user@im.wechat", "hello"))
        .await
        .unwrap();

    let requests = server.take_requests().await;
    let request = requests
        .iter()
        .find(|request| request.path == "/ilink/bot/sendmessage")
        .expect("sendmessage request");
    assert_eq!(request.authorization_type, "ilink_bot_token");
    assert_eq!(request.authorization, "Bearer bot-token");
    assert!(!request.x_wechat_uin.is_empty());
}

#[tokio::test]
async fn outbound_send_preserves_plain_text_literals() {
    let server = spawn_weixin_test_server(vec![]).await;
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let mut account = sample_account();
    account.baseurl = server.api_base().to_string();
    store.save_account(&account).unwrap();
    store.save_context_token("user@im.wechat", "ctx-1").unwrap();
    let channel = WeixinChannel::new(
        WeixinConfig {
            enabled: true,
            api_base: server.api_base().to_string(),
            cdn_base: "https://cdn.example.com".to_string(),
        },
        store,
        MessageBus::new(32),
    );

    let cases = [
        ("#hashtag", "#hashtag"),
        ("foo_bar_baz", "foo_bar_baz"),
        ("center_1_report", "center_1_report"),
        (
            "path/to_file_v1.txt and ./nested_dir/report_2.md",
            "path/to_file_v1.txt and ./nested_dir/report_2.md",
        ),
        ("123 apples", "123 apples"),
        ("2026.03 release", "2026.03 release"),
    ];

    for (input, expected) in cases {
        channel
            .send(sample_outbound("weixin", "user@im.wechat", input))
            .await
            .unwrap();

        let requests = server.take_requests().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0]
                .body
                .pointer("/msg/item_list/0/text_item/text")
                .and_then(Value::as_str),
            Some(expected)
        );
    }
}

#[tokio::test]
async fn outbound_send_skips_runtime_progress_messages() {
    let server = spawn_weixin_test_server(vec![]).await;
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let mut account = sample_account();
    account.baseurl = server.api_base().to_string();
    store.save_account(&account).unwrap();
    store.save_context_token("user@im.wechat", "ctx-1").unwrap();
    let channel = WeixinChannel::new(
        WeixinConfig {
            enabled: true,
            api_base: server.api_base().to_string(),
            cdn_base: "https://cdn.example.com".to_string(),
        },
        store,
        MessageBus::new(32),
    );

    let mut msg = sample_outbound("weixin", "user@im.wechat", "progress");
    msg.metadata.insert("_progress".to_string(), json!(true));
    channel.send(msg).await.unwrap();

    let requests = server.take_requests().await;
    assert!(requests.is_empty());
}

#[tokio::test]
async fn outbound_send_requires_cached_context_token() {
    let server = spawn_weixin_test_server(vec![]).await;
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let mut account = sample_account();
    account.baseurl = server.api_base().to_string();
    store.save_account(&account).unwrap();
    let channel = WeixinChannel::new(
        WeixinConfig {
            enabled: true,
            api_base: server.api_base().to_string(),
            cdn_base: "https://cdn.example.com".to_string(),
        },
        store,
        MessageBus::new(32),
    );

    let err = channel
        .send(sample_outbound("weixin", "user@im.wechat", "hello"))
        .await
        .unwrap_err();

    assert!(err.to_string().contains("context_token"));
}

#[tokio::test]
async fn outbound_send_fails_on_business_error_response() {
    let server = spawn_weixin_test_server(vec![json!({
        "ret": -2,
        "errmsg": "bad request",
    })])
    .await;
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let mut account = sample_account();
    account.baseurl = server.api_base().to_string();
    store.save_account(&account).unwrap();
    store.save_context_token("user@im.wechat", "ctx-1").unwrap();
    let channel = WeixinChannel::new(
        WeixinConfig {
            enabled: true,
            api_base: server.api_base().to_string(),
            cdn_base: "https://cdn.example.com".to_string(),
        },
        store,
        MessageBus::new(32),
    );

    let err = channel
        .send(sample_outbound("weixin", "user@im.wechat", "hello"))
        .await
        .unwrap_err();

    assert!(err.to_string().contains("ret=-2"));
    assert!(err.to_string().contains("bad request"));
}

#[tokio::test]
async fn poll_loop_routes_direct_text_messages() {
    let server = spawn_weixin_test_server(vec![direct_text_message(
        "alice@im.wechat",
        "ctx-1",
        "cursor-2",
        35000,
        vec![image_item(), text_item("hello"), text_item("ignored")],
    )])
    .await;
    let account = sample_account();
    let (_temp, bus, store, handle) = start_weixin_channel(&server, Some(account)).await;

    let inbound = tokio::time::timeout(std::time::Duration::from_secs(2), bus.consume_inbound())
        .await
        .expect("timely inbound")
        .expect("message");
    handle.abort();

    assert_eq!(inbound.channel, "weixin");
    assert_eq!(inbound.sender_id, "alice@im.wechat");
    assert_eq!(inbound.chat_id, "alice@im.wechat");
    assert_eq!(inbound.content, "hello");
    assert_eq!(
        store
            .load_context_token("alice@im.wechat")
            .unwrap()
            .as_deref(),
        Some("ctx-1")
    );
}

#[tokio::test]
async fn poll_loop_parses_protocol_msgs_shape_and_text_items() {
    let server = spawn_weixin_test_server(vec![protocol_direct_text_poll_response(
        "alice@im.wechat",
        "ctx-1",
        "cursor-2",
        35000,
        "hello from protocol",
    )])
    .await;
    let account = sample_account();
    let (_temp, bus, store, handle) = start_weixin_channel(&server, Some(account)).await;

    let inbound = tokio::time::timeout(std::time::Duration::from_secs(2), bus.consume_inbound())
        .await
        .expect("timely inbound")
        .expect("message");
    handle.abort();

    assert_eq!(inbound.channel, "weixin");
    assert_eq!(inbound.sender_id, "alice@im.wechat");
    assert_eq!(inbound.chat_id, "alice@im.wechat");
    assert_eq!(inbound.content, "hello from protocol");
    assert_eq!(
        store
            .load_context_token("alice@im.wechat")
            .unwrap()
            .as_deref(),
        Some("ctx-1")
    );
    let account = store.load_account().unwrap().unwrap();
    assert_eq!(account.get_updates_buf, "cursor-2");
}

#[tokio::test]
async fn poll_loop_ignores_group_messages() {
    let server = spawn_weixin_test_server(vec![group_text_message(
        "alice@im.wechat",
        "group@chat.wechat",
        "ctx-group",
        "cursor-2",
        35000,
    )])
    .await;
    let account = sample_account();
    let (_temp, bus, _, handle) = start_weixin_channel(&server, Some(account)).await;

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    handle.abort();

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), bus.consume_inbound())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn poll_loop_ignores_non_text_items() {
    let server =
        spawn_weixin_test_server(vec![non_text_message("alice@im.wechat", "cursor-2", 35000)])
            .await;
    let account = sample_account();
    let (_temp, bus, _, handle) = start_weixin_channel(&server, Some(account)).await;

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    handle.abort();

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), bus.consume_inbound())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn poll_loop_persists_get_updates_buf_after_poll() {
    let server = spawn_weixin_test_server(vec![direct_text_message(
        "alice@im.wechat",
        "ctx-1",
        "cursor-2",
        35000,
        vec![text_item("hello")],
    )])
    .await;
    let account = sample_account();
    let (_temp, bus, store, handle) = start_weixin_channel(&server, Some(account)).await;

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), bus.consume_inbound())
        .await
        .expect("timely inbound");
    handle.abort();

    let account = store.load_account().unwrap().unwrap();
    assert_eq!(account.get_updates_buf, "cursor-2");
}

#[tokio::test]
async fn poll_loop_marks_account_expired_on_errcode_minus_14() {
    let server = spawn_weixin_strict_test_server(vec![expired_message()]).await;
    let account = sample_account();
    let (_temp, bus, store, handle) = start_weixin_channel(&server, Some(account)).await;

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    handle.abort();
    drop(bus);

    let account = store.load_account().unwrap().unwrap();
    assert_eq!(account.status, "expired");
}

#[tokio::test]
async fn poll_loop_marks_account_expired_on_ret_minus_14() {
    let server = spawn_weixin_strict_test_server(vec![expired_message_with_ret_only()]).await;
    let account = sample_account();
    let (_temp, bus, store, handle) = start_weixin_channel(&server, Some(account)).await;

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    handle.abort();
    drop(bus);

    let account = store.load_account().unwrap().unwrap();
    assert_eq!(account.status, "expired");
}

#[tokio::test]
async fn poll_loop_rechecks_store_when_account_missing() {
    let server = spawn_weixin_test_server(vec![direct_text_message(
        "alice@im.wechat",
        "ctx-1",
        "cursor-2",
        35000,
        vec![text_item("hello")],
    )])
    .await;
    let (_temp, bus, store, handle) = start_weixin_channel(&server, None).await;

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let mut account = sample_account();
    account.baseurl = server.api_base().to_string();
    store.save_account(&account).unwrap();

    let inbound = tokio::time::timeout(std::time::Duration::from_secs(2), bus.consume_inbound())
        .await
        .expect("timely inbound")
        .expect("message");
    handle.abort();

    assert_eq!(inbound.channel, "weixin");
    assert_eq!(inbound.content, "hello");
}

#[tokio::test]
async fn weixin_logs_waiting_for_login_and_shutdown() {
    let server = spawn_weixin_test_server(vec![]).await;
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let bus = MessageBus::new(32);
    let channel = Arc::new(WeixinChannel::new(
        WeixinConfig {
            enabled: true,
            api_base: server.api_base().to_string(),
            cdn_base: "https://cdn.example.com".to_string(),
        },
        store,
        bus,
    ));

    let writer = SharedWriter::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .without_time()
        .with_writer(writer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let start_task = tokio::spawn({
        let channel = channel.clone();
        async move { channel.start().await.expect("weixin start") }
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    channel.stop().await.expect("stop");
    tokio::time::timeout(std::time::Duration::from_secs(1), start_task)
        .await
        .expect("channel stopped in time")
        .expect("join");

    let logs = String::from_utf8(writer.buffer.lock().expect("buffer").clone()).expect("utf8");
    assert!(logs.contains("weixin waiting for login"), "{logs}");
    assert!(logs.contains("weixin channel stopped"), "{logs}");
}

#[tokio::test]
async fn weixin_logs_polling_message_and_reply_lifecycle() {
    let server = spawn_weixin_test_server(vec![direct_text_message(
        "alice@im.wechat",
        "ctx-1",
        "cursor-2",
        35000,
        vec![text_item("hello from logs")],
    )])
    .await;
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let mut account = sample_account();
    account.baseurl = server.api_base().to_string();
    store.save_account(&account).unwrap();
    let bus = MessageBus::new(32);
    let channel = Arc::new(WeixinChannel::new(
        WeixinConfig {
            enabled: true,
            api_base: server.api_base().to_string(),
            cdn_base: "https://cdn.example.com".to_string(),
        },
        store,
        bus.clone(),
    ));

    let writer = SharedWriter::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .without_time()
        .with_writer(writer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let start_task = tokio::spawn({
        let channel = channel.clone();
        async move { channel.start().await.expect("weixin start") }
    });

    let inbound = tokio::time::timeout(std::time::Duration::from_secs(2), bus.consume_inbound())
        .await
        .expect("timely inbound")
        .expect("message");
    assert_eq!(inbound.chat_id, "alice@im.wechat");

    channel
        .send(sample_outbound(
            "weixin",
            "alice@im.wechat",
            "reply from logs",
        ))
        .await
        .expect("send reply");

    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let requests = server.take_requests().await;
            if requests
                .iter()
                .any(|request| request.path == "/ilink/bot/sendmessage")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("reply observed");

    channel.stop().await.expect("stop");
    tokio::time::timeout(std::time::Duration::from_secs(1), start_task)
        .await
        .expect("channel stopped in time")
        .expect("join");

    let logs = String::from_utf8(writer.buffer.lock().expect("buffer").clone()).expect("utf8");
    assert!(
        logs.contains("weixin polling started bot=ilink-bot-id"),
        "{logs}"
    );
    assert!(
        logs.contains("weixin text callback sender=alice@im.wechat chat=alice@im.wechat"),
        "{logs}"
    );
    assert!(
        logs.contains("weixin reply sent chat=alice@im.wechat"),
        "{logs}"
    );
    assert!(logs.contains("weixin channel stopped"), "{logs}");
}

#[tokio::test]
async fn poll_loop_recovers_after_request_error() {
    let server = spawn_weixin_flaky_test_server(vec![direct_text_message(
        "alice@im.wechat",
        "ctx-1",
        "cursor-2",
        35000,
        vec![text_item("hello")],
    )])
    .await;
    let account = sample_account();
    let (_temp, bus, _, handle) = start_weixin_channel(&server, Some(account)).await;

    let inbound = tokio::time::timeout(std::time::Duration::from_secs(2), bus.consume_inbound())
        .await
        .expect("timely inbound")
        .expect("message");
    handle.abort();

    assert_eq!(inbound.channel, "weixin");
    assert_eq!(inbound.content, "hello");
    let requests = server.take_requests().await;
    assert!(requests.len() >= 2);
}

#[tokio::test]
async fn poll_loop_retries_on_unexpected_errcode() {
    let server = spawn_weixin_test_server(vec![
        unexpected_errcode_message(),
        direct_text_message(
            "alice@im.wechat",
            "ctx-1",
            "cursor-2",
            35000,
            vec![text_item("hello")],
        ),
    ])
    .await;
    let account = sample_account();
    let (_temp, bus, _, handle) = start_weixin_channel(&server, Some(account)).await;

    let inbound = tokio::time::timeout(std::time::Duration::from_secs(2), bus.consume_inbound())
        .await
        .expect("timely inbound")
        .expect("message");
    handle.abort();

    assert_eq!(inbound.channel, "weixin");
    assert_eq!(inbound.content, "hello");
    let requests = server.take_requests().await;
    let poll_requests: Vec<_> = requests
        .iter()
        .filter(|request| request.path == "/ilink/bot/getupdates")
        .collect();
    assert!(poll_requests.len() >= 2);
    let gap = poll_requests[1]
        .observed_at
        .duration_since(poll_requests[0].observed_at);
    assert!(
        gap >= std::time::Duration::from_millis(150),
        "expected retry gap after errcode failure, got {:?}",
        gap
    );
}

#[tokio::test]
async fn getupdates_request_includes_required_auth_headers_and_channel_version() {
    let server = spawn_weixin_test_server(vec![json!({
        "errcode": 0,
        "data": {
            "message_type": 1,
            "from_user_id": "alice@im.wechat",
            "context_token": "ctx-1",
            "item_list": [text_item("hello")],
            "get_updates_buf": "cursor-2",
            "longpolling_timeout_ms": 35000,
        }
    })])
    .await;
    let account = sample_account();
    let (_temp, bus, _, handle) = start_weixin_channel(&server, Some(account)).await;

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    handle.abort();
    drop(bus);

    let requests = server.take_requests().await;
    let request = requests
        .iter()
        .find(|request| request.path == "/ilink/bot/getupdates")
        .expect("getupdates request");
    assert_eq!(request.authorization_type, "ilink_bot_token");
    assert_eq!(request.authorization, "Bearer bot-token");
    assert!(!request.x_wechat_uin.is_empty());
    assert_eq!(
        request
            .body
            .pointer("/base_info/channel_version")
            .and_then(Value::as_str),
        Some(env!("CARGO_PKG_VERSION"))
    );
}

#[tokio::test]
async fn client_side_timeout_is_treated_as_an_empty_poll() {
    let state = WeixinTestState::with_responses(vec![
        json!({
            "errcode": 0,
            "data": {
                "message_type": 1,
                "from_user_id": "alice@im.wechat",
                "context_token": "ctx-1",
                "item_list": [],
                "get_updates_buf": "cursor-2",
                "longpolling_timeout_ms": 25,
            }
        }),
        json!({
            "errcode": 0,
            "data": {
                "message_type": 1,
                "from_user_id": "alice@im.wechat",
                "context_token": "ctx-1",
                "item_list": [],
                "get_updates_buf": "cursor-3",
                "longpolling_timeout_ms": 25,
            }
        }),
    ]);
    let requests = state.requests.clone();
    let app = Router::new()
        .route(
            "/ilink/bot/getupdates",
            post(weixin_getupdates_slow_response),
        )
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    let server = WeixinTestServer {
        api_base: format!("http://{addr}"),
        requests,
    };
    let mut account = sample_account();
    account.longpolling_timeout_ms = 25;
    let (_temp, bus, _, handle) = start_weixin_channel(&server, Some(account)).await;

    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    handle.abort();
    drop(bus);

    let requests = server.take_requests().await;
    assert!(
        requests
            .iter()
            .any(|request| request.path == "/ilink/bot/getupdates")
    );
    assert!(requests.len() >= 2);
}

#[tokio::test]
async fn poll_timeout_follows_longpolling_timeout_ms() {
    let mut account = sample_account();
    account.longpolling_timeout_ms = 1234;
    let server = spawn_weixin_test_server(vec![json!({
        "errcode": 0,
        "data": {
            "message_type": 1,
            "from_user_id": "alice@im.wechat",
            "context_token": "ctx-1",
            "item_list": [],
            "get_updates_buf": "cursor-2",
            "longpolling_timeout_ms": 1234,
        }
    })])
    .await;
    let (_temp, bus, _, handle) = start_weixin_channel(&server, Some(account)).await;

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    handle.abort();
    drop(bus);

    let requests = server.take_requests().await;
    let request = requests
        .iter()
        .find(|request| request.path == "/ilink/bot/getupdates")
        .expect("getupdates request");
    assert_eq!(
        request
            .body
            .pointer("/longpolling_timeout_ms")
            .and_then(Value::as_u64),
        Some(1234)
    );
}

#[test]
fn weixin_account_store_preserves_multiple_context_tokens_on_disk() {
    let temp = tempdir().unwrap();
    let store_a = WeixinAccountStore::new(temp.path()).unwrap();
    let store_b = WeixinAccountStore::new(temp.path()).unwrap();

    store_a
        .save_context_token("user@im.wechat", "ctx-1")
        .unwrap();
    store_b
        .save_context_token("friend@im.wechat", "ctx-2")
        .unwrap();

    let raw = fs::read_to_string(
        temp.path()
            .join("channels")
            .join("weixin")
            .join("context_tokens.json"),
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();

    assert_eq!(json["user@im.wechat"], "ctx-1");
    assert_eq!(json["friend@im.wechat"], "ctx-2");
    assert_eq!(
        json.as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["friend@im.wechat".to_string(), "user@im.wechat".to_string(),])
    );

    assert_eq!(
        store_a
            .load_context_token("user@im.wechat")
            .unwrap()
            .as_deref(),
        Some("ctx-1")
    );
    assert_eq!(
        store_b
            .load_context_token("friend@im.wechat")
            .unwrap()
            .as_deref(),
        Some("ctx-2")
    );
}

#[test]
fn weixin_account_store_clear_all_removes_persisted_state() {
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();

    store.save_account(&sample_account()).unwrap();
    store.save_context_token("user@im.wechat", "ctx-1").unwrap();
    store.clear_all().unwrap();

    assert!(store.load_account().unwrap().is_none());
    assert!(
        store
            .load_context_token("user@im.wechat")
            .unwrap()
            .is_none()
    );
    assert!(
        !temp
            .path()
            .join("channels")
            .join("weixin")
            .join("context_tokens.json")
            .exists()
    );
}

#[test]
fn weixin_account_store_writes_expected_account_json_shape() {
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let account = sample_account();

    store.save_account(&account).unwrap();

    let raw = fs::read_to_string(
        temp.path()
            .join("channels")
            .join("weixin")
            .join("account.json"),
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();

    assert_eq!(json["bot_token"], "bot-token");
    assert_eq!(json["ilink_bot_id"], "ilink-bot-id");
    assert_eq!(json["baseurl"], "https://weixin.example.com");
    assert_eq!(json["ilink_user_id"], "user@im.wechat");
    assert_eq!(json["get_updates_buf"], "get-updates-buffer");
    assert_eq!(json["longpolling_timeout_ms"], 35000);
    assert_eq!(json["status"], "active");
    assert_eq!(json["updated_at"], "2026-03-22T10:11:12Z");
    assert_eq!(
        json.as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "bot_token".to_string(),
            "ilink_bot_id".to_string(),
            "baseurl".to_string(),
            "ilink_user_id".to_string(),
            "get_updates_buf".to_string(),
            "longpolling_timeout_ms".to_string(),
            "status".to_string(),
            "updated_at".to_string(),
        ])
    );
}

#[cfg(unix)]
#[test]
fn weixin_account_store_writes_owner_only_files_on_unix() {
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();

    store.save_account(&sample_account()).unwrap();
    store.save_context_token("user@im.wechat", "ctx-1").unwrap();

    let account_mode = fs::metadata(
        temp.path()
            .join("channels")
            .join("weixin")
            .join("account.json"),
    )
    .unwrap()
    .permissions()
    .mode()
        & 0o777;
    let context_mode = fs::metadata(
        temp.path()
            .join("channels")
            .join("weixin")
            .join("context_tokens.json"),
    )
    .unwrap()
    .permissions()
    .mode()
        & 0o777;

    assert_eq!(account_mode, 0o600);
    assert_eq!(context_mode, 0o600);
}
