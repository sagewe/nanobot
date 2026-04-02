use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail, ensure};
use chrono::Utc;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::providers::{LlmProvider, ProviderRequestDescriptor};
use crate::session::{Session, SessionMessage};

#[derive(Debug, Clone)]
pub struct MemoryStore {
    dir: PathBuf,
    memory_path: PathBuf,
    history_path: PathBuf,
}

impl MemoryStore {
    pub fn new(workspace: &Path) -> Result<Self> {
        let dir = workspace.join("memory");
        fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
        let store = Self {
            memory_path: dir.join("MEMORY.md"),
            history_path: dir.join("HISTORY.md"),
            dir,
        };
        store.ensure_files()?;
        Ok(store)
    }

    pub fn ensure_files(&self) -> Result<()> {
        if !self.memory_path.exists() {
            fs::write(&self.memory_path, "# MEMORY\n\nStore durable facts here.\n")
                .with_context(|| format!("failed to write {}", self.memory_path.display()))?;
        }
        if !self.history_path.exists() {
            fs::write(
                &self.history_path,
                "# HISTORY\n\nAppend consolidation events here.\n",
            )
            .with_context(|| format!("failed to write {}", self.history_path.display()))?;
        }
        Ok(())
    }

    pub fn read_memory(&self) -> Result<String> {
        self.ensure_files()?;
        fs::read_to_string(&self.memory_path)
            .with_context(|| format!("failed to read {}", self.memory_path.display()))
    }

    pub fn write_memory(&self, content: &str) -> Result<()> {
        self.ensure_files()?;
        fs::write(&self.memory_path, content)
            .with_context(|| format!("failed to write {}", self.memory_path.display()))
    }

