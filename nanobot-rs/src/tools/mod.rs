mod web;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use chrono::Utc;
use regex::Regex;
use serde_json::{Value, json};
use similar::TextDiff;
use tokio::process::Command;
use tokio::sync::Mutex;
use walkdir::WalkDir;

use crate::agent::SubagentManager;
use crate::bus::{MessageBus, OutboundMessage};
use crate::config::WebToolsConfig;
use crate::cron::{CronSchedule, CronService};
use crate::providers::ProviderRequestDescriptor;
use crate::security::network::contains_internal_url;

pub use web::{WebFetchTool, WebSearchTool};

pub(crate) fn truncate_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

fn take_last_chars(input: &str, max_chars: usize) -> String {
    let chars = input.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(max_chars);
    chars[start..].iter().collect()
}

#[derive(Debug, Clone, Default)]
pub struct ToolContext {
    pub channel: String,
    pub chat_id: String,
    pub session_key: String,
    pub message_id: Option<String>,
    pub reply_to_caller: bool,
    pub provider_request: Option<ProviderRequestDescriptor>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn schema(&self) -> Value;

    async fn execute(&self, args: Value) -> String;

    async fn set_context(&self, _context: ToolContext) {}
    async fn start_turn(&self) {}
    async fn take_direct_replies(&self) -> Vec<String> {
        Vec::new()
    }
    fn sent_in_turn(&self) -> bool {
        false
    }
}

#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: Arc<Mutex<HashMap<String, Arc<dyn Tool>>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register<T: Tool + 'static>(&self, tool: T) {
        self.tools
            .lock()
            .await
            .insert(tool.name().to_string(), Arc::new(tool));
    }

    pub async fn execute(&self, name: &str, args: Value) -> String {
        let tool = self.tools.lock().await.get(name).cloned();
        if let Some(tool) = tool {
            tool.execute(args).await
        } else {
            format!("Error: Tool '{name}' not found")
        }
    }

    pub async fn definitions(&self) -> Vec<Value> {
        self.tools
            .lock()
            .await
            .values()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name(),
                        "description": tool.description(),
                        "parameters": tool.schema(),
                    }
                })
            })
            .collect()
    }

    pub async fn set_context(&self, context: ToolContext) {
        let tools = self
            .tools
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for tool in tools {
            tool.set_context(context.clone()).await;
        }
    }

    pub async fn start_turn(&self) {
        let tools = self
            .tools
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for tool in tools {
            tool.start_turn().await;
        }
    }

    pub async fn sent_message_this_turn(&self) -> bool {
        self.tools
            .lock()
            .await
            .values()
            .any(|tool| tool.sent_in_turn())
    }

    pub async fn take_direct_replies(&self) -> Vec<String> {
        let tools = self
            .tools
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut replies = Vec::new();
        for tool in tools {
            replies.extend(tool.take_direct_replies().await);
        }
        replies
    }
}

fn resolve_path(path: &str, workspace: &Path, restrict: bool) -> anyhow::Result<PathBuf> {
    let input = PathBuf::from(path);
    let path = if input.is_absolute() {
        input
    } else {
        workspace.join(input)
    };
    let resolved = path.canonicalize().unwrap_or(path.clone());
    if restrict && !resolved.starts_with(workspace) {
        anyhow::bail!("Path {path:?} is outside workspace {}", workspace.display());
    }
    Ok(resolved)
}

pub struct ReadFileTool {
    workspace: PathBuf,
    restrict: bool,
}

