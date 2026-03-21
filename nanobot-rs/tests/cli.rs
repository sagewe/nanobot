use std::process::Command;

use serde_json::Value;
use tempfile::tempdir;

#[test]
fn onboard_generates_config_with_provider_and_web_defaults() {
    let dir = tempdir().expect("tempdir");
    let config_path = dir.path().join("config.json");
    let workspace_path = dir.path().join("workspace");

    let output = Command::new(env!("CARGO_BIN_EXE_nanobot-rs"))
        .arg("onboard")
        .arg("--config")
        .arg(&config_path)
        .arg("--workspace")
        .arg(&workspace_path)
        .output()
        .expect("run onboard");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let raw = std::fs::read_to_string(&config_path).expect("read config");
    let value: Value = serde_json::from_str(&raw).expect("parse config");

    assert_eq!(
        value
            .pointer("/agents/defaults/defaultProfile")
            .and_then(Value::as_str),
        Some("openai:gpt-4.1-mini")
    );
    assert_eq!(
        value
            .pointer("/agents/profiles/openai:gpt-4.1-mini/provider")
            .and_then(Value::as_str),
        Some("openai")
    );
    assert_eq!(
        value
            .pointer("/agents/profiles/openai:gpt-4.1-mini/model")
            .and_then(Value::as_str),
        Some("gpt-4.1-mini")
    );
    assert_eq!(
        value
            .pointer("/agents/profiles/openai:gpt-4.1-mini/request")
            .and_then(Value::as_object)
            .map(|request| request.len()),
        Some(0)
    );
    assert!(value.pointer("/agents/defaults/provider").is_none());
    assert!(value.pointer("/agents/defaults/model").is_none());
    assert_eq!(
        value
            .pointer("/providers/custom/apiBase")
            .and_then(Value::as_str),
        Some("http://localhost:8000/v1")
    );
    assert_eq!(
        value
            .pointer("/providers/openrouter/apiBase")
            .and_then(Value::as_str),
        Some("https://openrouter.ai/api/v1")
    );
    assert_eq!(
        value
            .pointer("/providers/ollama/apiBase")
            .and_then(Value::as_str),
        Some("http://localhost:11434/v1")
    );
    assert_eq!(
        value
            .pointer("/tools/web/search/provider")
            .and_then(Value::as_str),
        Some("duckduckgo")
    );
    assert_eq!(
        value
            .pointer("/tools/web/fetch/maxChars")
            .and_then(Value::as_u64),
        Some(20_000)
    );
    assert_eq!(
        value
            .pointer("/channels/wecom/enabled")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        value
            .pointer("/channels/wecom/wsBase")
            .and_then(Value::as_str),
        Some("wss://openws.work.weixin.qq.com")
    );
}
