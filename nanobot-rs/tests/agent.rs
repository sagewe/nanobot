use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use nanobot_rs::agent::{AgentLoop, SubagentManager};
use nanobot_rs::bus::{InboundMessage, MessageBus};
use nanobot_rs::config::WebToolsConfig;
use nanobot_rs::providers::{LlmProvider, LlmResponse, ToolCall};
use serde_json::json;
use tempfile::tempdir;
use tokio::sync::{Mutex, Notify};

#[derive(Clone)]
struct MockProvider {
    model: String,
    responses: Arc<Mutex<VecDeque<LlmResponse>>>,
}

#[async_trait]
impl LlmProvider for MockProvider {
    fn default_model(&self) -> &str {
        &self.model
    }

    async fn chat(
        &self,
        _messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        _model: &str,
    ) -> Result<LlmResponse> {
        self.responses
            .lock()
            .await
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("no more responses"))
    }
}

fn mock_provider(responses: Vec<LlmResponse>) -> Arc<dyn LlmProvider> {
    Arc::new(MockProvider {
        model: "mock-model".to_string(),
        responses: Arc::new(Mutex::new(responses.into())),
    })
}

#[derive(Default)]
struct ConcurrentProviderState {
    plain_started: AtomicBool,
    plain_finished: AtomicBool,
    tool_second_seen: AtomicBool,
    plain_started_notify: Notify,
    plain_finished_notify: Notify,
    tool_second_notify: Notify,
}

#[derive(Clone)]
struct ConcurrentProvider {
    state: Arc<ConcurrentProviderState>,
}

#[async_trait]
impl LlmProvider for ConcurrentProvider {
    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn chat(
        &self,
        messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        _model: &str,
    ) -> Result<LlmResponse> {
        let user_content = messages
            .iter()
            .rev()
            .find(|message| message.get("role").and_then(|role| role.as_str()) == Some("user"))
            .and_then(|message| message.get("content").and_then(|content| content.as_str()))
            .unwrap_or_default();
        let has_tool_result = messages
            .iter()
            .any(|message| message.get("role").and_then(|role| role.as_str()) == Some("tool"));

        if user_content.ends_with("plain") && !has_tool_result {
            self.state.plain_started.store(true, Ordering::SeqCst);
            self.state.plain_started_notify.notify_waiters();
            while !self.state.tool_second_seen.load(Ordering::SeqCst) {
                self.state.tool_second_notify.notified().await;
            }
            return Ok(LlmResponse {
                content: Some("plain final".to_string()),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
            });
        }

        if user_content.ends_with("tool") && !has_tool_result {
            return Ok(LlmResponse {
                content: Some("sending".to_string()),
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "message".to_string(),
                    arguments: json!({"content": "tool reply"}),
                }],
                finish_reason: "tool_calls".to_string(),
            });
        }

        if user_content.ends_with("tool") && has_tool_result {
            self.state.tool_second_seen.store(true, Ordering::SeqCst);
            self.state.tool_second_notify.notify_waiters();
            while !self.state.plain_finished.load(Ordering::SeqCst) {
                self.state.plain_finished_notify.notified().await;
            }
            return Ok(LlmResponse {
                content: Some("tool final".to_string()),
                tool_calls: Vec::new(),
                finish_reason: "stop".to_string(),
            });
        }

        Err(anyhow::anyhow!(
            "unexpected request shape for concurrent provider"
        ))
    }
}

#[tokio::test]
async fn agent_executes_tool_loop() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("note.txt"), "hello from file").expect("write note");
    let provider = mock_provider(vec![
        LlmResponse {
            content: Some("looking".to_string()),
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "read_file".to_string(),
                arguments: json!({"path": "note.txt"}),
            }],
            finish_reason: "tool_calls".to_string(),
        },
        LlmResponse {
            content: Some("done".to_string()),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
        },
    ]);
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus,
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    )
    .await
    .expect("agent");
    let result = agent
        .process_direct("read note", "cli:test", "cli", "test")
        .await
        .expect("process");
    assert_eq!(result, "done");
}