impl ReadFileTool {
    pub fn new(workspace: PathBuf, restrict: bool) -> Self {
        Self {
            workspace,
            restrict,
        }
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }
    fn description(&self) -> &'static str {
        "Read a file with line numbers and pagination."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "offset": {"type": "integer", "minimum": 1},
                "limit": {"type": "integer", "minimum": 1}
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> String {
        let path = args.get("path").and_then(Value::as_str).unwrap_or_default();
        let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(1) as usize;
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(2000) as usize;
        match resolve_path(path, &self.workspace, self.restrict) {
            Ok(path) => match std::fs::read_to_string(&path) {
                Ok(content) => {
                    let lines = content.lines().collect::<Vec<_>>();
                    if lines.is_empty() {
                        return format!("(Empty file: {})", path.display());
                    }
                    if offset > lines.len() {
                        return format!(
                            "Error: offset {offset} is beyond end of file ({} lines)",
                            lines.len()
                        );
                    }
                    let start = offset.saturating_sub(1);
                    let end = (start + limit).min(lines.len());
                    let numbered = lines[start..end]
                        .iter()
                        .enumerate()
                        .map(|(idx, line)| format!("{}| {line}", start + idx + 1))
                        .collect::<Vec<_>>();
                    let mut result = numbered.join("\n");
                    if result.chars().count() > 128_000 {
                        result = truncate_chars(&result, 128_000);
                    }
                    if end < lines.len() {
                        result.push_str(&format!(
                            "\n\n(Showing lines {}-{} of {}. Use offset={} to continue.)",
                            start + 1,
                            end,
                            lines.len(),
                            end + 1
                        ));
                    } else {
                        result
                            .push_str(&format!("\n\n(End of file — {} lines total)", lines.len()));
                    }
                    result
                }
                Err(error) => format!("Error reading file: {error}"),
            },
            Err(error) => format!("Error: {error}"),
        }
    }
}

pub struct WriteFileTool {
    workspace: PathBuf,
    restrict: bool,
}

impl WriteFileTool {
    pub fn new(workspace: PathBuf, restrict: bool) -> Self {
        Self {
            workspace,
            restrict,
        }
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &'static str {
        "write_file"
    }
    fn description(&self) -> &'static str {
        "Write content to a file."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"}
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: Value) -> String {
        let path = args.get("path").and_then(Value::as_str).unwrap_or_default();
        let content = args
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match resolve_path(path, &self.workspace, self.restrict) {
            Ok(path) => {
                if let Some(parent) = path.parent() {
                    if let Err(error) = std::fs::create_dir_all(parent) {
                        return format!("Error writing file: {error}");
                    }
                }
                match std::fs::write(&path, content) {
                    Ok(_) => format!(
                        "Successfully wrote {} bytes to {}",
                        content.len(),
                        path.display()
                    ),
                    Err(error) => format!("Error writing file: {error}"),
                }
            }
            Err(error) => format!("Error: {error}"),
        }
    }
}

fn find_match(content: &str, old_text: &str) -> Option<(String, usize)> {
    if old_text.is_empty() {
        return Some((String::new(), content.matches(old_text).count()));
    }
    if content.contains(old_text) {
        return Some((old_text.to_string(), content.matches(old_text).count()));
    }
    let old_lines = old_text.lines().collect::<Vec<_>>();
    if old_lines.is_empty() {
        return None;
    }
    let stripped_old = old_lines.iter().map(|line| line.trim()).collect::<Vec<_>>();
    let content_lines = content.lines().collect::<Vec<_>>();
    let mut matches = Vec::new();
    if content_lines.len() >= stripped_old.len() {
        for start in 0..=(content_lines.len() - stripped_old.len()) {
            let window = &content_lines[start..start + stripped_old.len()];
            let stripped_window = window.iter().map(|line| line.trim()).collect::<Vec<_>>();
            if stripped_window == stripped_old {
                matches.push(window.join("\n"));
            }
        }
    }
    matches.into_iter().next().map(|m| {
        let count = content.matches(&m).count();
        (m, count)
    })
}

pub struct EditFileTool {
    workspace: PathBuf,
    restrict: bool,
}

