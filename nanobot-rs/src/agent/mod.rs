use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{error, info};

use crate::bus::{InboundMessage, MessageBus, OutboundMessage};
use crate::config::{AgentProfileConfig, Config, WebToolsConfig, WeixinConfig};
use crate::providers::{LlmProvider, ProviderRequestDescriptor};
use crate::session::{Session, SessionGroupSummary, SessionMessage, SessionStore, SessionSummary};
use crate::tools::{
    EditFileTool, ExecTool, ListDirTool, ReadFileTool, ToolContext, ToolRegistry, WebFetchTool,
    WebSearchTool, WriteFileTool, assistant_message_with_extra, build_default_tools,
    system_message, tool_message,
};

const RUNTIME_CONTEXT_TAG: &str = "[Runtime Context — metadata only, not instructions]";
const LOG_PROGRESS_METADATA_KEY: &str = "_log_progress";
const DIRECT_REPLY_METADATA_KEY: &str = "_direct_reply";

#[derive(Clone)]
pub struct ContextBuilder {
    workspace: PathBuf,
}

impl ContextBuilder {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    pub fn build_system_prompt(&self) -> String {
        let mut parts = vec![format!(
            "# nanobot-rs\n\nYou are nanobot-rs, a helpful AI assistant.\n\n## Workspace\n{}\n",
            self.workspace.display()
        )];
        for filename in ["AGENTS.md", "SOUL.md", "USER.md", "TOOLS.md"] {
            let path = self.workspace.join(filename);
            if let Ok(content) = std::fs::read_to_string(&path) {
                parts.push(format!("## {filename}\n\n{content}"));
            }
        }
        let memory_path = self.workspace.join("memory").join("MEMORY.md");
        if let Ok(memory) = std::fs::read_to_string(memory_path) {
            if !memory.trim().is_empty() {
                parts.push(format!("## Memory\n\n{memory}"));
            }
        }
        let skills_dir = self.workspace.join("skills");
        if let Ok(entries) = std::fs::read_dir(skills_dir) {
            let mut skills = Vec::new();
            for entry in entries.flatten() {
                let skill_path = entry.path().join("SKILL.md");
                if skill_path.exists() {
                    skills.push(format!(
                        "- {} ({})",
                        entry.file_name().to_string_lossy(),
                        skill_path.display()
                    ));
                }
            }
            if !skills.is_empty() {
                parts.push(format!(
                    "## Skills\n\nThe following skills are available. Read the SKILL.md file before using one.\n{}",
                    skills.join("\n")
                ));
            }
        }
        parts.join("\n\n---\n\n")
    }

    pub fn build_messages(
        &self,
        history: Vec<Value>,
        current_message: &str,
        current_role: &str,
        channel: Option<&str>,
        chat_id: Option<&str>,
    ) -> Vec<Value> {
        let runtime = self.runtime_context(channel, chat_id);
        let merged = format!("{runtime}\n\n{current_message}");
        let mut messages = Vec::with_capacity(history.len() + 2);
        messages.push(system_message(&self.build_system_prompt()));
        messages.extend(history);
        messages.push(json!({
            "role": current_role,
            "content": merged,
        }));
        messages
    }

    pub fn runtime_context(&self, channel: Option<&str>, chat_id: Option<&str>) -> String {
        let mut lines = vec![format!("Current Time: {}", Utc::now().to_rfc3339())];
        if let (Some(channel), Some(chat_id)) = (channel, chat_id) {
            lines.push(format!("Channel: {channel}"));
            lines.push(format!("Chat ID: {chat_id}"));
        }
        format!("{RUNTIME_CONTEXT_TAG}\n{}", lines.join("\n"))
    }

