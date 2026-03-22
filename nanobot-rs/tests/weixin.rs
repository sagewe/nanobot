use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;

use chrono::{TimeZone, Utc};
use axum::extract::Query;
use axum::routing::get;
use axum::{Json, Router};
use nanobot_rs::channels::weixin::{
    WeixinAccountState, WeixinAccountStore, WeixinLoginManager,
};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use serde_json::{json, Value};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn sample_account() -> WeixinAccountState {
    WeixinAccountState {
        bot_token: "bot-token".to_string(),
        ilink_bot_id: "ilink-bot-id".to_string(),
        baseurl: "https://weixin.example.com".to_string(),
        ilink_user_id: Some("user@im.wechat".to_string()),
        get_updates_buf: "get-updates-buffer".to_string(),
        status: "active".to_string(),
        updated_at: Utc.with_ymd_and_hms(2026, 3, 22, 10, 11, 12).unwrap(),
    }
}

#[derive(Clone, Default)]
struct WeixinTestState {
    responses: Arc<Mutex<VecDeque<Value>>>,
}

impl WeixinTestState {
    fn with_responses(responses: Vec<Value>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into())),
        }
    }
}

async fn weixin_test_response(
    Query(_params): Query<std::collections::HashMap<String, String>>,
    axum::extract::State(state): axum::extract::State<WeixinTestState>,
) -> Json<Value> {
    let response = state
        .responses
        .lock()
        .await
        .pop_front()
        .expect("test server ran out of responses");
    Json(response)
}

async fn spawn_weixin_test_server(responses: Vec<Value>) -> WeixinTestServer {
    let state = WeixinTestState::with_responses(responses);
    let app = Router::new()
        .route("/ilink/bot/get_bot_qrcode", get(weixin_test_response))
        .route("/ilink/bot/get_qrcode_status", get(weixin_test_response))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    WeixinTestServer {
        api_base: format!("http://{addr}"),
    }
}

struct WeixinTestServer {
    api_base: String,
}

impl WeixinTestServer {
    fn api_base(&self) -> &str {
        &self.api_base
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
    let server = spawn_weixin_test_server(vec![qr_response(
        "qr-token",
        "data:image/png;base64,abc",
    )])
    .await;
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let manager = WeixinLoginManager::new(server.api_base(), store, "1.0.2");

    let login = manager.start_login().await.unwrap();

    assert_eq!(login.qrcode, "qr-token");
    assert_eq!(login.qrcode_img_content, "data:image/png;base64,abc");
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

    assert_eq!(status.status, "wait");
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

    assert_eq!(status.status, "scaned");
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

    assert_eq!(status.status, "expired");
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

    assert_eq!(status.status, "confirmed");
    let account = store.load_account().unwrap().unwrap();
    assert_eq!(account.bot_token, "bot-token");
    assert_eq!(account.ilink_bot_id, "bot@im.bot");
    assert_eq!(account.baseurl, "https://alt.example");
    assert_eq!(account.ilink_user_id.as_deref(), Some("user@im.wechat"));
    assert_eq!(account.get_updates_buf, "");
    assert_eq!(account.status, "confirmed");
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
