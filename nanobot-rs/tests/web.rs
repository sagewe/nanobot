use std::net::SocketAddr;

use axum::routing::get;
use axum::{Json, Router, response::Html};
use nanobot_rs::config::{WebFetchToolConfig, WebSearchToolConfig};
use nanobot_rs::tools::{Tool, WebFetchTool, WebSearchTool};
use serde_json::{Value, json};
use tokio::net::TcpListener;

async fn ddg_results() -> Html<&'static str> {
    Html(
        r#"
        <html><body>
          <div class="result">
            <a class="result__a" href="https://example.com/one">Example One</a>
            <a class="result__url" href="https://example.com/one">https://example.com/one</a>
            <div class="result__snippet">First result snippet.</div>
          </div>
          <div class="result">
            <a class="result__a" href="https://example.com/two">Example Two</a>
            <div class="result__snippet">Second result snippet.</div>
          </div>
        </body></html>
        "#,
    )
}

async fn brave_results() -> Json<Value> {
    Json(json!({
        "web": {
            "results": [{
                "title": "Brave Result",
                "url": "https://brave.example/result",
                "description": "Brave snippet"
            }]
        }
    }))
}

async fn jina_results() -> Json<Value> {
    Json(json!({
        "data": [{
            "title": "Jina Result",
            "url": "https://jina.example/result",
            "description": "Jina snippet"
        }]
    }))
}

async fn start_server() -> SocketAddr {
    let app = Router::new()
        .route("/ddg", get(ddg_results))
        .route("/brave", get(brave_results))
        .route("/jina", get(jina_results));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    addr
}

#[tokio::test]
async fn web_search_parses_duckduckgo_html_results() {
    let addr = start_server().await;
    let tool = WebSearchTool::new(WebSearchToolConfig {
        provider: "duckduckgo".to_string(),
        api_key: String::new(),
        base_url: format!("http://{addr}/ddg"),
        max_results: 5,
    });

    let result = tool.execute(json!({"query": "example", "count": 2})).await;

    assert!(result.contains("Example One"));
    assert!(result.contains("https://example.com/one"));
    assert!(result.contains("First result snippet."));
    assert!(result.contains("Example Two"));
}

#[tokio::test]
async fn web_search_falls_back_to_duckduckgo_when_brave_key_missing() {
    let addr = start_server().await;
    let tool = WebSearchTool::new(WebSearchToolConfig {
        provider: "brave".to_string(),
        api_key: String::new(),
        base_url: format!("http://{addr}/ddg"),
        max_results: 5,
    });

    let result = tool.execute(json!({"query": "example"})).await;

    assert!(result.contains("Example One"));
    assert!(!result.contains("unknown provider"));
}

#[tokio::test]
async fn web_search_parses_brave_json_results() {
    let addr = start_server().await;
    let tool = WebSearchTool::new(WebSearchToolConfig {
        provider: "brave".to_string(),
        api_key: "secret".to_string(),
        base_url: format!("http://{addr}/brave"),
        max_results: 5,
    });

    let result = tool.execute(json!({"query": "example"})).await;

    assert!(result.contains("Brave Result"));
    assert!(result.contains("https://brave.example/result"));
    assert!(result.contains("Brave snippet"));
}

#[tokio::test]
async fn web_search_parses_jina_json_results() {
    let addr = start_server().await;
    let tool = WebSearchTool::new(WebSearchToolConfig {
        provider: "jina".to_string(),
        api_key: "secret".to_string(),
        base_url: format!("http://{addr}/jina"),
        max_results: 5,
    });

    let result = tool.execute(json!({"query": "example"})).await;

    assert!(result.contains("Jina Result"));
    assert!(result.contains("https://jina.example/result"));
    assert!(result.contains("Jina snippet"));
}

#[tokio::test]
async fn web_search_rejects_unknown_provider() {
    let tool = WebSearchTool::new(WebSearchToolConfig {
        provider: "mystery".to_string(),
        api_key: String::new(),
        base_url: String::new(),
        max_results: 5,
    });

    let result = tool.execute(json!({"query": "example"})).await;

    assert!(result.contains("Unknown web search provider"));
}

#[tokio::test]
async fn web_fetch_blocks_localhost_targets() {
    let tool = WebFetchTool::new(WebFetchToolConfig { max_chars: 2_000 });

    let result = tool
        .execute(json!({"url": "http://127.0.0.1:8080/private"}))
        .await;

    let value: Value = serde_json::from_str(&result).expect("json result");
    assert_eq!(
        value.get("url").and_then(Value::as_str),
        Some("http://127.0.0.1:8080/private")
    );
    assert!(
        value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("internal/private")
    );
}
