use std::collections::HashMap;

use nanobot_rs::presentation::should_deliver_to_channel;
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
