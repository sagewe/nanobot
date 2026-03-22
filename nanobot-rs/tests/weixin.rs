use std::collections::{BTreeSet, HashMap, VecDeque};
use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{TimeZone, Utc};
use nanobot_rs::bus::MessageBus;
use nanobot_rs::channels::Channel;
use nanobot_rs::channels::weixin::{
    WeixinAccountState, WeixinAccountStore, WeixinChannel, WeixinLoginManager,
};
use nanobot_rs::config::WeixinConfig;
use serde_json::{Value, json};
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
struct WeixinTestState {
    responses: Arc<Mutex<VecDeque<Value>>>,
    requests: Arc<Mutex<Vec<WeixinRequestRecord>>>,
}

impl WeixinTestState {
    fn with_responses(responses: Vec<Value>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into())),
            requests: Arc::new(Mutex::new(Vec::new())),
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
}

async fn pop_weixin_response(state: &WeixinTestState) -> Json<Value> {
    let response = state.responses.lock().await.pop_front().unwrap_or_else(|| {
        json!({
            "errcode": 0,
            "data": {
                "items": []
            }
        })
    });
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

fn poll_response(items: Vec<Value>, get_updates_buf: &str, timeout_ms: u64) -> Value {
    json!({
        "errcode": 0,
        "data": {
            "items": items,
            "get_updates_buf": get_updates_buf,
            "longpolling_timeout_ms": timeout_ms,
        }
    })
}

fn poll_expired_response() -> Value {
    json!({
        "errcode": -14,
        "errmsg": "account expired",
    })
}

fn direct_text_item(from_user_id: &str, text: &str, context_token: &str) -> Value {
    json!({
        "msg_type": "text",
        "from_user_id": from_user_id,
        "chat_id": from_user_id,
        "text": text,
        "context_token": context_token,
    })
}

fn group_text_item(from_user_id: &str, group_id: &str, text: &str) -> Value {
    json!({
        "msg_type": "text",
        "from_user_id": from_user_id,
        "group_id": group_id,
        "chat_id": group_id,
        "text": text,
        "context_token": "ctx-group",
    })
}

fn non_text_item(from_user_id: &str) -> Value {
    json!({
        "msg_type": "image",
        "from_user_id": from_user_id,
        "chat_id": from_user_id,
        "media_id": "image-1",
        "context_token": "ctx-image",
    })
}

async fn start_weixin_channel(
    server: &WeixinTestServer,
    account: WeixinAccountState,
) -> (
    tempfile::TempDir,
    MessageBus,
    WeixinAccountStore,
    tokio::task::JoinHandle<()>,
) {
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let mut account = account;
    account.baseurl = server.api_base().to_string();
    store.save_account(&account).unwrap();
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

#[tokio::test]
async fn poll_loop_routes_direct_text_messages() {
    let server = spawn_weixin_test_server(vec![poll_response(
        vec![direct_text_item("alice@im.wechat", "hello", "ctx-1")],
        "cursor-2",
        35000,
    )])
    .await;
    let account = sample_account();
    let (_temp, bus, store, handle) = start_weixin_channel(&server, account).await;

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
async fn poll_loop_ignores_group_messages() {
    let server = spawn_weixin_test_server(vec![poll_response(
        vec![group_text_item(
            "alice@im.wechat",
            "group@chat.wechat",
            "hello",
        )],
        "cursor-2",
        35000,
    )])
    .await;
    let account = sample_account();
    let (_temp, bus, _, handle) = start_weixin_channel(&server, account).await;

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
    let server = spawn_weixin_test_server(vec![poll_response(
        vec![non_text_item("alice@im.wechat")],
        "cursor-2",
        35000,
    )])
    .await;
    let account = sample_account();
    let (_temp, bus, _, handle) = start_weixin_channel(&server, account).await;

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
    let server = spawn_weixin_test_server(vec![poll_response(vec![], "cursor-2", 35000)]).await;
    let account = sample_account();
    let (_temp, bus, store, handle) = start_weixin_channel(&server, account).await;

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    handle.abort();
    drop(bus);

    let account = store.load_account().unwrap().unwrap();
    assert_eq!(account.get_updates_buf, "cursor-2");
}

#[tokio::test]
async fn poll_loop_marks_account_expired_on_errcode_minus_14() {
    let server = spawn_weixin_test_server(vec![poll_expired_response()]).await;
    let account = sample_account();
    let (_temp, bus, store, handle) = start_weixin_channel(&server, account).await;

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    handle.abort();
    drop(bus);

    let account = store.load_account().unwrap().unwrap();
    assert_eq!(account.status, "expired");
}

#[tokio::test]
async fn getupdates_request_includes_required_auth_headers_and_channel_version() {
    let server = spawn_weixin_test_server(vec![poll_response(vec![], "cursor-2", 35000)]).await;
    let account = sample_account();
    let (_temp, bus, _, handle) = start_weixin_channel(&server, account).await;

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
        poll_response(vec![], "cursor-2", 25),
        poll_response(vec![], "cursor-3", 25),
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
    let (_temp, bus, _, handle) = start_weixin_channel(&server, account).await;

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
    let server = spawn_weixin_test_server(vec![poll_response(vec![], "cursor-2", 1234)]).await;
    let (_temp, bus, _, handle) = start_weixin_channel(&server, account).await;

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