impl EditFileTool {
    pub fn new(workspace: PathBuf, restrict: bool) -> Self {
        Self {
            workspace,
            restrict,
        }
    }
}

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &'static str {
        "edit_file"
    }
    fn description(&self) -> &'static str {
        "Edit a file by replacing old_text with new_text."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "old_text": {"type": "string"},
                "new_text": {"type": "string"},
                "replace_all": {"type": "boolean"}
            },
            "required": ["path", "old_text", "new_text"]
        })
    }

    async fn execute(&self, args: Value) -> String {
        let path = args.get("path").and_then(Value::as_str).unwrap_or_default();
        let old_text = args
            .get("old_text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let new_text = args
            .get("new_text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let replace_all = args
            .get("replace_all")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let resolved = match resolve_path(path, &self.workspace, self.restrict) {
            Ok(path) => path,
            Err(error) => return format!("Error: {error}"),
        };
        let raw = match std::fs::read(&resolved) {
            Ok(raw) => raw,
            Err(error) => return format!("Error reading file: {error}"),
        };
        let uses_crlf = raw.windows(2).any(|window| window == b"\r\n");
        let content = String::from_utf8_lossy(&raw).replace("\r\n", "\n");
        let old_text = old_text.replace("\r\n", "\n");
        let Some((matched, count)) = find_match(&content, &old_text) else {
            let best = content
                .lines()
                .take(old_text.lines().count().max(1))
                .collect::<Vec<_>>()
                .join("\n");
            let diff = TextDiff::from_lines(old_text.as_str(), best.as_str())
                .unified_diff()
                .header("old_text", path)
                .to_string();
            return if best.is_empty() {
                format!("Error: old_text not found in {path}. No similar text found.")
            } else {
                format!("Error: old_text not found in {path}.\n{diff}")
            };
        };
        if count > 1 && !replace_all {
            return format!(
                "Warning: old_text appears {count} times. Provide more context or set replace_all=true."
            );
        }
        let mut updated = if replace_all {
            content.replace(&matched, new_text)
        } else {
            content.replacen(&matched, new_text, 1)
        };
        if uses_crlf {
            updated = updated.replace('\n', "\r\n");
        }
        match std::fs::write(&resolved, updated) {
            Ok(_) => format!("Successfully edited {}", resolved.display()),
            Err(error) => format!("Error editing file: {error}"),
        }
    }
}

pub struct ListDirTool {
    workspace: PathBuf,
    restrict: bool,
}

impl ListDirTool {
    pub fn new(workspace: PathBuf, restrict: bool) -> Self {
        Self {
            workspace,
            restrict,
        }
    }
}

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &'static str {
        "list_dir"
    }
    fn description(&self) -> &'static str {
        "List directory contents, optionally recursively."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "recursive": {"type": "boolean"},
                "max_entries": {"type": "integer", "minimum": 1}
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value) -> String {
        const IGNORED: &[&str] = &[
            ".git",
            "node_modules",
            "__pycache__",
            ".venv",
            "venv",
            "dist",
            "build",
            "target",
        ];
        let path = args.get("path").and_then(Value::as_str).unwrap_or_default();
        let recursive = args
            .get("recursive")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let max_entries = args
            .get("max_entries")
            .and_then(Value::as_u64)
            .unwrap_or(200) as usize;
        let resolved = match resolve_path(path, &self.workspace, self.restrict) {
            Ok(path) => path,
            Err(error) => return format!("Error: {error}"),
        };
        if !resolved.exists() {
            return format!("Error: Directory not found: {}", resolved.display());
        }
        let mut entries = Vec::new();
        let walker = if recursive {
            WalkDir::new(&resolved)
                .into_iter()
                .filter_entry(|entry| !IGNORED.iter().any(|ignored| entry.file_name() == *ignored))
                .collect::<Vec<_>>()
        } else {
            match std::fs::read_dir(&resolved) {
                Ok(read_dir) => {
                    for item in read_dir.flatten() {
                        let name = item.file_name().to_string_lossy().to_string();
                        if !IGNORED.iter().any(|ignored| name == *ignored) {
                            entries.push(name);
                        }
                    }
                    if entries.is_empty() {
                        return format!("Directory is empty: {}", resolved.display());
                    }
                    entries.sort();
                    if entries.len() > max_entries {
                        let mut sliced = entries[..max_entries].to_vec();
                        sliced.push(format!(
                            "... truncated (showing {max_entries} of {} entries)",
                            entries.len()
                        ));
                        return sliced.join("\n");
                    }
                    return entries.join("\n");
                }
                Err(error) => return format!("Error listing directory: {error}"),
            }
        };
        let mut display = Vec::new();
        for entry in walker.into_iter().flatten() {
            if entry.path() == resolved {
                continue;
            }
            if let Ok(relative) = entry.path().strip_prefix(&resolved) {
                let text = relative.display().to_string();
                if !text.is_empty() {
                    display.push(text);
                }
            }
        }
        display.sort();
        if display.is_empty() {
            return format!("Directory is empty: {}", resolved.display());
        }
        if display.len() > max_entries {
            let total = display.len();
            display.truncate(max_entries);
            display.push(format!(
                "... truncated (showing {max_entries} of {total} entries)"
            ));
        }
        display.join("\n")
    }
}

