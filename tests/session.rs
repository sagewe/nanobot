use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Map, Value, json};
use sidekick::agent::memory::{ConsolidationPolicy, MemoryConsolidator};
use sidekick::providers::{LlmProvider, LlmResponse, ProviderRequestDescriptor, ToolCall};
use sidekick::session::{Session, SessionMessage, SessionStore};
use tempfile::tempdir;
use tokio::sync::Mutex;
use tracing::subscriber::with_default;

struct SharedWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("writer lock").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn value_messages(messages: &[SessionMessage]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .map(|message| serde_json::to_value(message).expect("session message"))
        .collect()
}

fn warning_log(buffer: &std::sync::Arc<std::sync::Mutex<Vec<u8>>>) -> String {
    String::from_utf8(buffer.lock().expect("buffer lock").clone()).expect("utf8 log")
}

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

#[derive(Clone)]
enum MemoryProviderReply {
    Response(LlmResponse),
}

impl MemoryProviderReply {
    fn ok(response: LlmResponse) -> Self {
        Self::Response(response)
    }
}

#[derive(Default)]
struct MemoryProviderState {
    calls: Arc<Mutex<Vec<Vec<Value>>>>,
    replies: Arc<Mutex<VecDeque<MemoryProviderReply>>>,
    tamper_memory_path: Arc<Mutex<Option<PathBuf>>>,
}

#[derive(Clone)]
struct SessionMemoryProvider {
    state: Arc<MemoryProviderState>,
}

impl SessionMemoryProvider {
    async fn pop_reply(queue: &Mutex<VecDeque<MemoryProviderReply>>) -> Result<LlmResponse> {
        match queue.lock().await.pop_front() {
            Some(MemoryProviderReply::Response(response)) => Ok(response),
            None => Err(anyhow::anyhow!("no queued reply")),
        }
    }
}

#[async_trait]
impl LlmProvider for SessionMemoryProvider {
    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn chat(
        &self,
        _messages: Vec<Value>,
        _tools: Vec<Value>,
        _model: &str,
    ) -> Result<LlmResponse> {
        Err(anyhow::anyhow!(
            "chat() should not be used for session memory provider"
        ))
    }

    async fn chat_with_request(
        &self,
        messages: Vec<Value>,
        _tools: Vec<Value>,
        _request: &ProviderRequestDescriptor,
    ) -> Result<LlmResponse> {
        self.state.calls.lock().await.push(messages);
        if let Some(path) = self.state.tamper_memory_path.lock().await.take() {
            if path.is_file() {
                fs::remove_file(&path).expect("remove memory file");
            }
            fs::create_dir_all(&path).expect("replace memory file with directory");
        }
        Self::pop_reply(&self.state.replies).await
    }
}

fn text_message(role: &str, content: &str) -> SessionMessage {
    SessionMessage {
        role: role.to_string(),
        content: json!(content),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    }
}

fn excluded_message(role: &str, content: &str) -> SessionMessage {
    let mut message = text_message(role, content);
    message
        .extra
        .insert("_exclude_from_context".to_string(), json!(true));
    message
}

fn assistant_tool_message(call_id: &str) -> SessionMessage {
    SessionMessage {
        role: "assistant".to_string(),
        content: Value::Null,
        timestamp: None,
        tool_calls: Some(vec![json!({
            "id": call_id,
            "type": "function",
            "function": {"name": "lookup", "arguments": "{}"}
        })]),
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    }
}