    pub fn strip_runtime_prefix(content: &str) -> Option<String> {
        if !content.starts_with(RUNTIME_CONTEXT_TAG) {
            return Some(content.to_string());
        }
        let parts = content.splitn(2, "\n\n").collect::<Vec<_>>();
        if parts.len() == 2 && !parts[1].trim().is_empty() {
            Some(parts[1].to_string())
        } else {
            None
        }
    }
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

fn profile_to_request(profile: &AgentProfileConfig) -> ProviderRequestDescriptor {
    ProviderRequestDescriptor::new(
        profile.provider.clone(),
        profile.model.clone(),
        profile.request.clone(),
    )
}

#[async_trait]
trait ProgressReporter: Send + Sync {
    async fn report(&self, content: String, tool_hint: bool);
}

struct BusProgressReporter {
    bus: MessageBus,
    channel: String,
    chat_id: String,
    metadata: HashMap<String, Value>,
}

#[async_trait]
impl ProgressReporter for BusProgressReporter {
    async fn report(&self, content: String, tool_hint: bool) {
        if content.trim().is_empty() {
            return;
        }
        let mut metadata = self.metadata.clone();
        metadata.insert("_progress".to_string(), json!(true));
        metadata.insert("_tool_hint".to_string(), json!(tool_hint));
        let _ = self
            .bus
            .publish_outbound(OutboundMessage {
                channel: self.channel.clone(),
                chat_id: self.chat_id.clone(),
                content,
                metadata,
            })
            .await;
    }
}

struct LogProgressReporter {
    session_key: String,
    channel: String,
    chat_id: String,
}

#[async_trait]
impl ProgressReporter for LogProgressReporter {
    async fn report(&self, content: String, tool_hint: bool) {
        if content.trim().is_empty() {
            return;
        }
        if tool_hint {
            info!(
                session = %self.session_key,
                channel = %self.channel,
                chat_id = %self.chat_id,
                tool = %content,
                "agent tool"
            );
        } else {
            info!(
                session = %self.session_key,
                channel = %self.channel,
                chat_id = %self.chat_id,
                progress = %content,
                "agent progress"
            );
        }
    }
}

#[derive(Clone)]
pub struct SubagentManager {
    provider: Arc<dyn LlmProvider>,
    workspace: PathBuf,
    bus: MessageBus,
    default_request: ProviderRequestDescriptor,
    max_iterations: usize,
    exec_timeout: u64,
    restrict_to_workspace: bool,
    web_tools: WebToolsConfig,
    running_tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    session_tasks: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

impl SubagentManager {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        workspace: PathBuf,
        bus: MessageBus,
        model: String,
        max_iterations: usize,
        exec_timeout: u64,
        restrict_to_workspace: bool,
        web_tools: WebToolsConfig,
    ) -> Self {
        let request = ProviderRequestDescriptor::new("openai", model, serde_json::Map::new());
        Self::new_with_request(
            provider,
            workspace,
            bus,
            request,
            max_iterations,
            exec_timeout,
            restrict_to_workspace,
            web_tools,
        )
    }