pub struct ExecTool {
    working_dir: PathBuf,
    timeout: u64,
    restrict: bool,
}

impl ExecTool {
    pub fn new(working_dir: PathBuf, timeout: u64, restrict: bool) -> Self {
        Self {
            working_dir,
            timeout,
            restrict,
        }
    }

    fn guard_command(&self, command: &str, cwd: &Path) -> Option<String> {
        let lower = command.to_lowercase();
        let deny_patterns = [
            r"\brm\s+-[rf]{1,2}\b",
            r"\bshutdown\b",
            r"\breboot\b",
            r"\bdd\s+if=",
            r">\s*/dev/sd",
        ];
        for pattern in deny_patterns {
            if Regex::new(pattern).expect("valid regex").is_match(&lower) {
                return Some(
                    "Error: Command blocked by safety guard (dangerous pattern detected)"
                        .to_string(),
                );
            }
        }
        if contains_internal_url(command) {
            return Some(
                "Error: Command blocked by safety guard (internal/private URL detected)"
                    .to_string(),
            );
        }
        if self.restrict {
            for path in extract_absolute_paths(command) {
                let resolved = PathBuf::from(path.replace(
                    '~',
                    &dirs::home_dir().unwrap_or_default().display().to_string(),
                ));
                if resolved.is_absolute() && !resolved.starts_with(cwd) {
                    return Some(
                        "Error: Command blocked by safety guard (path outside working dir)"
                            .to_string(),
                    );
                }
            }
            if command.contains("../") || command.contains("..\\") {
                return Some(
                    "Error: Command blocked by safety guard (path traversal detected)".to_string(),
                );
            }
        }
        None
    }
}

fn extract_absolute_paths(command: &str) -> Vec<String> {
    let posix = Regex::new(r#"(?:(?<=\s)|^)(/[^\s"'>;|<]+)"#).expect("valid regex");
    let home = Regex::new(r#"(?:(?<=\s)|^)(~[^\s"'>;|<]+)"#).expect("valid regex");
    posix
        .captures_iter(command)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .chain(
            home.captures_iter(command)
                .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string())),
        )
        .collect()
}

