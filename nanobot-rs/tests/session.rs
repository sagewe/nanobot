use nanobot_rs::session::{Session, SessionMessage, SessionStore};
use serde_json::json;
use tempfile::tempdir;

fn tool_turn(prefix: &str, idx: usize) -> Vec<SessionMessage> {
    vec![
        SessionMessage {
            role: "assistant".to_string(),
            content: serde_json::Value::Null,
            timestamp: None,
            tool_calls: Some(vec![
                json!({"id": format!("{prefix}_{idx}_a"), "type": "function", "function": {"name": "x", "arguments": "{}"}}),
                json!({"id": format!("{prefix}_{idx}_b"), "type": "function", "function": {"name": "y", "arguments": "{}"}}),
            ]),
            tool_call_id: None,
            name: None,
            extra: Default::default(),
        },
        SessionMessage {
            role: "tool".to_string(),
            content: json!("ok"),
            timestamp: None,
            tool_calls: None,
            tool_call_id: Some(format!("{prefix}_{idx}_a")),
            name: Some("x".to_string()),
            extra: Default::default(),
        },
        SessionMessage {
            role: "tool".to_string(),
            content: json!("ok"),
            timestamp: None,
            tool_calls: None,
            tool_call_id: Some(format!("{prefix}_{idx}_b")),
            name: Some("y".to_string()),
            extra: Default::default(),
        },
    ]
}