fn tool_result_message(call_id: &str) -> SessionMessage {
    SessionMessage {
        role: "tool".to_string(),
        content: json!("tool result"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: Some(call_id.to_string()),
        name: Some("lookup".to_string()),
        extra: Default::default(),
    }
}

fn save_memory_response(history_entry: &str, memory_update: Option<&str>) -> LlmResponse {
    let mut arguments = serde_json::Map::new();
    arguments.insert("history_entry".to_string(), json!(history_entry));
    arguments.insert(
        "memory_update".to_string(),
        memory_update.map_or(Value::Null, |content| json!(content)),
    );
    LlmResponse {
        content: None,
        tool_calls: vec![ToolCall {
            id: "save_memory_call".to_string(),
            name: "save_memory".to_string(),
            arguments: Value::Object(arguments),
        }],
        finish_reason: "tool_calls".to_string(),
        extra: Map::new(),
    }
}

fn memory_policy(max_context_tokens: usize, target_context_tokens: usize) -> ConsolidationPolicy {
    ConsolidationPolicy {
        max_context_tokens,
        target_context_tokens,
        retry_limit: 2,
        max_rounds: 2,
    }
}

fn default_request() -> ProviderRequestDescriptor {
    ProviderRequestDescriptor::new("openai", "mock-model", Map::new())
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
fn session_store_persists_and_reloads_last_consolidated() {
    let dir = tempdir().expect("tempdir");
    let store = SessionStore::new(dir.path()).expect("session store");

    let mut session = Session::new("cli:watermark");
    session.last_consolidated = 4;
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
        .get_session_detail("cli:watermark")
        .expect("load session")
        .expect("session");

    assert_eq!(loaded.last_consolidated, 4);
}

#[tokio::test]
async fn memory_consolidator_flushes_history_then_memory_and_advances_watermark() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(MemoryProviderState::default());
    state
        .replies
        .lock()
        .await
        .push_back(MemoryProviderReply::ok(save_memory_response(
            "## 2026-04-03\n- durable fact",
            Some("# MEMORY\n\ndurable fact\n"),
        )));
    let provider: Arc<dyn LlmProvider> = Arc::new(SessionMemoryProvider {
        state: state.clone(),
    });
    let consolidator =
        MemoryConsolidator::new(dir.path(), provider, memory_policy(1, 0)).expect("memory");
    let mut session = Session::new("cli:session-memory");
    session.messages.push(text_message("user", "remember this"));
    session.messages.push(text_message("assistant", "noted"));

    let changed = consolidator
        .flush_session(&mut session, &default_request(), "manual flush")
        .await
        .expect("flush");

    assert!(changed);
    assert_eq!(session.last_consolidated, 2);
    assert!(
        fs::read_to_string(dir.path().join("memory").join("HISTORY.md"))
            .expect("history")
            .contains("durable fact")
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("memory").join("MEMORY.md")).expect("memory"),
        "# MEMORY\n\ndurable fact\n"
    );
}

#[tokio::test]
async fn memory_consolidator_keeps_history_append_and_watermark_when_memory_write_fails() {
    let dir = tempdir().expect("tempdir");
    let memory_path = dir.path().join("memory").join("MEMORY.md");
    let state = Arc::new(MemoryProviderState::default());
    state
        .replies
        .lock()
        .await
        .push_back(MemoryProviderReply::ok(save_memory_response(
            "## 2026-04-03\n- failed memory write",
            Some("# MEMORY\n\nnew content\n"),
        )));
    *state.tamper_memory_path.lock().await = Some(memory_path);
    let provider: Arc<dyn LlmProvider> = Arc::new(SessionMemoryProvider {
        state: state.clone(),
    });
    let consolidator =
        MemoryConsolidator::new(dir.path(), provider, memory_policy(1, 0)).expect("memory");
    let mut session = Session::new("cli:session-fail");
    session.messages.push(text_message("user", "remember this"));
    session.messages.push(text_message("assistant", "noted"));

    let err = consolidator
        .flush_session(&mut session, &default_request(), "manual flush")
        .await
        .expect_err("flush should fail");

    assert!(err.to_string().contains("MEMORY.md"), "{err}");
    assert_eq!(session.last_consolidated, 0);
    assert!(
        fs::read_to_string(dir.path().join("memory").join("HISTORY.md"))
            .expect("history")
            .contains("failed memory write")
    );
}

#[test]
fn clear_resets_last_consolidated_to_zero() {
    let mut session = Session::new("cli:reset");
    session.last_consolidated = 9;
    session.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("hello"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });

    session.clear();

    assert_eq!(session.last_consolidated, 0);
    assert!(session.messages.is_empty());
}

