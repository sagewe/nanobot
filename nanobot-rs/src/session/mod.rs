use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::warn;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionMessage {
    pub role: String,
    #[serde(default)]
    pub content: Value,
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,
    #[serde(default)]
    pub tool_calls: Option<Vec<Value>>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl SessionMessage {
    pub fn excluded_from_context(&self) -> bool {
        self.extra
            .get("_exclude_from_context")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    pub fn to_llm_message(&self) -> Value {
        let mut obj = self.extra.clone();
        obj.insert("role".to_string(), json!(self.role));
        obj.insert("content".to_string(), self.content.clone());
        if let Some(tool_calls) = &self.tool_calls {
            obj.insert("tool_calls".to_string(), json!(tool_calls));
        }
        if let Some(tool_call_id) = &self.tool_call_id {
            obj.insert("tool_call_id".to_string(), json!(tool_call_id));
        }
        if let Some(name) = &self.name {
            obj.insert("name".to_string(), json!(name));
        }
        Value::Object(obj)
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub key: String,
    pub active_profile: Option<String>,
    pub source_session_key: Option<String>,
    pub messages: Vec<SessionMessage>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_consolidated: usize,
}

impl Session {
    pub fn new(key: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            key: key.into(),
            active_profile: None,
            source_session_key: None,
            messages: Vec::new(),
            created_at: now,
            updated_at: now,
            last_consolidated: 0,
        }
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.last_consolidated = 0;
        self.updated_at = Utc::now();
    }

    pub fn active_profile_or<'a>(&'a self, default_profile: &'a str) -> &'a str {
        self.active_profile.as_deref().unwrap_or(default_profile)
    }

    pub fn get_history(&self, max_messages: usize) -> Vec<Value> {
        let unconsolidated = self.messages[self.last_consolidated..]
            .iter()
            .filter(|message| !message.excluded_from_context())
            .cloned()
            .collect::<Vec<_>>();
        let mut sliced = if max_messages == 0 || max_messages >= unconsolidated.len() {
            unconsolidated
        } else {
            unconsolidated[unconsolidated.len() - max_messages..].to_vec()
        };

        if let Some(pos) = sliced.iter().position(|m| m.role == "user") {
            sliced = sliced[pos..].to_vec();
        }

        let start = legal_start(&sliced);
        if start > 0 {
            sliced = sliced[start..].to_vec();
        }

        sliced.into_iter().map(|m| m.to_llm_message()).collect()
    }
}

fn legal_start(messages: &[SessionMessage]) -> usize {
    let mut declared = HashSet::<String>::new();
    let mut start = 0;
    for (idx, msg) in messages.iter().enumerate() {
        match msg.role.as_str() {
            "assistant" => {
                if let Some(tool_calls) = &msg.tool_calls {
                    for call in tool_calls {
                        if let Some(id) = call.get("id").and_then(Value::as_str) {
                            declared.insert(id.to_string());
                        }
                    }
                }
            }
            "tool" => {
                if let Some(tool_call_id) = &msg.tool_call_id {
                    if !declared.contains(tool_call_id) {
                        start = idx + 1;
                        declared.clear();
                        for prev in &messages[start..=idx] {
                            if prev.role == "assistant" {
                                if let Some(tool_calls) = &prev.tool_calls {
                                    for call in tool_calls {
                                        if let Some(id) = call.get("id").and_then(Value::as_str) {
                                            declared.insert(id.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    start
}

#[derive(Debug, Clone)]
pub struct SessionStore {
    dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub key: String,
    pub channel: String,
    pub session_id: String,
    pub active_profile: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: usize,
    pub preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionGroupSummary {
    pub channel: String,
    pub sessions: Vec<SessionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionMetadata {
    #[serde(rename = "_type")]
    kind: String,
    key: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    last_consolidated: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_session_key: Option<String>,
}

impl SessionStore {
    pub fn new(workspace: &Path) -> Result<Self> {
        let dir = workspace.join("sessions");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;
        Ok(Self { dir })
    }

    pub fn path_for(&self, key: &str) -> PathBuf {
        let safe = key.replace(':', "_");
        self.dir.join(format!("{safe}.jsonl"))
    }

    pub fn get_or_create(&self, key: &str) -> Result<Session> {
        Ok(self.load(key)?.unwrap_or_else(|| Session::new(key)))
    }

    pub fn get_or_create_with_default_profile(
        &self,
        key: &str,
        default_profile: &str,
    ) -> Result<Session> {
        let mut session = self.get_or_create(key)?;
        if session.active_profile.is_none() {
            session.active_profile = Some(default_profile.to_string());
        }
        Ok(session)
    }

    pub fn load(&self, key: &str) -> Result<Option<Session>> {
        let path = self.path_for(key);
        if !path.exists() {
            return Ok(None);
        }
        self.load_from_path(&path).map(Some)
    }

    pub fn get_session_detail(&self, key: &str) -> Result<Option<Session>> {
        self.load(key)
    }

    pub fn get_session_summary(&self, key: &str) -> Result<Option<SessionSummary>> {
        Ok(self.load(key)?.map(|session| session.into_summary()))
    }

    pub fn list_sessions_across_namespaces(&self) -> Result<Vec<SessionSummary>> {
        let mut sessions = Vec::new();
        for path in self.session_file_paths()? {
            if let Some(session) = self.load_session_from_path_logged(&path) {
                sessions.push(session.into_summary());
            }
        }
        sessions.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.key.cmp(&b.key))
        });
        Ok(sessions)
    }

    pub fn list_sessions_grouped_by_channel(&self) -> Result<Vec<SessionGroupSummary>> {
        let mut grouped = BTreeMap::<String, Vec<SessionSummary>>::new();
        for session in self.list_sessions_across_namespaces()? {
            grouped
                .entry(session.channel.clone())
                .or_default()
                .push(session);
        }
        Ok(grouped
            .into_iter()
            .map(|(channel, sessions)| SessionGroupSummary { channel, sessions })
            .collect())
    }

    pub fn list_sessions_in_namespace(&self, namespace: &str) -> Result<Vec<SessionSummary>> {
        let prefix = namespace_prefix(namespace);
        let mut sessions = Vec::new();
        for path in self.session_file_paths()? {
            let Some(session) = self.load_session_from_path_logged(&path) else {
                continue;
            };
            if session.key.starts_with(&prefix) {
                sessions.push(session.into_summary());
            }
        }
        sessions.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.key.cmp(&b.key))
        });
        Ok(sessions)
    }

    pub fn delete_session(&self, key: &str) -> Result<bool> {
        let path = self.path_for(key);
        if !path.exists() {
            return Ok(false);
        }
        std::fs::remove_file(&path)
            .with_context(|| format!("failed to delete session {}", path.display()))?;
        Ok(true)
    }

    pub fn duplicate_session_to_web(&self, source_key: &str) -> Result<Session> {
        let source = self
            .load(source_key)?
            .with_context(|| format!("session {source_key} not found"))?;
        let now = Utc::now();
        let mut duplicated = Session::new(format!("web:{}", Uuid::new_v4()));
        duplicated.active_profile = source.active_profile.clone();
        duplicated.source_session_key = Some(source.key.clone());
        duplicated.messages = source.messages.clone();
        duplicated.created_at = now;
        duplicated.updated_at = now;
        duplicated.last_consolidated = source.last_consolidated;
        self.save(&duplicated)?;
        Ok(duplicated)
    }

    fn load_from_path(&self, path: &Path) -> Result<Session> {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read session {}", path.display()))?;
        let mut key = metadata_key_from_path(path);
        let mut created_at = Utc::now();
        let mut updated_at = Utc::now();
        let mut last_consolidated = 0usize;
        let mut active_profile = None;
        let mut source_session_key = None;
        let mut messages = Vec::new();
        for line in raw.lines().filter(|line| !line.trim().is_empty()) {
            let value: Value = serde_json::from_str(line)?;
            if value.get("_type").and_then(Value::as_str) == Some("metadata") {
                let metadata: SessionMetadata = serde_json::from_value(value)?;
                if metadata.kind != "metadata" {
                    continue;
                }
                key = metadata.key;
                created_at = metadata.created_at;
                updated_at = metadata.updated_at;
                last_consolidated = metadata.last_consolidated;
                active_profile = metadata.active_profile;
                source_session_key = metadata.source_session_key;
            } else {
                messages.push(serde_json::from_value(value)?);
            }
        }
        Ok(Session {
            key,
            active_profile,
            source_session_key,
            messages,
            created_at,
            updated_at,
            last_consolidated,
        })
    }

    pub fn save(&self, session: &Session) -> Result<()> {
        let mut lines = Vec::with_capacity(session.messages.len() + 1);
        lines.push(serde_json::to_string(&SessionMetadata {
            kind: "metadata".to_string(),
            key: session.key.clone(),
            created_at: session.created_at,
            updated_at: session.updated_at,
            last_consolidated: session.last_consolidated,
            active_profile: session.active_profile.clone(),
            source_session_key: session.source_session_key.clone(),
        })?);
        for message in &session.messages {
            lines.push(serde_json::to_string(message)?);
        }
        let path = self.path_for(&session.key);
        std::fs::write(&path, lines.join("\n") + "\n")
            .with_context(|| format!("failed to write session {}", path.display()))?;
        Ok(())
    }

    fn session_file_paths(&self) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(&self.dir)
            .with_context(|| format!("failed to read {}", self.dir.display()))?
        {
            let path = entry?.path();
            if path.is_file() {
                paths.push(path);
            }
        }
        Ok(paths)
    }

    fn load_session_from_path_logged(&self, path: &Path) -> Option<Session> {
        match self.load_from_path(path) {
            Ok(session) => Some(session),
            Err(err) => {
                warn!(path = %path.display(), error = %err, "skipping unreadable session file");
                None
            }
        }
    }
}

impl Session {
    fn into_summary(self) -> SessionSummary {
        let (channel, session_id) = split_session_key(&self.key);
        SessionSummary {
            key: self.key,
            channel,
            session_id,
            active_profile: self.active_profile,
            created_at: self.created_at,
            updated_at: self.updated_at,
            message_count: self.messages.len(),
            preview: self.messages.iter().rev().find_map(message_preview),
        }
    }
}

fn message_preview(message: &SessionMessage) -> Option<String> {
    if !matches!(message.role.as_str(), "user" | "assistant") {
        return None;
    }
    match &message.content {
        Value::String(text) if !text.trim().is_empty() => Some(truncate_preview(text, 120)),
        Value::Null => None,
        other => Some(truncate_preview(&other.to_string(), 120)),
    }
}

fn truncate_preview(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    let chars = trimmed.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return trimmed.to_string();
    }
    format!("{}…", chars[..max_chars].iter().collect::<String>())
}

pub fn split_session_key(key: &str) -> (String, String) {
    if let Some((channel, session_id)) = key.split_once(':') {
        (channel.to_string(), session_id.to_string())
    } else {
        (key.to_string(), String::new())
    }
}

fn namespace_prefix(namespace: &str) -> String {
    if namespace.ends_with(':') {
        namespace.to_string()
    } else {
        format!("{namespace}:")
    }
}

fn metadata_key_from_path(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    if let Some((namespace, rest)) = stem.split_once('_') {
        format!("{namespace}:{rest}")
    } else {
        stem.to_string()
    }
}