#[async_trait]
impl Tool for ExecTool {
    fn name(&self) -> &'static str {
        "exec"
    }
    fn description(&self) -> &'static str {
        "Execute a shell command and return its output."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "working_dir": {"type": "string"},
                "timeout": {"type": "integer", "minimum": 1, "maximum": 600}
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: Value) -> String {
        let command = args
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let timeout = args
            .get("timeout")
            .and_then(Value::as_u64)
            .unwrap_or(self.timeout)
            .min(600);
        let working_dir = args
            .get("working_dir")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_else(|| self.working_dir.clone());
        if let Some(error) = self.guard_command(command, &working_dir) {
            return error;
        }
        let child = match Command::new("sh")
            .arg("-lc")
            .arg(command)
            .current_dir(&working_dir)
            .kill_on_drop(true)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => return format!("Error executing command: {error}"),
        };
        match tokio::time::timeout(
            std::time::Duration::from_secs(timeout),
            child.wait_with_output(),
        )
        .await
        {
            Ok(Ok(output)) => {
                let mut result = String::new();
                if !output.stdout.is_empty() {
                    result.push_str(&String::from_utf8_lossy(&output.stdout));
                }
                if !output.stderr.is_empty() {
                    result.push_str("\nSTDERR:\n");
                    result.push_str(&String::from_utf8_lossy(&output.stderr));
                }
                result.push_str(&format!(
                    "\nExit code: {}",
                    output.status.code().unwrap_or(-1)
                ));
                let total_chars = result.chars().count();
                if total_chars > 10_000 {
                    let half = 5_000;
                    result = format!(
                        "{}\n\n... ({} chars truncated) ...\n\n{}",
                        truncate_chars(&result, half),
                        total_chars - 10_000,
                        take_last_chars(&result, half)
                    );
                }
                result
            }
            Ok(Err(error)) => format!("Error executing command: {error}"),
            Err(_) => format!("Error: Command timed out after {timeout} seconds"),
        }
    }
}

pub struct MessageTool {
    bus: MessageBus,
    context: Arc<Mutex<ToolContext>>,
    sent_in_turn: Arc<AtomicBool>,
    direct_replies: Arc<Mutex<Vec<String>>>,
}

impl MessageTool {
    pub fn new(bus: MessageBus) -> Self {
        Self {
            bus,
            context: Arc::new(Mutex::new(ToolContext::default())),
            sent_in_turn: Arc::new(AtomicBool::new(false)),
            direct_replies: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl Tool for MessageTool {
    fn name(&self) -> &'static str {
        "message"
    }
    fn description(&self) -> &'static str {
        "Send a message to the user."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "content": {"type": "string"},
                "channel": {"type": "string"},
                "chat_id": {"type": "string"}
            },
            "required": ["content"]
        })
    }

    async fn execute(&self, args: Value) -> String {
        let content = args
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut context = self.context.lock().await.clone();
        if let Some(channel) = args.get("channel").and_then(Value::as_str) {
            context.channel = channel.to_string();
        }
        if let Some(chat_id) = args.get("chat_id").and_then(Value::as_str) {
            context.chat_id = chat_id.to_string();
        }
        if context.channel.is_empty() || context.chat_id.is_empty() {
            return "Error: No target channel/chat specified".to_string();
        }
        if context.reply_to_caller {
            self.sent_in_turn.store(true, Ordering::SeqCst);
            self.direct_replies.lock().await.push(content.to_string());
            return "Message returned directly to caller".to_string();
        }
        let mut metadata = HashMap::new();
        if let Some(message_id) = context.message_id.clone() {
            metadata.insert("message_id".to_string(), json!(message_id));
        }
        let outbound = OutboundMessage {
            channel: context.channel.clone(),
            chat_id: context.chat_id.clone(),
            content: content.to_string(),
            metadata,
        };
        match self.bus.publish_outbound(outbound).await {
            Ok(_) => {
                self.sent_in_turn.store(true, Ordering::SeqCst);
                format!("Message sent to {}:{}", context.channel, context.chat_id)
            }
            Err(error) => format!("Error sending message: {error}"),
        }
    }

    async fn set_context(&self, context: ToolContext) {
        *self.context.lock().await = context;
    }

    async fn start_turn(&self) {
        self.sent_in_turn.store(false, Ordering::SeqCst);
        self.direct_replies.lock().await.clear();
    }

    async fn take_direct_replies(&self) -> Vec<String> {
        let mut replies = self.direct_replies.lock().await;
        std::mem::take(&mut *replies)
    }

    fn sent_in_turn(&self) -> bool {
        self.sent_in_turn.load(Ordering::SeqCst)
    }
}