#[test]
fn unconsolidated_tail_and_boundary_helpers_match_history_slicing() {
    let mut session = Session::new("cli:tail");
    session.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("old"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    session.messages.extend(tool_turn("tail", 0));
    session.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("current"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    session.messages.extend(tool_turn("tail", 1));
    session.last_consolidated = 4;

    assert_eq!(session.unconsolidated_tail().len(), 4);
    assert_eq!(
        Session::safe_history_start(session.unconsolidated_tail()),
        0
    );

    let history = session.get_history(100);
    assert_no_orphans(&history);
}

#[tokio::test]
async fn memory_consolidator_uses_deterministic_budget_policy_and_safe_boundary() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(MemoryProviderState::default());
    state
        .replies
        .lock()
        .await
        .push_back(MemoryProviderReply::ok(save_memory_response(
            "## 2026-04-03\n- compacted tool turn",
            None,
        )));
    let provider: Arc<dyn LlmProvider> = Arc::new(SessionMemoryProvider {
        state: state.clone(),
    });
    let consolidator =
        MemoryConsolidator::new(dir.path(), provider, memory_policy(8, 4)).expect("memory");
    let mut session = Session::new("cli:session-boundary");
    session.messages.push(text_message("user", "old turn"));
    session
        .messages
        .push(text_message("assistant", "old answer"));
    session
        .messages
        .push(text_message("user", "lookup weather"));
    session.messages.push(assistant_tool_message("call_1"));
    session.messages.push(tool_result_message("call_1"));
    session
        .messages
        .push(excluded_message("assistant", "timeline-only"));
    session.messages.push(text_message(
        "user",
        "current turn that should remain active",
    ));
    session
        .messages
        .push(text_message("assistant", "current answer"));
    session.last_consolidated = 2;

    let changed = consolidator
        .consolidate_if_needed(&mut session, &default_request())
        .await
        .expect("consolidate");

    assert!(changed);
    assert_eq!(session.last_consolidated, 6);

    let prompts = state.calls.lock().await;
    let prompt = prompts
        .last()
        .and_then(|messages| messages.last())
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(!prompt.contains("old turn"), "{prompt}");
    assert!(prompt.contains("lookup weather"), "{prompt}");
    assert!(!prompt.contains("timeline-only"), "{prompt}");
    assert!(
        !prompt.contains("current turn that should remain active"),
        "{prompt}"
    );
}

#[tokio::test]
async fn under_budget_sessions_skip_consolidation_and_restart_safe_watermarks_skip_old_turns() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(MemoryProviderState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(SessionMemoryProvider {
        state: state.clone(),
    });
    let consolidator =
        MemoryConsolidator::new(dir.path(), provider, memory_policy(200, 100)).expect("memory");

    let mut under_budget = Session::new("cli:under-budget");
    under_budget.messages.push(text_message("user", "short"));
    under_budget
        .messages
        .push(text_message("assistant", "reply"));

    let changed = consolidator
        .consolidate_if_needed(&mut under_budget, &default_request())
        .await
        .expect("skip consolidation");

    assert!(!changed);
    assert_eq!(under_budget.last_consolidated, 0);
    assert!(state.calls.lock().await.is_empty());

    let store = SessionStore::new(dir.path()).expect("session store");
    let mut source = Session::new("telegram:thread-9");
    source
        .messages
        .push(text_message("user", "already compacted"));
    source
        .messages
        .push(text_message("assistant", "old answer"));
    source
        .messages
        .push(text_message("user", "keep this unconsolidated"));
    source
        .messages
        .push(text_message("assistant", "new answer"));
    source.last_consolidated = 2;
    store.save(&source).expect("save source");

    let mut duplicated = store
        .duplicate_session_to_web("telegram:thread-9")
        .expect("duplicate");
    assert_eq!(duplicated.last_consolidated, 2);

    state
        .replies
        .lock()
        .await
        .push_back(MemoryProviderReply::ok(save_memory_response(
            "## 2026-04-03\n- flushed duplicate tail",
            None,
        )));

    let changed = consolidator
        .flush_session(&mut duplicated, &default_request(), "restart flush")
        .await
        .expect("flush duplicate");

    assert!(changed);
    assert_eq!(duplicated.last_consolidated, 4);

    let prompts = state.calls.lock().await;
    let prompt = prompts
        .last()
        .and_then(|messages| messages.last())
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(!prompt.contains("already compacted"), "{prompt}");
    assert!(prompt.contains("keep this unconsolidated"), "{prompt}");
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
fn session_summary_preview_ignores_timeline_only_btw_messages() {
    let dir = tempdir().expect("tempdir");
    let store = SessionStore::new(dir.path()).expect("session store");

    let mut session = Session::new("web:btw-preview");
    session.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("main task"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });

    let mut btw_extra = serde_json::Map::new();
    btw_extra.insert("_exclude_from_context".to_string(), json!(true));
    btw_extra.insert("_timeline_kind".to_string(), json!("btw_query"));
    btw_extra.insert("_btw_id".to_string(), json!("btw-1"));
    session.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("side question"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: btw_extra,
    });
    store.save(&session).expect("save session");

    let summary = store
        .get_session_summary("web:btw-preview")
        .expect("summary lookup")
        .expect("summary");

    assert_eq!(summary.preview.as_deref(), Some("main task"));
}

