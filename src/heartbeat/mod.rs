//! Heartbeat service — periodic agent wake-up to check for pending tasks.
//!
//! Keeps the workspace heartbeat loop behavior aligned with the previous service.
//!
//! **Phase 1 (decision):** reads `HEARTBEAT.md` from the workspace and asks
//! the LLM — via a structured tool call — whether there are active tasks.
//! This avoids fragile free-text parsing.
//!
//! **Phase 2 (execution):** only triggered when Phase 1 returns `run`.  The
//! `on_execute` callback runs the task through the full agent loop and returns
//! a response string.  If `on_notify` is set the response is forwarded there
//! (e.g. to publish on an outbound channel).

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use tracing::{debug, error, info};

use crate::providers::LlmProvider;

// ---------------------------------------------------------------------------
// Callback type aliases
// ---------------------------------------------------------------------------

type ExecuteCallback =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = String> + Send>> + Send + Sync>;

type NotifyCallback = Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

// ---------------------------------------------------------------------------
// Tool definition sent to the LLM in Phase 1
// ---------------------------------------------------------------------------

fn heartbeat_tool_spec() -> Value {
    json!([{
        "type": "function",
        "function": {
            "name": "heartbeat",
            "description": "Report heartbeat decision after reviewing tasks.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["skip", "run"],
                        "description": "skip = nothing to do, run = has active tasks"
                    },
                    "tasks": {
                        "type": "string",
                        "description": "Natural-language summary of active tasks (required for run)"
                    }
                },
                "required": ["action"]
            }
        }
    }])
}

// ---------------------------------------------------------------------------
// HeartbeatService
// ---------------------------------------------------------------------------

/// Periodic heartbeat service.
pub struct HeartbeatService {
    workspace: PathBuf,
    provider: Arc<dyn LlmProvider>,
    model: String,
    interval_s: u64,
    enabled: bool,
    on_execute: Mutex<Option<ExecuteCallback>>,
    on_notify: Mutex<Option<NotifyCallback>>,
    running: Arc<AtomicBool>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl HeartbeatService {
    pub fn new(
        workspace: PathBuf,
        provider: Arc<dyn LlmProvider>,
        model: impl Into<String>,
        interval_s: u64,
        enabled: bool,
    ) -> Self {
        Self {
            workspace,
            provider,
            model: model.into(),
            interval_s,
            enabled,
            on_execute: Mutex::new(None),
            on_notify: Mutex::new(None),
            running: Arc::new(AtomicBool::new(false)),
            task: Mutex::new(None),
        }
    }

    /// Set the Phase-2 execution callback: receives a task description,
    /// runs it through the agent loop, and returns the response text.
    pub fn set_on_execute<F, Fut>(&self, f: F)
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = String> + Send + 'static,
    {
        *self.on_execute.lock().unwrap() = Some(Arc::new(move |tasks| Box::pin(f(tasks))));
    }

    /// Set the notification callback: called when Phase 2 produces a response
    /// worth delivering to the user's channel.
    pub fn set_on_notify<F, Fut>(&self, f: F)
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        *self.on_notify.lock().unwrap() = Some(Arc::new(move |r| Box::pin(f(r))));
    }

    /// Start the background heartbeat loop.
    pub async fn start(self: &Arc<Self>) {
        if !self.enabled {
            info!("Heartbeat disabled");
            return;
        }
        if self.running.swap(true, Ordering::SeqCst) {
            return; // already running
        }
        info!("Heartbeat started (every {}s)", self.interval_s);
        let svc = Arc::clone(self);
        let handle = tokio::spawn(async move {
            svc.run_loop().await;
        });
        *self.task.lock().unwrap() = Some(handle);
    }

    /// Stop the heartbeat loop.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(task) = self.task.lock().unwrap().take() {
            task.abort();
        }
    }

    /// Manually trigger one heartbeat tick (for testing / ad-hoc use).
    pub async fn trigger_now(self: &Arc<Self>) -> Option<String> {
        let content = self.read_heartbeat_file()?;
        let (action, tasks) = self.decide(&content).await;
        if action != "run" {
            return None;
        }
        let cb = self.on_execute.lock().unwrap().clone()?;
        Some(cb(tasks).await)
    }

    // -----------------------------------------------------------------------
    // Internal
    // -----------------------------------------------------------------------

    fn heartbeat_file(&self) -> PathBuf {
        self.workspace.join("HEARTBEAT.md")
    }

    fn read_heartbeat_file(&self) -> Option<String> {
        let path = self.heartbeat_file();
        if path.exists() {
            std::fs::read_to_string(&path).ok()
        } else {
            None
        }
    }

    /// Phase 1: ask the LLM whether there are active tasks.
    /// Returns `(action, tasks)` where `action` is `"skip"` or `"run"`.
    async fn decide(&self, content: &str) -> (String, String) {
        let now = chrono::Utc::now()
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string();
        let messages = vec![
            json!({
                "role": "system",
                "content": "You are a heartbeat agent. Call the heartbeat tool to report your decision."
            }),
            json!({
                "role": "user",
                "content": format!(
                    "Current Time: {now}\n\nReview the following HEARTBEAT.md and decide whether there are active tasks.\n\n{content}"
                )
            }),
        ];

        let tools: Vec<Value> = match heartbeat_tool_spec() {
            Value::Array(arr) => arr,
            _ => vec![],
        };

        match self
            .provider
            .chat_with_retry(messages, tools, &self.model)
            .await
        {
            Ok(response) if response.has_tool_calls() => {
                let args = &response.tool_calls[0].arguments;
                let action = args
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("skip")
                    .to_string();
                let tasks = args
                    .get("tasks")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                (action, tasks)
            }
            Ok(_) => ("skip".to_string(), String::new()),
            Err(e) => {
                error!("Heartbeat Phase 1 LLM error: {}", e);
                ("skip".to_string(), String::new())
            }
        }
    }

    async fn run_loop(self: Arc<Self>) {
        while self.running.load(Ordering::SeqCst) {
            tokio::time::sleep(tokio::time::Duration::from_secs(self.interval_s)).await;
            if !self.running.load(Ordering::SeqCst) {
                break;
            }
            if let Err(e) = self.tick().await {
                error!("Heartbeat tick error: {}", e);
            }
        }
    }

    async fn tick(self: &Arc<Self>) -> Result<()> {
        let Some(content) = self.read_heartbeat_file() else {
            debug!("Heartbeat: HEARTBEAT.md missing or empty");
            return Ok(());
        };

        info!("Heartbeat: checking for tasks...");
        let (action, tasks) = self.decide(&content).await;

        if action != "run" {
            info!("Heartbeat: OK (nothing to do)");
            return Ok(());
        }

        info!("Heartbeat: tasks found, executing...");
        let exec_cb = self.on_execute.lock().unwrap().clone();
        let Some(exec_cb) = exec_cb else {
            return Ok(());
        };

        let response = exec_cb(tasks).await;

        if response.is_empty() {
            return Ok(());
        }

        // Deliver the response if a notify callback is configured.
        let notify_cb = self.on_notify.lock().unwrap().clone();
        if let Some(notify_cb) = notify_cb {
            info!("Heartbeat: delivering response");
            notify_cb(response).await;
        } else {
            info!("Heartbeat: completed (no notify target configured)");
        }

        Ok(())
    }
}
