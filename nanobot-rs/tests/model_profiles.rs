use std::fs;

use nanobot_rs::config::load_config;
use serde_json::Value;
use tempfile::tempdir;

fn load_config_from_json(raw: &str) -> anyhow::Result<Value> {
    let dir = tempdir()?;
    let path = dir.path().join("config.json");
    fs::write(&path, raw)?;
    let config = load_config(Some(&path))?;
    serde_json::to_value(config).map_err(Into::into)
}

#[test]
fn new_shape_deserializes_profiles_and_default_profile() {
    let value = load_config_from_json(
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/nanobot",
      "maxToolIterations": 20,
      "defaultProfile": "openai:gpt-4.1-mini"
    },
    "profiles": {
      "openai:gpt-4.1-mini": {
        "provider": "openai",
        "model": "gpt-4.1-mini",
        "request": {
          "temperature": 0.2
        }
      }
    }
  }
}"#,
    )
    .expect("load config");

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
            .pointer("/agents/profiles/openai:gpt-4.1-mini/request/temperature")
            .and_then(Value::as_f64),
        Some(0.2)
    );
}

#[test]
fn request_defaults_to_an_empty_object_when_omitted() {
    let value = load_config_from_json(
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/nanobot",
      "maxToolIterations": 20,
      "defaultProfile": "openai:gpt-4.1-mini"
    },
    "profiles": {
      "openai:gpt-4.1-mini": {
        "provider": "openai",
        "model": "gpt-4.1-mini"
      }
    }
  }
}"#,
    )
    .expect("load config");

    assert_eq!(
        value
            .pointer("/agents/profiles/openai:gpt-4.1-mini/request")
            .and_then(Value::as_object)
            .map(|request| request.len()),
        Some(0)
    );
}

#[test]
fn non_object_request_is_rejected() {
    let err = load_config_from_json(
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/nanobot",
      "maxToolIterations": 20,
      "defaultProfile": "openai:gpt-4.1-mini"
    },
    "profiles": {
      "openai:gpt-4.1-mini": {
        "provider": "openai",
        "model": "gpt-4.1-mini",
        "request": "invalid"
      }
    }
  }
}"#,
    )
    .expect_err("request should be rejected");

    assert!(err.to_string().contains("request"));
}

#[test]
fn unknown_provider_is_rejected_during_load() {
    let err = load_config_from_json(
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/nanobot",
      "maxToolIterations": 20,
      "defaultProfile": "custom:demo"
    },
    "profiles": {
      "custom:demo": {
        "provider": "does-not-exist",
        "model": "demo"
      }
    }
  }
}"#,
    )
    .expect_err("unknown provider should be rejected");

    assert!(err.to_string().contains("does-not-exist"));
}

#[test]
fn missing_default_profile_is_rejected() {
    let err = load_config_from_json(
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/nanobot",
      "maxToolIterations": 20
    },
    "profiles": {
      "openai:gpt-4.1-mini": {
        "provider": "openai",
        "model": "gpt-4.1-mini"
      }
    }
  }
}"#,
    )
    .expect_err("missing default profile should be rejected");

    assert!(err.to_string().contains("defaultProfile"));
}

#[test]
fn legacy_provider_and_model_synthesize_one_default_profile() {
    let value = load_config_from_json(
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/nanobot",
      "provider": "ollama",
      "model": "llama3.2",
      "maxToolIterations": 20
    }
  }
}"#,
    )
    .expect("load legacy config");

    assert_eq!(
        value
            .pointer("/agents/defaults/defaultProfile")
            .and_then(Value::as_str),
        Some("ollama:llama3.2")
    );
    assert_eq!(
        value
            .pointer("/agents/profiles/ollama:llama3.2/provider")
            .and_then(Value::as_str),
        Some("ollama")
    );
    assert_eq!(
        value
            .pointer("/agents/profiles/ollama:llama3.2/model")
            .and_then(Value::as_str),
        Some("llama3.2")
    );
}

#[test]
fn missing_default_profile_without_legacy_shape_is_rejected() {
    let err = load_config_from_json(
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/nanobot",
      "maxToolIterations": 20
    }
  }
}"#,
    )
    .expect_err("missing default profile should fail clearly");

    assert!(err.to_string().contains("defaultProfile"));
}