#[test]
fn session_store_lists_cross_namespace_sessions_and_grouped_channels() {
    let dir = tempdir().expect("tempdir");
    let store = SessionStore::new(dir.path()).expect("session store");
    let base = chrono::Utc::now();

    let mut web_one = Session::new("web:one");
    web_one.active_profile = Some("web-profile".to_string());
    web_one.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("older"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    web_one.created_at = base - chrono::Duration::seconds(3);
    web_one.updated_at = base - chrono::Duration::seconds(3);
    store.save(&web_one).expect("save web one");

    let mut system = Session::new("system:wecom:chat-42");
    system.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("middle"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    system.created_at = base - chrono::Duration::seconds(2);
    system.updated_at = base - chrono::Duration::seconds(2);
    store.save(&system).expect("save system");

    let mut web_two = Session::new("web:two");
    web_two.active_profile = Some("second-profile".to_string());
    web_two.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("newest"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    web_two.created_at = base - chrono::Duration::seconds(1);
    web_two.updated_at = base - chrono::Duration::seconds(1);
    store.save(&web_two).expect("save web two");

    let mut cli = Session::new("cli:other");
    cli.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("oldest"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    cli.created_at = base;
    cli.updated_at = base;
    store.save(&cli).expect("save cli");

    let sessions = store
        .list_sessions_across_namespaces()
        .expect("list sessions");
    assert_eq!(
        sessions
            .iter()
            .map(|session| session.key.as_str())
            .collect::<Vec<_>>(),
        vec!["cli:other", "web:two", "system:wecom:chat-42", "web:one"]
    );
    assert_eq!(sessions[2].channel, "system");
    assert_eq!(sessions[2].session_id, "wecom:chat-42");
    assert_eq!(sessions[0].channel, "cli");
    assert_eq!(sessions[0].session_id, "other");

    let grouped = store
        .list_sessions_grouped_by_channel()
        .expect("grouped sessions");
    assert_eq!(
        grouped
            .iter()
            .map(|group| group.channel.as_str())
            .collect::<Vec<_>>(),
        vec!["cli", "system", "web"]
    );
    assert_eq!(grouped[1].sessions.len(), 1);
    assert_eq!(grouped[1].sessions[0].channel, "system");
    assert_eq!(grouped[1].sessions[0].session_id, "wecom:chat-42");
    assert_eq!(
        grouped[2]
            .sessions
            .iter()
            .map(|session| session.key.as_str())
            .collect::<Vec<_>>(),
        vec!["web:two", "web:one"]
    );
}

#[test]
fn duplicate_to_web_copies_history_and_source_metadata() {
    let dir = tempdir().expect("tempdir");
    let store = SessionStore::new(dir.path()).expect("session store");

    let mut source = Session::new("telegram:thread-9");
    source.active_profile = Some("archive-profile".to_string());
    source.source_session_key = Some("signal:origin-1".to_string());
    source.created_at = chrono::Utc::now() - chrono::Duration::seconds(10);
    source.updated_at = chrono::Utc::now() - chrono::Duration::seconds(5);
    source.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("ignored"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    source.messages.extend(tool_turn("first", 0));
    source.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("keep"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    source.messages.extend(tool_turn("second", 1));
    source.last_consolidated = 4;
    store.save(&source).expect("save source");

    let duplicated = store
        .duplicate_session_to_web("telegram:thread-9")
        .expect("duplicate session");

    assert!(duplicated.key.starts_with("web:"));
    assert_ne!(duplicated.key, source.key);
    assert_eq!(
        duplicated.active_profile.as_deref(),
        Some("archive-profile")
    );
    assert_eq!(
        duplicated.source_session_key.as_deref(),
        Some("telegram:thread-9")
    );
    assert_eq!(
        value_messages(&duplicated.messages),
        value_messages(&source.messages)
    );
    assert_eq!(duplicated.last_consolidated, 4);

    let loaded = store
        .get_session_detail(&duplicated.key)
        .expect("load duplicated")
        .expect("duplicated session");
    assert_eq!(loaded.active_profile.as_deref(), Some("archive-profile"));
    assert_eq!(
        loaded.source_session_key.as_deref(),
        Some("telegram:thread-9")
    );
    assert_eq!(
        value_messages(&loaded.messages),
        value_messages(&source.messages)
    );
    assert_eq!(loaded.last_consolidated, 4);

    let copied_history = loaded.get_history(100);
    assert_eq!(copied_history, source.get_history(100));
    assert_no_orphans(&copied_history);
}

#[test]
fn duplicate_to_web_sets_source_key_when_source_has_no_ancestor_metadata() {
    let dir = tempdir().expect("tempdir");
    let store = SessionStore::new(dir.path()).expect("session store");

    let mut source = Session::new("cli:thread-7");
    source.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("hello"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    store.save(&source).expect("save source");

    let duplicated = store
        .duplicate_session_to_web("cli:thread-7")
        .expect("duplicate session");

    assert_eq!(
        duplicated.source_session_key.as_deref(),
        Some("cli:thread-7")
    );
    let loaded = store
        .get_session_detail(&duplicated.key)
        .expect("load duplicated")
        .expect("duplicated session");
    assert_eq!(loaded.source_session_key.as_deref(), Some("cli:thread-7"));
}

#[test]
fn get_history_excludes_timeline_only_messages_from_model_context() {
    let mut session = Session::new("web:timeline");
    session.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("main question"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    session.messages.push(SessionMessage {
        role: "assistant".to_string(),
        content: json!("main answer"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    session.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("/btw quick question"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: serde_json::Map::from_iter([("_exclude_from_context".to_string(), json!(true))]),
    });
    session.messages.push(SessionMessage {
        role: "assistant".to_string(),
        content: json!("btw answer"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: serde_json::Map::from_iter([("_exclude_from_context".to_string(), json!(true))]),
    });

    let history = session.get_history(100);
    let contents = history
        .iter()
        .filter_map(|message| message.get("content").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>();

    assert_eq!(contents, vec!["main question", "main answer"]);
}

#[test]
fn list_sessions_warns_when_a_session_file_is_skipped() {
    let dir = tempdir().expect("tempdir");
    let store = SessionStore::new(dir.path()).expect("session store");

    let mut good = Session::new("web:ok");
    good.messages.push(SessionMessage {
        role: "user".to_string(),
        content: json!("hello"),
        timestamp: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
        extra: Default::default(),
    });
    store.save(&good).expect("save good session");

    let bad_path = store.path_for("cli:broken");
    std::fs::write(&bad_path, "{not-json").expect("write bad session");

    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_target(false)
        .without_time()
        .with_writer({
            let captured = captured.clone();
            move || SharedWriter(captured.clone())
        })
        .finish();

    let sessions = with_default(subscriber, || {
        store
            .list_sessions_across_namespaces()
            .expect("list sessions")
    });

    assert_eq!(sessions.len(), 1);
    let log = warning_log(&captured);
    assert!(log.contains(bad_path.to_string_lossy().as_ref()));
    assert!(log.contains("skipping"));
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
    assert_eq!(loaded.last_consolidated, 0);
}
