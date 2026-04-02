use std::collections::VecDeque;
use std::io;
use std::io::Write;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Map, json};
use sidekick::agent::AgentLoop;
use sidekick::bus::MessageBus;
use sidekick::config::WebToolsConfig;
use sidekick::providers::{LlmProvider, LlmResponse, ToolCall};
use sidekick::web::{AgentChatService, ChatService};
use tempfile::tempdir;
use tokio::sync::Mutex as AsyncMutex;

#[derive(Clone)]
struct MockProvider {
    model: String,
    responses: Arc<AsyncMutex<VecDeque<LlmResponse>>>,
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
        responses: Arc::new(AsyncMutex::new(responses.into())),
    })
}

#[derive(Clone, Default)]
struct SharedWriter {
    buffer: Arc<Mutex<Vec<u8>>>,
}

struct SharedWriterHandle {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SharedWriter {
    type Writer = SharedWriterHandle;

    fn make_writer(&'a self) -> Self::Writer {
        SharedWriterHandle {
            buffer: self.buffer.clone(),
        }
    }
}

impl Write for SharedWriterHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.lock().expect("buffer").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn web_chat_logs_agent_execution_progress() {
    let dir = tempdir().expect("tempdir");
    let provider = mock_provider(vec![
        LlmResponse {
            content: Some("checking workspace".to_string()),
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "list_dir".to_string(),
                arguments: json!({"path": "."}),
            }],
            finish_reason: "tool_calls".to_string(),
            extra: Map::new(),
        },
        LlmResponse {
            content: Some("done".to_string()),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
            extra: Map::new(),
        },
    ]);
    let agent = AgentLoop::new(
        MessageBus::new(32),
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
    let service = AgentChatService::new(agent);

    let writer = SharedWriter::default();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_writer(writer.clone())
        .finish();

    let _guard = tracing::subscriber::set_default(subscriber);
    let reply = service
        .chat("inspect the workspace", "web", "browser-session")
        .await;

    assert_eq!(reply.expect("reply").reply, "done");
    let logs = String::from_utf8(writer.buffer.lock().expect("buffer").clone()).expect("utf8");
    assert!(logs.contains("web session browser-session started"));
    assert!(logs.contains("checking workspace"));
    assert!(logs.contains("list_dir(\".\")"));
    assert!(logs.contains("web session browser-session completed"));
}