pub struct SpawnTool {
    manager: SubagentManager,
    context: Arc<Mutex<ToolContext>>,
}

impl SpawnTool {
    pub fn new(manager: SubagentManager) -> Self {
        Self {
            manager,
            context: Arc::new(Mutex::new(ToolContext::default())),
        }
    }
}

#[async_trait]
impl Tool for SpawnTool {
    fn name(&self) -> &'static str {
        "spawn"
    }
    fn description(&self) -> &'static str {
        "Spawn a background subagent for a longer task."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": {"type": "string"},
                "label": {"type": "string"}
            },
            "required": ["task"]
        })
    }

    async fn execute(&self, args: Value) -> String {
        let task = args.get("task").and_then(Value::as_str).unwrap_or_default();
        let label = args.get("label").and_then(Value::as_str);
        let context = self.context.lock().await.clone();
        if let Some(request) = context.provider_request {
            self.manager
                .spawn_with_request(
                    task.to_string(),
                    label.map(str::to_string),
                    context.channel,
                    context.chat_id,
                    request,
                )
                .await
        } else {
            self.manager
                .spawn(
                    task.to_string(),
                    label.map(str::to_string),
                    context.channel,
                    context.chat_id,
                )
                .await
        }
    }

    async fn set_context(&self, context: ToolContext) {
        *self.context.lock().await = context;
    }
}

pub async fn build_default_tools(
    workspace: PathBuf,
    bus: MessageBus,
    timeout: u64,
    restrict_to_workspace: bool,
    subagent_manager: SubagentManager,
    web: WebToolsConfig,
    cron: Option<Arc<CronService>>,
) -> ToolRegistry {
    let registry = ToolRegistry::new();
    registry
        .register(ReadFileTool::new(workspace.clone(), restrict_to_workspace))
        .await;
    registry
        .register(WriteFileTool::new(workspace.clone(), restrict_to_workspace))
        .await;
    registry
        .register(EditFileTool::new(workspace.clone(), restrict_to_workspace))
        .await;
    registry
        .register(ListDirTool::new(workspace.clone(), restrict_to_workspace))
        .await;
    registry
        .register(ExecTool::new(
            workspace.clone(),
            timeout,
            restrict_to_workspace,
        ))
        .await;
    registry
        .register(WebSearchTool::new(web.search.clone()))
        .await;
    registry.register(WebFetchTool::new(web.fetch)).await;
    registry.register(MessageTool::new(bus)).await;
    registry.register(SpawnTool::new(subagent_manager)).await;
    if let Some(cron) = cron {
        registry.register(CronTool::new(cron)).await;
    }
    registry
}

pub fn assistant_message(content: Option<String>, tool_calls: Vec<Value>) -> Value {
    assistant_message_with_extra(content, tool_calls, serde_json::Map::new())
}

pub fn assistant_message_with_extra(
    content: Option<String>,
    tool_calls: Vec<Value>,
    extra: serde_json::Map<String, Value>,
) -> Value {
    let mut message = extra;
    message.insert("role".to_string(), json!("assistant"));
    message.insert("content".to_string(), json!(content));
    message.insert(
        "tool_calls".to_string(),
        if tool_calls.is_empty() {
            Value::Null
        } else {
            Value::Array(tool_calls)
        },
    );
    message.insert("timestamp".to_string(), json!(Utc::now()));
    Value::Object(message)
}

pub fn tool_message(tool_call_id: &str, name: &str, content: &str) -> Value {
    json!({
        "role": "tool",
        "tool_call_id": tool_call_id,
        "name": name,
        "content": content,
        "timestamp": Utc::now(),
    })
}

pub fn user_message(content: &str) -> Value {
    json!({
        "role": "user",
        "content": content,
        "timestamp": Utc::now(),
    })
}

