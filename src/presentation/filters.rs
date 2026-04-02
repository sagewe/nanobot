use std::collections::HashMap;

use serde_json::Value;

pub fn should_deliver_to_channel(channel: &str, metadata: &HashMap<String, Value>) -> bool {
    let is_runtime = metadata
        .get("_progress")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || metadata
            .get("_tool_hint")
            .and_then(Value::as_bool)
            .unwrap_or(false);

    match channel {
        "telegram" | "wecom" | "web" | "weixin" | "feishu" => !is_runtime,
        _ => true,
    }
}
