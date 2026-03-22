use std::collections::BTreeSet;
use std::fs;

use chrono::{TimeZone, Utc};
use nanobot_rs::channels::weixin::{WeixinAccountState, WeixinAccountStore};
use tempfile::tempdir;

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
}

#[test]
fn weixin_account_store_writes_expected_account_json_shape() {
    let temp = tempdir().unwrap();
    let store = WeixinAccountStore::new(temp.path()).unwrap();
    let account = sample_account();

    store.save_account(&account).unwrap();

    let raw = fs::read_to_string(store.account_path()).unwrap();
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