pub fn system_message(content: &str) -> Value {
    json!({
        "role": "system",
        "content": content,
    })
}

// ---------------------------------------------------------------------------
// CronTool — lets the agent manage scheduled jobs
// ---------------------------------------------------------------------------

/// Tool that exposes add / list / remove cron job operations to the agent.
pub struct CronTool {
    cron: Arc<CronService>,
    context: Mutex<ToolContext>,
}

impl CronTool {
    pub fn new(cron: Arc<CronService>) -> Self {
        Self {
            cron,
            context: Mutex::new(ToolContext::default()),
        }
    }

    fn is_cron_context(session_key: &str) -> bool {
        session_key.starts_with("cron:")
    }

    fn add_job(
        &self,
        message: &str,
        every_seconds: Option<i64>,
        cron_expr: Option<&str>,
        tz: Option<&str>,
        at: Option<&str>,
        channel: &str,
        chat_id: &str,
    ) -> String {
        if message.is_empty() {
            return "Error: message is required for add".to_string();
        }
        if channel.is_empty() || chat_id.is_empty() {
            return "Error: no session context (channel/chat_id)".to_string();
        }
        if tz.is_some() && cron_expr.is_none() {
            return "Error: tz can only be used with cron_expr".to_string();
        }

        let (schedule, delete_after) = if let Some(every_s) = every_seconds {
            if every_s <= 0 {
                return "Error: every_seconds must be positive".to_string();
            }
            (CronSchedule::every(every_s * 1000), false)
        } else if let Some(expr) = cron_expr {
            (CronSchedule::cron(expr, tz.map(str::to_string)), false)
        } else if let Some(at_str) = at {
            // Parse ISO datetime
            let ts_ms = parse_iso_datetime_ms(at_str);
            match ts_ms {
                Ok(ms) => (CronSchedule::at(ms), true),
                Err(e) => return format!("Error: {e}"),
            }
        } else {
            return "Error: either every_seconds, cron_expr, or at is required".to_string();
        };

        match self.cron.add_job(
            message.chars().take(30).collect::<String>(),
            schedule,
            message,
            true,
            Some(channel.to_string()),
            Some(chat_id.to_string()),
            delete_after,
        ) {
            Ok(job) => format!("Created job '{}' (id: {})", job.name, job.id),
            Err(e) => format!("Error: {e}"),
        }
    }

    fn list_jobs(&self) -> String {
        let jobs = self.cron.list_jobs(false);
        if jobs.is_empty() {
            return "No scheduled jobs.".to_string();
        }
        let mut lines = vec!["Scheduled jobs:".to_string()];
        for j in &jobs {
            let timing = format_schedule_timing(&j.schedule);
            lines.push(format!("- {} (id: {}, {})", j.name, j.id, timing));
            if let Some(last_ms) = j.state.last_run_at_ms {
                let last = chrono::DateTime::from_timestamp_millis(last_ms)
                    .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                    .unwrap_or_default();
                let status = j.state.last_status.as_deref().unwrap_or("unknown");
                let err = j
                    .state
                    .last_error
                    .as_deref()
                    .map(|e| format!(" ({e})"))
                    .unwrap_or_default();
                lines.push(format!("  Last run: {last} — {status}{err}"));
            }
            if let Some(next_ms) = j.state.next_run_at_ms {
                let next = chrono::DateTime::from_timestamp_millis(next_ms)
                    .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                    .unwrap_or_default();
                lines.push(format!("  Next run: {next}"));
            }
        }
        lines.join("\n")
    }

    fn remove_job(&self, job_id: Option<&str>) -> String {
        let Some(id) = job_id else {
            return "Error: job_id is required for remove".to_string();
        };
        if self.cron.remove_job(id) {
            format!("Removed job {id}")
        } else {
            format!("Job {id} not found")
        }
    }
}