fn assert_no_orphans(history: &[serde_json::Value]) {
    let declared = history
        .iter()
        .filter(|message| message.get("role").and_then(|role| role.as_str()) == Some("assistant"))
        .flat_map(|message| {
            message
                .get("tool_calls")
                .and_then(|calls| calls.as_array())
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|call| {
            call.get("id")
                .and_then(|id| id.as_str())
                .map(str::to_string)
        })
        .collect::<std::collections::HashSet<_>>();
    for message in history {
        if message.get("role").and_then(|role| role.as_str()) == Some("tool") {
            let id = message
                .get("tool_call_id")
                .and_then(|id| id.as_str())
                .unwrap();
            assert!(declared.contains(id), "orphan tool result: {id}");
        }
    }
}

#[test]
fn get_history_drops_orphan_tool_results() {
    let mut session = Session::new("cli:test");
    session.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("old"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    for index in 0..20 {
        session.messages.extend(tool_turn("old", index));
    }
    session.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("new"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    for index in 0..25 {
        session.messages.extend(tool_turn("cur", index));
    }
    let history = session.get_history(100);
    assert_no_orphans(&history);
}

#[test]
fn history_keeps_legitimate_tool_pairs() {
    let mut session = Session::new("cli:ok");
    session.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("hello"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    for index in 0..5 {
        session.messages.extend(tool_turn("ok", index));
    }
    let history = session.get_history(500);
    assert_eq!(
        history
            .iter()
            .filter(|message| message.get("role").and_then(|role| role.as_str()) == Some("tool"))
            .count(),
        10
    );
    assert_no_orphans(&history);
}

#[test]
fn load_old_session_without_active_profile_uses_supplied_default() {
    let dir = tempdir().expect("tempdir");
    let store = SessionStore::new(dir.path()).expect("session store");
    let path = store.path_for("web:alpha");
    let created_at = "2026-03-22T00:00:00Z";
    let updated_at = "2026-03-22T00:05:00Z";
    std::fs::write(
        &path,
        format!(
            r#"{{"_type":"metadata","key":"web:alpha","created_at":"{created_at}","updated_at":"{updated_at}","last_consolidated":0}}
{{"role":"user","content":"hello","timestamp":"{updated_at}","tool_calls":null,"tool_call_id":null,"name":null}}
"#,
        ),
    )
    .expect("write old session");

    let session = store
        .get_or_create_with_default_profile("web:alpha", "fallback-profile")
        .expect("load session");

    assert_eq!(session.active_profile.as_deref(), Some("fallback-profile"));
    assert_eq!(session.active_profile_or("ignored"), "fallback-profile");
}

#[test]
fn old_message_without_extra_deserializes_cleanly() {
    let message: SessionMessage = serde_json::from_value(json!({
        "role": "user",
        "content": "hello",
        "timestamp": "2026-03-22T00:00:00Z",
        "tool_calls": null,
        "tool_call_id": null,
        "name": null
    }))
    .expect("session message");

    assert!(message.extra.is_empty());
    assert_eq!(message.to_llm_message()["role"], json!("user"));
}

#[test]
fn assistant_and_tool_messages_round_trip_extra_fields() {
    let assistant: SessionMessage = serde_json::from_value(json!({
        "role": "assistant",
        "content": null,
        "timestamp": "2026-03-22T00:00:00Z",
        "tool_calls": [{
            "id": "call_1",
            "type": "function",
            "function": {"name": "search", "arguments": "{}"}
        }],
        "tool_call_id": null,
        "name": null,
        "reasoning_content": "thinking"
    }))
    .expect("assistant message");

    let tool: SessionMessage = serde_json::from_value(json!({
        "role": "tool",
        "content": "result",
        "timestamp": "2026-03-22T00:00:01Z",
        "tool_calls": null,
        "tool_call_id": "call_1",
        "name": "search",
        "web_search": true
    }))
    .expect("tool message");

    assert_eq!(
        assistant.extra.get("reasoning_content"),
        Some(&json!("thinking"))
    );
    assert_eq!(tool.extra.get("web_search"), Some(&json!(true)));

    let assistant_out = assistant.to_llm_message();
    let tool_out = tool.to_llm_message();
    assert_eq!(assistant_out["reasoning_content"], json!("thinking"));
    assert_eq!(tool_out["web_search"], json!(true));
}

#[test]
fn to_llm_message_merges_extra_back_into_payload() {
    let mut extra = serde_json::Map::new();
    extra.insert("reasoning_content".to_string(), json!("internal"));
    extra.insert("cached".to_string(), json!(true));
    let message = SessionMessage {
        role: "assistant".to_string(),
        content: json!(null),
        timestamp: None,
        tool_calls: Some(vec![json!({
            "id": "call_1",
            "type": "function",
            "function": {"name": "search", "arguments": "{}"}
        })]),
        tool_call_id: None,
        name: None,
        extra,
    };

    let payload = message.to_llm_message();

    assert_eq!(payload["reasoning_content"], json!("internal"));
    assert_eq!(payload["cached"], json!(true));
    assert_eq!(payload["role"], json!("assistant"));
    assert!(payload.get("tool_calls").is_some());
}

#[test]
fn session_store_helpers_expose_namespaced_sessions() {
    let dir = tempdir().expect("tempdir");
    let store = SessionStore::new(dir.path()).expect("session store");

    let mut web_one = Session::new("web:one");
    web_one.active_profile = Some("web-profile".to_string());
    web_one.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("first"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    web_one.updated_at = chrono::Utc::now();
    store.save(&web_one).expect("save web one");

    let mut web_two = Session::new("web:two");
    web_two.active_profile = Some("second-profile".to_string());
    web_two.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("second"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    web_two.updated_at = chrono::Utc::now() + chrono::Duration::seconds(1);
    store.save(&web_two).expect("save web two");

    let mut cli_session = Session::new("cli:other");
    cli_session.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("ignored"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    store.save(&cli_session).expect("save cli session");

    let sessions = store
        .list_sessions_in_namespace("web")
        .expect("list sessions");
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].key, "web:two");
    assert_eq!(
        sessions[0].active_profile.as_deref(),
        Some("second-profile")
    );
    assert_eq!(sessions[1].key, "web:one");

    let summary = store
        .get_session_summary("web:one")
        .expect("summary lookup")
        .expect("summary");
    assert_eq!(summary.key, "web:one");
    assert_eq!(summary.active_profile.as_deref(), Some("web-profile"));
    assert_eq!(summary.message_count, 1);
    assert_eq!(summary.preview.as_deref(), Some("first"));

    let detail = store
        .get_session_detail("web:two")
        .expect("detail lookup")
        .expect("detail");
    assert_eq!(detail.key, "web:two");
    assert_eq!(detail.active_profile.as_deref(), Some("second-profile"));
    assert_eq!(detail.messages.len(), 1);
}

#[test]
fn session_store_load_preserves_metadata_key_with_additional_colons() {
    let dir = tempdir().expect("tempdir");
    let store = SessionStore::new(dir.path()).expect("session store");
    let mut session = Session::new("system:wecom:chat-42");
    session.active_profile = Some("profile".to_string());
    session.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("hello"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    store.save(&session).expect("save session");

    let loaded = store
        .get_session_detail("system:wecom:chat-42")
        .expect("load session")
        .expect("session");

    assert_eq!(loaded.key, "system:wecom:chat-42");
}
