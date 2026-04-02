use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use axum::Router;
use axum::response::Html;
use axum::routing::get;
use sidekick::agent::{AgentLoop, SubagentManager};
use sidekick::bus::MessageBus;
use sidekick::config::{WebSearchToolConfig, WebToolsConfig};
use sidekick::providers::{LlmProvider, LlmResponse, ToolCall};
use sidekick::tools::build_default_tools;
use serde_json::{Map, json};
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

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

async fn ddg_results() -> Html<&'static str> {
    Html(
        r#"
        <html><body>
          <div class="result">
            <a class="result__a" href="https://example.com/one">Example One</a>
            <div class="result__snippet">First result snippet.</div>
          </div>
        </body></html>
        "#,
    )
}

async fn start_server() -> SocketAddr {
    let app = Router::new().route("/ddg", get(ddg_results));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    addr
}

fn web_tools(addr: SocketAddr) -> WebToolsConfig {
    WebToolsConfig {
        search: WebSearchToolConfig {
            provider: "duckduckgo".to_string(),
            api_key: String::new(),
            base_url: format!("http://{addr}/ddg"),
            max_results: 5,
        },
        ..WebToolsConfig::default()
    }
}

#[tokio::test]
async fn build_default_tools_exposes_web_search_and_web_fetch() {
    let dir = tempdir().expect("tempdir");
    let bus = MessageBus::new(32);
    let subagents = SubagentManager::new(
        mock_provider(Vec::new()),
        dir.path().to_path_buf(),
        bus.clone(),
        "mock-model".to_string(),
        5,
        10,
        false,
        WebToolsConfig::default(),
    );

    let tools = build_default_tools(
        dir.path().to_path_buf(),
        bus,
        10,
        false,
        subagents,
        WebToolsConfig::default(),
        None,
    )
    .await;
    let defs = tools.definitions().await;
    let names = defs
        .iter()
        .filter_map(|tool| {
            tool.pointer("/function/name")
                .and_then(|value| value.as_str())
        })
        .collect::<Vec<_>>();

    assert!(names.contains(&"web_search"));
    assert!(names.contains(&"web_fetch"));
}

#[tokio::test]
async fn subagent_can_execute_web_search_tool() {
    let addr = start_server().await;
    let dir = tempdir().expect("tempdir");
    let provider = mock_provider(vec![
        LlmResponse {
            content: Some("researching".to_string()),
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "web_search".to_string(),
                arguments: json!({"query": "example query"}),
            }],
            finish_reason: "tool_calls".to_string(),
            extra: Map::new(),
        },
        LlmResponse {
            content: Some("background done".to_string()),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
            extra: Map::new(),
        },
    ]);
    let bus = MessageBus::new(32);
    let manager = SubagentManager::new(
        provider,
        dir.path().to_path_buf(),
        bus.clone(),
        "mock-model".to_string(),
        5,
        10,
        false,
        web_tools(addr),
    );

    let status = manager
        .spawn(
            "search the web".to_string(),
            Some("search".to_string()),
            "cli".to_string(),
            "test".to_string(),
        )
        .await;
    assert!(status.contains("Subagent [search] started"));

    let inbound = tokio::time::timeout(std::time::Duration::from_secs(2), bus.consume_inbound())
        .await
        .expect("timely inbound")
        .expect("inbound message");
    assert_eq!(inbound.channel, "system");
    assert!(inbound.content.contains("background done"));
}

#[tokio::test]
async fn agent_can_execute_web_search_tool() {
    let addr = start_server().await;
    let dir = tempdir().expect("tempdir");
    let provider = mock_provider(vec![
        LlmResponse {
            content: Some("researching".to_string()),
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "web_search".to_string(),
                arguments: json!({"query": "example query"}),
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
    let bus = MessageBus::new(32);
    let agent = AgentLoop::new(
        bus,
        provider,
        dir.path().to_path_buf(),
        "mock-model".to_string(),
        5,
        10,
        false,
        web_tools(addr),
    )
    .await
    .expect("agent");

    let result = agent
        .process_direct("search the web", "cli:test", "cli", "test")
        .await
        .expect("process");

    assert_eq!(result, "done");
}