fn format_schedule_timing(schedule: &CronSchedule) -> String {
    use crate::cron::ScheduleKind;
    match schedule.kind {
        ScheduleKind::Cron => {
            let expr = schedule.expr.as_deref().unwrap_or("");
            let tz = schedule
                .tz
                .as_deref()
                .map(|t| format!(" ({t})"))
                .unwrap_or_default();
            format!("cron: {expr}{tz}")
        }
        ScheduleKind::Every => {
            let ms = schedule.every_ms.unwrap_or(0);
            if ms % 3_600_000 == 0 {
                format!("every {}h", ms / 3_600_000)
            } else if ms % 60_000 == 0 {
                format!("every {}m", ms / 60_000)
            } else if ms % 1_000 == 0 {
                format!("every {}s", ms / 1_000)
            } else {
                format!("every {ms}ms")
            }
        }
        ScheduleKind::At => {
            let dt = schedule
                .at_ms
                .and_then(chrono::DateTime::from_timestamp_millis)
                .map(|dt: chrono::DateTime<chrono::Utc>| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                .unwrap_or_default();
            format!("at {dt}")
        }
    }
}

fn parse_iso_datetime_ms(s: &str) -> Result<i64, String> {
    // Try with timezone suffix first, then assume local time.
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.timestamp_millis());
    }
    // Try naive datetime and treat as local.
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        use chrono::TimeZone;
        let dt = chrono::Local.from_local_datetime(&ndt).earliest();
        if let Some(dt) = dt {
            return Ok(dt.timestamp_millis());
        }
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        use chrono::TimeZone;
        let dt = chrono::Local.from_local_datetime(&ndt).earliest();
        if let Some(dt) = dt {
            return Ok(dt.timestamp_millis());
        }
    }
    Err(format!(
        "invalid datetime '{s}'. Expected ISO format, e.g. '2026-03-01T09:00:00'"
    ))
}

#[async_trait]
impl Tool for CronTool {
    fn name(&self) -> &'static str {
        "cron"
    }

    fn description(&self) -> &'static str {
        "Schedule reminders and recurring tasks. Actions: add, list, remove."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["add", "list", "remove"],
                    "description": "Action to perform"
                },
                "message": {
                    "type": "string",
                    "description": "Reminder message or task instruction (required for add)"
                },
                "every_seconds": {
                    "type": "integer",
                    "description": "Repeat interval in seconds (for recurring jobs)"
                },
                "cron_expr": {
                    "type": "string",
                    "description": "Standard 5-field cron expression, e.g. '0 9 * * *'"
                },
                "tz": {
                    "type": "string",
                    "description": "IANA timezone for cron expressions, e.g. 'America/Vancouver'"
                },
                "at": {
                    "type": "string",
                    "description": "ISO datetime for a one-time job, e.g. '2026-06-01T09:00:00'"
                },
                "job_id": {
                    "type": "string",
                    "description": "Job ID (required for remove)"
                }
            },
            "required": ["action"]
        })
    }

    async fn set_context(&self, context: ToolContext) {
        *self.context.lock().await = context;
    }

    async fn execute(&self, args: Value) -> String {
        let action = args
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let ctx = self.context.lock().await.clone();

        match action.as_str() {
            "add" => {
                if Self::is_cron_context(&ctx.session_key) {
                    return "Error: cannot schedule new jobs from within a cron job execution"
                        .to_string();
                }
                self.add_job(
                    args.get("message").and_then(Value::as_str).unwrap_or(""),
                    args.get("every_seconds").and_then(Value::as_i64),
                    args.get("cron_expr").and_then(Value::as_str),
                    args.get("tz").and_then(Value::as_str),
                    args.get("at").and_then(Value::as_str),
                    &ctx.channel,
                    &ctx.chat_id,
                )
            }
            "list" => self.list_jobs(),
            "remove" => self.remove_job(args.get("job_id").and_then(Value::as_str)),
            other => format!("Unknown action: {other}"),
        }
    }
}