#[tokio::test]
async fn agent_process_direct_returns_message_tool_reply() {
    let dir = tempdir().expect("tempdir");
    let long_chinese =
        "请问您想查询哪个城市的天气？请提供城市名称或位置信息，这样我才能帮您查询天气情况。";
    let provider = mock_provider(vec![
        LlmResponse {
            content: Some("sending".to_string()),
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "message".to_string(),
                arguments: json!({"content": long_chinese}),
            }],
            finish_reason: "tool_calls".to_string(),
        },
        LlmResponse {
            content: Some("done".to_string()),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
        },
    ]);
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus.clone(),
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    )
    .await
    .expect("agent");
    let result = agent
        .process_direct("say hi", "cli:test", "cli", "test")
        .await
        .expect("process");
    assert_eq!(result, long_chinese);
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            bus.consume_outbound()
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn agent_bus_mode_suppresses_duplicate_final_reply_after_message_tool() {
    let dir = tempdir().expect("tempdir");
    let long_chinese =
        "请问您想查询哪个城市的天气？请提供城市名称或位置信息，这样我才能帮您查询天气情况。";
    let provider = mock_provider(vec![
        LlmResponse {
            content: Some("sending".to_string()),
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "message".to_string(),
                arguments: json!({"content": long_chinese}),
            }],
            finish_reason: "tool_calls".to_string(),
        },
        LlmResponse {
            content: Some("done".to_string()),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
        },
    ]);
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus.clone(),
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    )
    .await
    .expect("agent");
    let runner = {
        let agent = agent.clone();
        tokio::spawn(async move {
            agent.run().await;
        })
    };
    bus.publish_inbound(InboundMessage {
        channel: "cli".to_string(),
        sender_id: "user".to_string(),
        chat_id: "test".to_string(),
        content: "say hi".to_string(),
        timestamp: chrono::Utc::now(),
        metadata: Default::default(),
        session_key_override: Some("cli:test".to_string()),
    })
    .await
    .expect("publish inbound");
    let outbound = loop {
        let outbound = bus.consume_outbound().await.expect("message tool outbound");
        let is_progress = outbound
            .metadata
            .get("_progress")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        if !is_progress {
            break outbound;
        }
    };
    assert_eq!(outbound.content, long_chinese);
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            bus.consume_outbound()
        )
        .await
        .is_err()
    );
    agent.stop();
    runner.abort();
}

#[tokio::test]
async fn agent_returns_iteration_limit_message() {
    let dir = tempdir().expect("tempdir");
    let provider = mock_provider(vec![
        LlmResponse {
            content: None,
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "list_dir".to_string(),
                arguments: json!({"path": "."}),
            }],
            finish_reason: "tool_calls".to_string(),
        },
        LlmResponse {
            content: None,
            tool_calls: vec![ToolCall {
                id: "call_2".to_string(),
                name: "list_dir".to_string(),
                arguments: json!({"path": "."}),
            }],
            finish_reason: "tool_calls".to_string(),
        },
    ]);
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus,
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        1,
        10,
        false,
        WebToolsConfig::default(),
    )
    .await
    .expect("agent");
    let result = agent
        .process_direct("loop", "cli:test", "cli", "test")
        .await
        .expect("process");
    assert!(result.contains("maximum number of tool call iterations (1)"));
}

#[tokio::test]
async fn subagent_reports_back_via_bus() {
    let dir = tempdir().expect("tempdir");
    let provider = mock_provider(vec![LlmResponse {
        content: Some("background result".to_string()),
        tool_calls: Vec::new(),
        finish_reason: "stop".to_string(),
    }]);
    let bus = MessageBus::new(32);
    let manager = SubagentManager::new(
        provider,
        dir.path().to_path_buf(),
        bus.clone(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    );
    let status = manager
        .spawn(
            "do background work".to_string(),
            Some("job".to_string()),
            "cli".to_string(),
            "test".to_string(),
        )
        .await;
    assert!(status.contains("Subagent [job] started"));
    let inbound = tokio::time::timeout(std::time::Duration::from_secs(2), bus.consume_inbound())
        .await
        .expect("timely inbound")
        .expect("inbound message");
    assert_eq!(inbound.channel, "system");
    assert!(inbound.content.contains("background result"));
}

#[tokio::test]
async fn concurrent_direct_requests_do_not_share_tool_state() {
    let dir = tempdir().expect("tempdir");
    let state = Arc::new(ConcurrentProviderState::default());
    let provider: Arc<dyn LlmProvider> = Arc::new(ConcurrentProvider {
        state: state.clone(),
    });
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus,
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    )
    .await
    .expect("agent");

    let plain_agent = agent.clone();
    let plain_task = tokio::spawn(async move {
        plain_agent
            .process_direct("plain", "web:plain", "web", "plain")
            .await
    });

    while !state.plain_started.load(Ordering::SeqCst) {
        tokio::time::timeout(
            Duration::from_secs(1),
            state.plain_started_notify.notified(),
        )
        .await
        .expect("plain request should start")
    }

    let tool_agent = agent.clone();
    let tool_task = tokio::spawn(async move {
        tool_agent
            .process_direct("tool", "web:tool", "web", "tool")
            .await
    });

    let plain_result = plain_task.await.expect("plain join").expect("plain result");
    state.plain_finished.store(true, Ordering::SeqCst);
    state.plain_finished_notify.notify_waiters();
    let tool_result = tool_task.await.expect("tool join").expect("tool result");

    assert_eq!(plain_result, "plain final");
    assert_eq!(tool_result, "tool reply");
}
