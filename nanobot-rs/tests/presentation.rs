use std::collections::HashMap;

use nanobot_rs::presentation::{
    render_telegram_html, render_web_html, render_wecom_markdown, should_deliver_to_channel,
};
use serde_json::{Value, json};

fn progress_metadata() -> HashMap<String, Value> {
    HashMap::from([("_progress".to_string(), json!(true))])
}

fn tool_hint_metadata() -> HashMap<String, Value> {
    HashMap::from([("_tool_hint".to_string(), json!(true))])
}

#[test]
fn runtime_messages_are_hidden_from_external_channels() {
    assert!(!should_deliver_to_channel("telegram", &progress_metadata()));
    assert!(!should_deliver_to_channel("wecom", &tool_hint_metadata()));
    assert!(!should_deliver_to_channel("web", &progress_metadata()));
}

#[test]
fn cli_keeps_runtime_messages_visible() {
    assert!(should_deliver_to_channel("cli", &progress_metadata()));
}

#[test]
fn normal_messages_remain_visible_everywhere() {
    assert!(should_deliver_to_channel("telegram", &HashMap::new()));
    assert!(should_deliver_to_channel("wecom", &HashMap::new()));
    assert!(should_deliver_to_channel("web", &HashMap::new()));
}

#[test]
fn web_renderer_returns_sanitized_html() {
    let html = render_web_html("**bold** <script>alert(1)</script>");

    assert!(html.contains("<strong>bold</strong>"));
    assert!(!html.contains("<script>"));
}

#[test]
fn telegram_renderer_returns_html_subset() {
    let html = render_telegram_html("**bold** `code` [link](https://example.com)");

    assert!(html.contains("<b>bold</b>"));
    assert!(html.contains("<code>code</code>"));
    assert!(html.contains("<a href=\"https://example.com\">link</a>"));
}

#[test]
fn wecom_renderer_returns_markdown_and_enforces_limit() {
    let rendered = render_wecom_markdown("# title");

    assert!(rendered.contains("# title"));
}