    pub fn new_with_request(
        provider: Arc<dyn LlmProvider>,
        workspace: PathBuf,
        bus: MessageBus,
        default_request: ProviderRequestDescriptor,
        max_iterations: usize,
        exec_timeout: u64,
        restrict_to_workspace: bool,
        web_tools: WebToolsConfig,
    ) -> Self {
        Self {
            provider,
            workspace,
            bus,
            default_request,
            max_iterations,
            exec_timeout,
            restrict_to_workspace,
            web_tools,
            running_tasks: Arc::new(Mutex::new(HashMap::new())),
            session_tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn spawn(
        &self,
        task: String,
        label: Option<String>,
        origin_channel: String,
        origin_chat_id: String,
    ) -> String {
        self.spawn_with_request(
            task,
            label,
            origin_channel,
            origin_chat_id,
            self.default_request.clone(),
        )
        .await
    }

    pub async fn spawn_with_request(
        &self,
        task: String,
        label: Option<String>,
        origin_channel: String,
        origin_chat_id: String,
        request: ProviderRequestDescriptor,
    ) -> String {
        let task_id = truncate_chars(&uuid::Uuid::new_v4().to_string(), 8);
        let label = label.unwrap_or_else(|| {
            if task.chars().count() > 30 {
                format!("{}...", truncate_chars(&task, 30))
            } else {
                task.clone()
            }
        });
        let session_key = format!("{origin_channel}:{origin_chat_id}");
        let manager = self.clone();
        let task_id_for_handle = task_id.clone();
        let session_key_for_handle = session_key.clone();
        let label_for_handle = label.clone();
        let handle = tokio::spawn(async move {
            manager
                .run_subagent(
                    task_id_for_handle.clone(),
                    task.clone(),
                    label_for_handle.clone(),
                    origin_channel.clone(),
                    origin_chat_id.clone(),
                    request,
                )
                .await;
            manager
                .running_tasks
                .lock()
                .await
                .remove(&task_id_for_handle);
            if let Some(ids) = manager
                .session_tasks
                .lock()
                .await
                .get_mut(&session_key_for_handle)
            {
                ids.retain(|id| id != &task_id_for_handle);
            }
        });
        self.running_tasks
            .lock()
            .await
            .insert(task_id.clone(), handle);
        self.session_tasks
            .lock()
            .await
            .entry(session_key)
            .or_default()
            .push(task_id.clone());
        format!("Subagent [{label}] started (id: {task_id}). I'll notify you when it completes.")
    }

    async fn run_subagent(
        &self,
        task_id: String,
        task: String,
        label: String,
        origin_channel: String,
        origin_chat_id: String,
        request: ProviderRequestDescriptor,
    ) {
        let tools = ToolRegistry::new();
        tools
            .register(ReadFileTool::new(
                self.workspace.clone(),
                self.restrict_to_workspace,
            ))
            .await;
        tools
            .register(WriteFileTool::new(
                self.workspace.clone(),
                self.restrict_to_workspace,
            ))
            .await;
        tools
            .register(EditFileTool::new(
                self.workspace.clone(),
                self.restrict_to_workspace,
            ))
            .await;
        tools
            .register(ListDirTool::new(
                self.workspace.clone(),
                self.restrict_to_workspace,
            ))
            .await;
        tools
            .register(ExecTool::new(
                self.workspace.clone(),
                self.exec_timeout,
                self.restrict_to_workspace,
            ))
            .await;
        tools
            .register(WebSearchTool::new(self.web_tools.search.clone()))
            .await;
        tools
            .register(WebFetchTool::new(self.web_tools.fetch.clone()))
            .await;
        let system_prompt = format!(
            "# Subagent\n\nYou are a background subagent. Focus only on the assigned task.\nWorkspace: {}",
            self.workspace.display()
        );
        let mut messages = vec![
            system_message(&system_prompt),
            json!({"role": "user", "content": task}),
        ];
        let mut final_content = None;
        for _ in 0..self.max_iterations.min(15) {
            let defs = tools.definitions().await;
            match self
                .provider
                .chat_with_request_retry(messages.clone(), defs, &request)
                .await
            {
                Ok(response) => {
                    if response.has_tool_calls() {
                        let tool_calls = response
                            .tool_calls
                            .iter()
                            .map(|call| call.to_openai_tool_call())
                            .collect::<Vec<_>>();
                        messages.push(assistant_message_with_extra(
                            response.content.clone(),
                            tool_calls,
                            response.extra.clone(),
                        ));
                        for tool_call in response.tool_calls {
                            let result = tools.execute(&tool_call.name, tool_call.arguments).await;
                            messages.push(tool_message(&tool_call.id, &tool_call.name, &result));
                        }
                    } else {
                        final_content = response.content;
                        break;
                    }
                }
                Err(error) => {
                    final_content = Some(format!("Error: {error}"));
                    break;
                }
            }
        }
        let final_content = final_content
            .unwrap_or_else(|| "Task completed but no final response was generated.".to_string());
        let content = format!(
            "[Subagent '{label}' completed]\n\nTask: {task}\n\nResult:\n{final_content}\n\nSummarize this naturally for the user. Keep it brief and do not mention internal task IDs."
        );
        let _ = self
            .bus
            .publish_inbound(InboundMessage {
                channel: "system".to_string(),
                sender_id: "subagent".to_string(),
                chat_id: format!("{origin_channel}:{origin_chat_id}"),
                content,
                timestamp: Utc::now(),
                metadata: HashMap::new(),
                session_key_override: None,
            })
            .await;
        info!("subagent {task_id} finished");
    }

    pub async fn cancel_by_session(&self, session_key: &str) -> usize {
        let ids = self
            .session_tasks
            .lock()
            .await
            .get(session_key)
            .cloned()
            .unwrap_or_default();
        let mut cancelled = 0;
        let mut running = self.running_tasks.lock().await;
        for id in ids {
            if let Some(handle) = running.remove(&id) {
                handle.abort();
                cancelled += 1;
            }
        }
        cancelled
    }
}

pub struct AgentLoop {
    bus: MessageBus,
    provider: Arc<dyn LlmProvider>,
    workspace: PathBuf,
    default_profile: String,
    profiles: HashMap<String, AgentProfileConfig>,
    max_iterations: usize,
    exec_timeout: u64,
    restrict_to_workspace: bool,
    web_tools: WebToolsConfig,
    weixin_web: WeixinConfig,
    sessions: SessionStore,
    context: ContextBuilder,
    subagents: SubagentManager,
    processing_lock: Arc<Mutex<()>>,
    active_tasks: Arc<Mutex<HashMap<String, Vec<JoinHandle<()>>>>>,
    running: Arc<AtomicBool>,
}

impl AgentLoop {
    pub async fn new(
        bus: MessageBus,
        provider: Arc<dyn LlmProvider>,
        workspace: PathBuf,
        model: String,
        max_iterations: usize,
        exec_timeout: u64,
        restrict_to_workspace: bool,
        web_tools: WebToolsConfig,
    ) -> Result<Self> {
        let default_profile = format!("openai:{model}");
        let profiles = HashMap::from([(
            default_profile.clone(),
            AgentProfileConfig {
                provider: "openai".to_string(),
                model: model.clone(),
                request: serde_json::Map::new(),
            },
        )]);
        let weixin_web = WeixinConfig::default();
        Self::new_internal(
            bus,
            provider,
            workspace,
            default_profile,
            profiles,
            max_iterations,
            exec_timeout,
            restrict_to_workspace,
            web_tools,
            weixin_web,
        )
        .await
    }

    pub async fn from_config(
        bus: MessageBus,
        provider: Arc<dyn LlmProvider>,
        config: Config,
    ) -> Result<Self> {
        let workspace = config.workspace_path();
        Self::new_internal(
            bus,
            provider,
            workspace,
            config.agents.defaults.default_profile.clone(),
            config.agents.profiles.clone(),
            config.agents.defaults.max_tool_iterations,
            config.tools.exec.timeout,
            config.tools.restrict_to_workspace,
            config.tools.web.clone(),
            config.channels.weixin.clone(),
        )
        .await
    }

    async fn new_internal(
        bus: MessageBus,
        provider: Arc<dyn LlmProvider>,
        workspace: PathBuf,
        default_profile: String,
        profiles: HashMap<String, AgentProfileConfig>,
        max_iterations: usize,
        exec_timeout: u64,
        restrict_to_workspace: bool,
        web_tools: WebToolsConfig,
        weixin_web: WeixinConfig,
    ) -> Result<Self> {
        let sessions = SessionStore::new(&workspace)?;
        let context = ContextBuilder::new(workspace.clone());
        let default_request =
            profile_to_request(profiles.get(&default_profile).ok_or_else(|| {
                anyhow::anyhow!("default profile '{default_profile}' is missing")
            })?);
        let subagents = SubagentManager::new_with_request(
            provider.clone(),
            workspace.clone(),
            bus.clone(),
            default_request,
            max_iterations,
            exec_timeout,
            restrict_to_workspace,
            web_tools.clone(),
        );
        Ok(Self {
            bus,
            provider,
            workspace,
            default_profile,
            profiles,
            max_iterations,
            exec_timeout,
            restrict_to_workspace,
            web_tools,
            weixin_web,
            sessions,
            context,
            subagents,
            processing_lock: Arc::new(Mutex::new(())),
            active_tasks: Arc::new(Mutex::new(HashMap::new())),
            running: Arc::new(AtomicBool::new(false)),
        })
    }

    pub async fn run(&self) {
        self.running.store(true, Ordering::SeqCst);
        while self.running.load(Ordering::SeqCst) {
            let Some(msg) = self.bus.consume_inbound().await else {
                continue;
            };
            let session_key = msg.session_key();
            if msg.content.trim().eq_ignore_ascii_case("/stop") {
                self.handle_stop(&msg).await;
                continue;
            }
            let this = self.clone();
            let handle = tokio::spawn(async move {
                if let Err(error) = this.dispatch(msg).await {
                    error!("agent dispatch failed: {error}");
                }
            });
            self.active_tasks
                .lock()
                .await
                .entry(session_key)
                .or_default()
                .push(handle);
        }
    }

    async fn dispatch(&self, msg: InboundMessage) -> Result<()> {
        let _guard = self.processing_lock.lock().await;
        let tools = self.build_tools().await;
        if let Some(outbound) = self.process_message(msg, &tools).await? {
            self.bus.publish_outbound(outbound).await?;
        }
        Ok(())
    }

    async fn handle_stop(&self, msg: &InboundMessage) {
        let session_key = msg.session_key();
        let tasks = self
            .active_tasks
            .lock()
            .await
            .remove(&session_key)
            .unwrap_or_default();
        let mut cancelled = 0usize;
        for handle in tasks {
            handle.abort();
            cancelled += 1;
        }
        cancelled += self.subagents.cancel_by_session(&session_key).await;
        let content = if cancelled == 0 {
            "No active task to stop.".to_string()
        } else {
            format!("Stopped {cancelled} task(s).")
        };
        let _ = self
            .bus
            .publish_outbound(OutboundMessage {
                channel: msg.channel.clone(),
                chat_id: msg.chat_id.clone(),
                content,
                metadata: HashMap::new(),
            })
            .await;
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    fn normalize_session_profile<'a>(&'a self, session: &'a mut Session) -> &'a str {
        let selected = session
            .active_profile
            .as_deref()
            .filter(|key| self.profiles.contains_key(*key))
            .unwrap_or(&self.default_profile)
            .to_string();
        if session.active_profile.as_deref() != Some(selected.as_str()) {
            session.active_profile = Some(selected.clone());
        }
        session
            .active_profile
            .as_deref()
            .unwrap_or(&self.default_profile)
    }

    fn resolve_request(&self, session: &mut Session) -> Result<ProviderRequestDescriptor> {
        let key = self.normalize_session_profile(session).to_string();
        let profile = self
            .profiles
            .get(&key)
            .ok_or_else(|| anyhow!("session profile '{key}' is not configured"))?;
        Ok(profile_to_request(profile))
    }

    fn help_text(&self) -> String {
        "nanobot-rs commands:\n/new — Start a new conversation\n/stop — Stop the current task\n/help — Show available commands\n/models — List available model profiles\n/model <provider:model> — Switch the current session model".to_string()
    }

    fn models_text(&self, current_profile: &str) -> String {
        let mut profiles = self.profiles.keys().cloned().collect::<Vec<_>>();
        profiles.sort();
        let mut lines = vec!["Available model profiles:".to_string()];
        for profile in profiles {
            let marker = if profile == current_profile { "*" } else { " " };
            lines.push(format!("{marker} {profile}"));
        }
        lines.join("\n")
    }

    pub fn default_profile(&self) -> &str {
        &self.default_profile
    }

    pub fn workspace_path(&self) -> &Path {
        &self.workspace
    }

    pub fn weixin_web_config(&self) -> &WeixinConfig {
        &self.weixin_web
    }

    pub fn has_profile(&self, key: &str) -> bool {
        self.profiles.contains_key(key)
    }

    pub fn current_profile_for_session(&self, session_key: &str) -> Result<String> {
        let mut session = self
            .sessions
            .get_or_create_with_default_profile(session_key, &self.default_profile)?;
        Ok(self.normalize_session_profile(&mut session).to_string())
    }

    pub fn list_sessions_in_namespace(&self, namespace: &str) -> Result<Vec<SessionSummary>> {
        self.sessions.list_sessions_in_namespace(namespace)
    }

    pub fn list_sessions_grouped_by_channel(&self) -> Result<Vec<SessionGroupSummary>> {
        self.sessions.list_sessions_grouped_by_channel()
    }

    pub fn load_session(&self, session_key: &str) -> Result<Option<Session>> {
        let Some(mut session) = self.sessions.load(session_key)? else {
            return Ok(None);
        };
        self.normalize_session_profile(&mut session);
        Ok(Some(session))
    }

    pub fn load_session_by_key(&self, session_key: &str) -> Result<Option<Session>> {
        self.load_session(session_key)
    }

    pub fn create_session(&self, session_key: &str) -> Result<Session> {
        let mut session = self
            .sessions
            .get_or_create_with_default_profile(session_key, &self.default_profile)?;
        self.normalize_session_profile(&mut session);
        self.sessions.save(&session)?;
        Ok(session)
    }

    pub fn duplicate_session_to_web(&self, source_key: &str) -> Result<Session> {
        let mut session = self.sessions.duplicate_session_to_web(source_key)?;
        self.normalize_session_profile(&mut session);
        self.sessions.save(&session)?;
        Ok(session)
    }

    pub async fn process_direct(
        &self,
        content: &str,
        session_key: &str,
        channel: &str,
        chat_id: &str,
    ) -> Result<String> {
        self.process_direct_internal(content, session_key, channel, chat_id, false)
            .await
    }

    pub async fn process_direct_logged(
        &self,
        content: &str,
        session_key: &str,
        channel: &str,
        chat_id: &str,
    ) -> Result<String> {
        self.process_direct_internal(content, session_key, channel, chat_id, true)
            .await
    }

    async fn process_direct_internal(
        &self,
        content: &str,
        session_key: &str,
        channel: &str,
        chat_id: &str,
        log_progress: bool,
    ) -> Result<String> {
        let mut metadata = HashMap::new();
        metadata.insert(DIRECT_REPLY_METADATA_KEY.to_string(), json!(true));
        if log_progress {
            metadata.insert(LOG_PROGRESS_METADATA_KEY.to_string(), json!(true));
        }
        let msg = InboundMessage {
            channel: channel.to_string(),
            sender_id: "user".to_string(),
            chat_id: chat_id.to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            metadata,
            session_key_override: Some(session_key.to_string()),
        };
        let tools = self.build_tools().await;
        let outbound = self.process_message(msg, &tools).await?;
        if let Some(outbound) = outbound {
            return Ok(outbound.content);
        }
        let direct_replies = tools.take_direct_replies().await;
        Ok(direct_replies.join("\n\n"))
    }

    async fn process_message(
        &self,
        msg: InboundMessage,
        tools: &ToolRegistry,
    ) -> Result<Option<OutboundMessage>> {
        if msg.channel == "system" {
            let (channel, chat_id) = msg
                .chat_id
                .split_once(':')
                .map(|(channel, chat_id)| (channel.to_string(), chat_id.to_string()))
                .unwrap_or_else(|| ("cli".to_string(), msg.chat_id.clone()));
            let mut session = self.sessions.get_or_create_with_default_profile(
                &format!("{channel}:{chat_id}"),
                &self.default_profile,
            )?;
            let request = self.resolve_request(&mut session)?;
            tools
                .set_context(ToolContext {
                    channel: channel.clone(),
                    chat_id: chat_id.clone(),
                    message_id: None,
                    reply_to_caller: false,
                    provider_request: Some(request.clone()),
                })
                .await;
            let history = session.get_history(0);
            let messages = self.context.build_messages(
                history,
                &msg.content,
                if msg.sender_id == "subagent" {
                    "assistant"
                } else {
                    "user"
                },
                Some(&channel),
                Some(&chat_id),
            );
            let (final_content, all_messages) =
                self.run_agent_loop(messages, None, tools, &request).await?;
            self.save_turn(&mut session, all_messages, 1)?;
            self.sessions.save(&session)?;
            return Ok(Some(OutboundMessage {
                channel,
                chat_id,
                content: final_content.unwrap_or_else(|| "Background task completed.".to_string()),
                metadata: HashMap::new(),
            }));
        }

        let session_key = msg.session_key();
        let mut session = self
            .sessions
            .get_or_create_with_default_profile(&session_key, &self.default_profile)?;
        let current_profile = self.normalize_session_profile(&mut session).to_string();
        match msg.content.trim() {
            "/new" => {
                session.clear();
                session.active_profile = Some(self.default_profile.clone());
                self.sessions.save(&session)?;
                return Ok(Some(OutboundMessage {
                    channel: msg.channel,
                    chat_id: msg.chat_id,
                    content: "New session started.".to_string(),
                    metadata: HashMap::new(),
                }));
            }
            "/help" => {
                return Ok(Some(OutboundMessage {
                    channel: msg.channel,
                    chat_id: msg.chat_id,
                    content: self.help_text(),
                    metadata: HashMap::new(),
                }));
            }
            "/models" => {
                return Ok(Some(OutboundMessage {
                    channel: msg.channel,
                    chat_id: msg.chat_id,
                    content: self.models_text(&current_profile),
                    metadata: HashMap::new(),
                }));
            }
            command if command.starts_with("/model ") => {
                let requested = command.trim_start_matches("/model").trim();
                if !self.profiles.contains_key(requested) {
                    return Ok(Some(OutboundMessage {
                        channel: msg.channel,
                        chat_id: msg.chat_id,
                        content: format!("Unknown model profile: {requested}"),
                        metadata: HashMap::new(),
                    }));
                }
                session.active_profile = Some(requested.to_string());
                self.sessions.save(&session)?;
                return Ok(Some(OutboundMessage {
                    channel: msg.channel,
                    chat_id: msg.chat_id,
                    content: format!("Switched this session to {requested}."),
                    metadata: HashMap::new(),
                }));
            }
            _ => {}
        }

        let request = self.resolve_request(&mut session)?;

        tools
            .set_context(ToolContext {
                channel: msg.channel.clone(),
                chat_id: msg.chat_id.clone(),
                message_id: msg
                    .metadata
                    .get("message_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                reply_to_caller: msg
                    .metadata
                    .get(DIRECT_REPLY_METADATA_KEY)
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                provider_request: Some(request.clone()),
            })
            .await;
        tools.start_turn().await;

        let history = session.get_history(0);
        let messages = self.context.build_messages(
            history.clone(),
            &msg.content,
            "user",
            Some(&msg.channel),
            Some(&msg.chat_id),
        );
        let log_progress = msg
            .metadata
            .get(LOG_PROGRESS_METADATA_KEY)
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let direct_reply = msg
            .metadata
            .get(DIRECT_REPLY_METADATA_KEY)
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let reporter: Option<Arc<dyn ProgressReporter>> = if log_progress {
            Some(Arc::new(LogProgressReporter {
                session_key: session_key.clone(),
                channel: msg.channel.clone(),
                chat_id: msg.chat_id.clone(),
            }))
        } else if direct_reply {
            None
        } else {
            Some(Arc::new(BusProgressReporter {
                bus: self.bus.clone(),
                channel: msg.channel.clone(),
                chat_id: msg.chat_id.clone(),
                metadata: msg.metadata.clone(),
            }))
        };
        let (final_content, all_messages) = self
            .run_agent_loop(messages, reporter, tools, &request)
            .await?;
        self.save_turn(&mut session, all_messages, 1 + history.len())?;
        self.sessions.save(&session)?;

        if tools.sent_message_this_turn().await {
            return Ok(None);
        }

        Ok(Some(OutboundMessage {
            channel: msg.channel,
            chat_id: msg.chat_id,
            content: final_content.unwrap_or_else(|| {
                "I've completed processing but have no response to give.".to_string()
            }),
            metadata: msg.metadata,
        }))
    }

    async fn run_agent_loop(
        &self,
        initial_messages: Vec<Value>,
        reporter: Option<Arc<dyn ProgressReporter>>,
        tools: &ToolRegistry,
        request: &ProviderRequestDescriptor,
    ) -> Result<(Option<String>, Vec<Value>)> {
        let mut messages = initial_messages;
        let mut final_content = None;
        for _ in 0..self.max_iterations {
            let defs = tools.definitions().await;
            let response = self
                .provider
                .chat_with_request_retry(messages.clone(), defs, request)
                .await?;
            if response.has_tool_calls() {
                if let Some(reporter) = &reporter {
                    if let Some(content) = response.content.clone() {
                        reporter.report(content, false).await;
                    }
                    let hint = response
                        .tool_calls
                        .iter()
                        .map(tool_hint)
                        .collect::<Vec<_>>()
                        .join(", ");
                    if !hint.is_empty() {
                        reporter.report(hint, true).await;
                    }
                }
                let tool_calls = response
                    .tool_calls
                    .iter()
                    .map(|call| call.to_openai_tool_call())
                    .collect::<Vec<_>>();
                messages.push(assistant_message_with_extra(
                    response.content.clone(),
                    tool_calls,
                    response.extra.clone(),
                ));
                for tool_call in response.tool_calls {
                    let result = tools.execute(&tool_call.name, tool_call.arguments).await;
                    messages.push(tool_message(&tool_call.id, &tool_call.name, &result));
                }
            } else {
                final_content = response.content.clone();
                messages.push(assistant_message_with_extra(
                    response.content,
                    Vec::new(),
                    response.extra,
                ));
                break;
            }
        }
        if final_content.is_none() {
            final_content = Some(format!(
                "I reached the maximum number of tool call iterations ({}) without completing the task.",
                self.max_iterations
            ));
        }
        Ok((final_content, messages))
    }

    async fn build_tools(&self) -> ToolRegistry {
        build_default_tools(
            self.workspace.clone(),
            self.bus.clone(),
            self.exec_timeout,
            self.restrict_to_workspace,
            self.subagents.clone(),
            self.web_tools.clone(),
        )
        .await
    }

    fn save_turn(&self, session: &mut Session, messages: Vec<Value>, skip: usize) -> Result<()> {
        for value in messages.into_iter().skip(skip) {
            let mut message: SessionMessage = serde_json::from_value(value)?;
            if message.role == "assistant"
                && message.content.is_null()
                && message.tool_calls.is_none()
            {
                continue;
            }
            if message.role == "user" {
                if let Some(content) = message.content.as_str() {
                    match ContextBuilder::strip_runtime_prefix(content) {
                        Some(stripped) => message.content = json!(stripped),
                        None => continue,
                    }
                }
            }
            message.timestamp.get_or_insert_with(Utc::now);
            session.messages.push(message);
        }
        session.updated_at = Utc::now();
        Ok(())
    }
}

impl Clone for AgentLoop {
    fn clone(&self) -> Self {
        Self {
            bus: self.bus.clone(),
            provider: self.provider.clone(),
            workspace: self.workspace.clone(),
            default_profile: self.default_profile.clone(),
            profiles: self.profiles.clone(),
            max_iterations: self.max_iterations,
            exec_timeout: self.exec_timeout,
            restrict_to_workspace: self.restrict_to_workspace,
            web_tools: self.web_tools.clone(),
            weixin_web: self.weixin_web.clone(),
            sessions: self.sessions.clone(),
            context: self.context.clone(),
            subagents: self.subagents.clone(),
            processing_lock: self.processing_lock.clone(),
            active_tasks: self.active_tasks.clone(),
            running: self.running.clone(),
        }
    }
}

fn tool_hint(call: &crate::providers::ToolCall) -> String {
    if let Some(object) = call.arguments.as_object() {
        if let Some(value) = object.values().find_map(Value::as_str) {
            if value.chars().count() > 40 {
                return format!("{}(\"{}…\")", call.name, truncate_chars(value, 40));
            }
            return format!("{}(\"{}\")", call.name, value);
        }
    }
    call.name.clone()
}