    pub fn append_history(&self, entry: &str) -> Result<()> {
        self.ensure_files()?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.history_path)
            .with_context(|| format!("failed to open {}", self.history_path.display()))?;
        if file.metadata()?.len() > 0 {
            writeln!(file)?;
        }
        writeln!(file, "{entry}")
            .with_context(|| format!("failed to append {}", self.history_path.display()))?;
        Ok(())
    }

    pub fn append_raw_archive(&self, label: &str, content: &str) -> Result<PathBuf> {
        let archive_dir = self.dir.join("raw-archive");
        fs::create_dir_all(&archive_dir)
            .with_context(|| format!("failed to create {}", archive_dir.display()))?;
        let safe_label = label
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
            .collect::<String>();
        let safe_label = safe_label.trim_matches('-');
        let path = archive_dir.join(format!(
            "{}-{}.md",
            if safe_label.is_empty() {
                "memory-archive"
            } else {
                safe_label
            },
            Utc::now().format("%Y%m%dT%H%M%S")
        ));
        fs::write(
            &path,
            format!("# Raw archive\n\nLabel: {label}\n\n{content}\n"),
        )
        .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(path)
    }

    pub fn memory_path(&self) -> &Path {
        &self.memory_path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsolidationPolicy {
    pub max_context_tokens: usize,
    pub target_context_tokens: usize,
    pub retry_limit: usize,
    pub max_rounds: usize,
}

pub struct MemoryConsolidator {
    store: MemoryStore,
    provider: Arc<dyn LlmProvider>,
    policy: ConsolidationPolicy,
    failure_counts: Arc<Mutex<HashMap<String, usize>>>,
    session_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

#[derive(Debug)]
struct SaveMemoryPayload {
    history_entry: String,
    memory_update: Option<String>,
}

impl MemoryConsolidator {
    pub fn new(
        workspace: &Path,
        provider: Arc<dyn LlmProvider>,
        policy: ConsolidationPolicy,
    ) -> Result<Self> {
        Ok(Self {
            store: MemoryStore::new(workspace)?,
            provider,
            policy,
            failure_counts: Arc::new(Mutex::new(HashMap::new())),
            session_locks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn estimated_unconsolidated_tokens(&self, session: &Session) -> usize {
        estimate_messages(session.unconsolidated_tail())
    }

    pub fn consolidation_boundary(&self, session: &Session) -> Option<usize> {
        let boundary = select_compaction_boundary(session, &self.policy);
        (boundary > session.clamped_last_consolidated()).then_some(boundary)
    }

    pub async fn failure_count(&self, session_key: &str) -> usize {
        self.failure_counts
            .lock()
            .await
            .get(session_key)
            .copied()
            .unwrap_or(0)
    }

    pub fn save_memory_tool_schema() -> Value {
        save_memory_tool()
    }

    pub fn validate_save_memory_response(
        response: crate::providers::LlmResponse,
    ) -> Result<(String, Option<String>)> {
        let payload = parse_save_memory_response(response)?;
        Ok((payload.history_entry, payload.memory_update))
    }

    pub async fn flush_session(
        &self,
        session: &mut Session,
        request: &ProviderRequestDescriptor,
        reason: &str,
    ) -> Result<bool> {
        let end = session.messages.len();
        if end <= session.clamped_last_consolidated() {
            return Ok(false);
        }
        self.consolidate_range(session, request, end, reason).await
    }

    pub async fn consolidate_if_needed(
        &self,
        session: &mut Session,
        request: &ProviderRequestDescriptor,
    ) -> Result<bool> {
        if self.policy.max_context_tokens == 0 {
            return self
                .flush_session(session, request, "token budget consolidation")
                .await;
        }

        if self.estimated_unconsolidated_tokens(session) <= self.policy.max_context_tokens {
            return Ok(false);
        }

        let Some(end) = self.consolidation_boundary(session) else {
            return Ok(false);
        };
        self.consolidate_range(session, request, end, "token budget consolidation")
            .await
    }

    pub async fn archive_unconsolidated_tail(
        &self,
        session: &mut Session,
        reason: &str,
    ) -> Result<Option<PathBuf>> {
        let end = session.messages.len();
        let start = session.clamped_last_consolidated();
        if end <= start {
            return Ok(None);
        }
        let raw = render_archive_slice(session.unconsolidated_slice_to(end));
        let path = self.archive_raw(session, end, reason, &raw)?;
        self.clear_failure_count(&session.key).await;
        Ok(Some(path))
    }

    async fn consolidate_range(
        &self,
        session: &mut Session,
        request: &ProviderRequestDescriptor,
        end: usize,
        reason: &str,
    ) -> Result<bool> {
        let start = session.clamped_last_consolidated();
        if end <= start {
            return Ok(false);
        }

        let lock = self.session_lock(&session.key).await;
        let _guard = lock.lock().await;

        let slice = session.unconsolidated_slice_to(end);
        let visible = promptable_slice(slice);

        if visible.is_empty() {
            session.last_consolidated = end;
            self.clear_failure_count(&session.key).await;
            return Ok(true);
        }

        let memory = self.store.read_memory()?;
        let prompt = build_consolidation_prompt(&session.key, reason, &memory, &visible);
        let payload = match self.request_save_memory(request, &prompt).await {
            Ok(payload) => payload,
            Err(error) => {
                let failures = self.increment_failure_count(&session.key).await;
                if failures >= self.policy.retry_limit.max(1) {
                    let raw = render_archive_slice(slice);
                    self.archive_raw(session, end, reason, &raw)?;
                    self.clear_failure_count(&session.key).await;
                }
                return Err(error);
            }
        };

        self.store.append_history(payload.history_entry.trim())?;
        if let Some(memory_update) = payload.memory_update.as_deref() {
            self.store.write_memory(memory_update).with_context(|| {
                format!(
                    "failed to persist {} for session {}",
                    self.store.memory_path().display(),
                    session.key
                )
            })?;
        }

        session.last_consolidated = end;
        self.clear_failure_count(&session.key).await;
        Ok(true)
    }

    async fn request_save_memory(
        &self,
        request: &ProviderRequestDescriptor,
        prompt: &str,
    ) -> Result<SaveMemoryPayload> {
        let attempts = self.policy.retry_limit.max(1);
        let messages = vec![
            json!({
                "role": "system",
                "content": "You compress durable memory for Sidekick. Always answer with a single save_memory tool call.",
            }),
            json!({
                "role": "user",
                "content": prompt,
            }),
        ];
        let tools = vec![Self::save_memory_tool_schema()];

        let mut last_error = None;
        for _ in 0..attempts {
            match self
                .provider
                .chat_with_request_retry(messages.clone(), tools.clone(), request)
                .await
            {
                Ok(response) => match parse_save_memory_response(response) {
                    Ok(payload) => return Ok(payload),
                    Err(error) => last_error = Some(error),
                },
                Err(error) => last_error = Some(error),
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("save_memory failed without an error")))
    }

    async fn increment_failure_count(&self, session_key: &str) -> usize {
        let mut counts = self.failure_counts.lock().await;
        let count = counts.entry(session_key.to_string()).or_insert(0);
        *count += 1;
        *count
    }

    async fn clear_failure_count(&self, session_key: &str) {
        self.failure_counts.lock().await.remove(session_key);
    }

    fn archive_raw(
        &self,
        session: &mut Session,
        end: usize,
        reason: &str,
        raw: &str,
    ) -> Result<PathBuf> {
        let path = self
            .store
            .append_raw_archive(&format!("{}-{reason}", session.key), raw)?;
        self.store.append_history(&format!(
            "## {} [RAW]\n- session: {}\n- reason: {}\n- archive: {}",
            Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
            session.key,
            reason,
            path.display()
        ))?;
        session.last_consolidated = end;
        Ok(path)
    }

    async fn session_lock(&self, session_key: &str) -> Arc<Mutex<()>> {
        let mut locks = self.session_locks.lock().await;
        locks
            .entry(session_key.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

fn select_compaction_boundary(session: &Session, policy: &ConsolidationPolicy) -> usize {
    let start = session.clamped_last_consolidated();
    let boundaries = session
        .legal_consolidation_boundaries()
        .into_iter()
        .filter(|boundary| *boundary < session.messages.len())
        .collect::<Vec<_>>();
    if boundaries.is_empty() {
        return session.messages.len();
    }

    let total = estimate_messages(session.unconsolidated_tail());
    if total <= policy.max_context_tokens {
        return start;
    }

    let mut fallback = *boundaries.last().unwrap_or(&session.messages.len());
    for boundary in boundaries {
        let remaining_visible = promptable_slice(&session.messages[boundary..]);
        if remaining_visible.is_empty() {
            return session.messages.len();
        }
        if estimate_messages(&remaining_visible) <= policy.target_context_tokens {
            return boundary;
        }
        fallback = boundary;
    }

    fallback
}

fn estimate_messages(messages: &[SessionMessage]) -> usize {
    messages
        .iter()
        .filter(|message| !message.excluded_from_context())
        .map(estimate_message)
        .sum()
}

fn estimate_message(message: &SessionMessage) -> usize {
    let mut tokens = 0usize;
    if let Some(text) = message.content.as_str() {
        tokens += text.split_whitespace().count();
    }
    if let Some(tool_calls) = &message.tool_calls {
        tokens += tool_calls.len() * 2;
    }
    if message.role == "tool" && message.name.is_some() {
        tokens += 1;
    }
    tokens.max(1)
}

fn build_consolidation_prompt(
    session_key: &str,
    reason: &str,
    current_memory: &str,
    slice: &[SessionMessage],
) -> String {
    format!(
        "Session: {session_key}\nReason: {reason}\n\nCurrent MEMORY.md:\n{current_memory}\n\nConversation slice to consolidate:\n{}\n\nWrite a save_memory tool call with:\n- history_entry: a concise markdown summary for HISTORY.md\n- memory_update: the full new MEMORY.md content, or null if MEMORY.md should stay unchanged",
        render_prompt_slice(slice)
    )
}

fn render_prompt_slice(slice: &[SessionMessage]) -> String {
    slice
        .iter()
        .map(|message| {
            let mut parts = vec![format!("{}:", message.role.to_uppercase())];
            if let Some(text) = message.content.as_str() {
                let text = text.trim();
                if !text.is_empty() {
                    parts.push(text.to_string());
                }
            }
            if let Some(tool_calls) = &message.tool_calls {
                let names = tool_calls
                    .iter()
                    .filter_map(|call| call.pointer("/function/name").and_then(Value::as_str))
                    .collect::<Vec<_>>();
                if !names.is_empty() {
                    parts.push(format!("tool_calls={}", names.join(",")));
                }
            }
            if message.role == "tool" {
                if let Some(name) = &message.name {
                    parts.push(format!("name={name}"));
                }
                if let Some(tool_call_id) = &message.tool_call_id {
                    parts.push(format!("tool_call_id={tool_call_id}"));
                }
            }
            parts.join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_archive_slice(slice: &[SessionMessage]) -> String {
    slice
        .iter()
        .map(|message| {
            format!(
                "- role={}; content={}; excluded={}",
                message.role,
                message
                    .content
                    .as_str()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| message.content.to_string()),
                message.excluded_from_context()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn promptable_slice(slice: &[SessionMessage]) -> Vec<SessionMessage> {
    let visible = slice
        .iter()
        .filter(|message| !message.excluded_from_context())
        .cloned()
        .collect::<Vec<_>>();
    let Some(first_user) = visible.iter().position(|message| message.role == "user") else {
        return Vec::new();
    };
    let mut visible = visible[first_user..].to_vec();
    let safe_start = Session::safe_history_start(&visible);
    if safe_start > 0 {
        visible = visible[safe_start..].to_vec();
    }
    visible
}

fn save_memory_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "save_memory",
            "description": "Persist a history entry and optional MEMORY.md rewrite for a consolidated session slice.",
            "parameters": {
                "type": "object",
                "properties": {
                    "history_entry": {
                        "type": "string"
                    },
                    "memory_update": {
                        "type": ["string", "null"]
                    }
                },
                "required": ["history_entry", "memory_update"],
                "additionalProperties": false
            }
        }
    })
}

fn parse_save_memory_response(
    response: crate::providers::LlmResponse,
) -> Result<SaveMemoryPayload> {
    ensure!(
        response.tool_calls.len() == 1,
        "provider returned unexpected tool output during save_memory"
    );
    let tool_call = response
        .tool_calls
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("provider did not return a save_memory tool call"))?;
    ensure!(
        tool_call.name == "save_memory",
        "provider did not return a save_memory tool call"
    );
    let args = tool_call
        .arguments
        .as_object()
        .ok_or_else(|| anyhow!("save_memory arguments must be a JSON object"))?;
    ensure!(
        args.len() == 2,
        "save_memory arguments must only contain history_entry and memory_update"
    );
    let history_entry = args
        .get("history_entry")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("save_memory missing history_entry"))?
        .to_string();
    let memory_update = match args.get("memory_update") {
        Some(Value::Null) => None,
        Some(Value::String(content)) => Some(content.clone()),
        Some(_) => bail!("save_memory memory_update must be a string or null"),
        None => bail!("save_memory missing memory_update"),
    };
    Ok(SaveMemoryPayload {
        history_entry,
        memory_update,
    })
}
