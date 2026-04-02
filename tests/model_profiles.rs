use std::fs;

use serde_json::Value;
use sidekick::config::load_config;
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
      "workspace": "/tmp/sidekick",
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
      "workspace": "/tmp/sidekick",
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
fn message_debounce_ms_loads_from_camel_case_config() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("config.json");
    fs::write(
        &path,
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/sidekick",
      "maxToolIterations": 20,
      "messageDebounceMs": 1500,
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
    .expect("write config");

    let config = load_config(Some(&path)).expect("load config");
    assert_eq!(config.agents.defaults.message_debounce_ms, 1500);
}

#[test]
fn empty_config_still_inherits_the_default_profile() {
    let value = load_config_from_json("{}").expect("load sparse config");

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
}

#[test]
fn sparse_weixin_config_keeps_enabled_and_fills_default_bases() {
    let value = load_config_from_json(
        r#"{
  "channels": {
    "weixin": {
      "enabled": true
    }
  }
}"#,
    )
    .expect("load config");

    assert_eq!(
        value
            .pointer("/channels/weixin/enabled")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value
            .pointer("/channels/weixin/apiBase")
            .and_then(Value::as_str),
        Some("https://ilinkai.weixin.qq.com")
    );
    assert_eq!(
        value
            .pointer("/channels/weixin/cdnBase")
            .and_then(Value::as_str),
        Some("https://novac2c.cdn.weixin.qq.com/c2c")
    );
}

#[test]
fn non_object_request_is_rejected() {
    let err = load_config_from_json(
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/sidekick",
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
      "workspace": "/tmp/sidekick",
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
      "workspace": "/tmp/sidekick",
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
      "workspace": "/tmp/sidekick",
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
fn legacy_provider_and_model_synthesize_default_profile_even_with_other_profiles() {
    let value = load_config_from_json(
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/sidekick",
      "provider": "ollama",
      "model": "llama3.2",
      "maxToolIterations": 20
    },
    "profiles": {
      "openai:gpt-4.1-mini": {
        "provider": "openai",
        "model": "gpt-4.1-mini",
        "request": {
          "temperature": 0.4
        }
      }
    }
  }
}"#,
    )
    .expect("load mixed legacy/new config");

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
            .pointer("/agents/profiles/openai:gpt-4.1-mini/provider")
            .and_then(Value::as_str),
        Some("openai")
    );
    assert_eq!(
        value
            .pointer("/agents/profiles/openai:gpt-4.1-mini/request/temperature")
            .and_then(Value::as_f64),
        Some(0.4)
    );
}

#[test]
fn legacy_synthesis_preserves_existing_profile_at_the_same_key() {
    let value = load_config_from_json(
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/sidekick",
      "provider": "ollama",
      "model": "llama3.2",
      "maxToolIterations": 20
    },
    "profiles": {
      "ollama:llama3.2": {
        "provider": "ollama",
        "model": "llama3.2",
        "request": {
          "temperature": 0.7
        }
      },
      "openai:gpt-4.1-mini": {
        "provider": "openai",
        "model": "gpt-4.1-mini",
        "request": {
          "temperature": 0.4
        }
      }
    }
  }
}"#,
    )
    .expect("load config with colliding legacy profile");

    assert_eq!(
        value
            .pointer("/agents/defaults/defaultProfile")
            .and_then(Value::as_str),
        Some("ollama:llama3.2")
    );
    assert_eq!(
        value
            .pointer("/agents/profiles/ollama:llama3.2/request/temperature")
            .and_then(Value::as_f64),
        Some(0.7)
    );
    assert_eq!(
        value
            .pointer("/agents/profiles/openai:gpt-4.1-mini/request/temperature")
            .and_then(Value::as_f64),
        Some(0.4)
    );
}

#[test]
fn legacy_synthesis_rejects_colliding_profile_with_mismatched_identity() {
    let err = load_config_from_json(
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/sidekick",
      "provider": "ollama",
      "model": "llama3.2",
      "maxToolIterations": 20
    },
    "profiles": {
      "ollama:llama3.2": {
        "provider": "openai",
        "model": "gpt-4.1-mini",
        "request": {
          "temperature": 0.7
        }
      }
    }
  }
}"#,
    )
    .expect_err("mismatched colliding profile should be rejected");

    assert!(err.to_string().contains("ollama:llama3.2"));
}

#[test]
fn legacy_synthesis_accepts_case_insensitive_matching_provider_identity() {
    let value = load_config_from_json(
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/sidekick",
      "provider": "openai",
      "model": "gpt-4.1-mini",
      "maxToolIterations": 20
    },
    "profiles": {
      "openai:gpt-4.1-mini": {
        "provider": " OpenAI ",
        "model": "gpt-4.1-mini",
        "request": {
          "temperature": 0.4
        }
      }
    }
  }
}"#,
    )
    .expect("case-insensitive provider identity should be accepted");

    assert_eq!(
        value
            .pointer("/agents/defaults/defaultProfile")
            .and_then(Value::as_str),
        Some("openai:gpt-4.1-mini")
    );
    assert_eq!(
        value
            .pointer("/agents/profiles/openai:gpt-4.1-mini/request/temperature")
            .and_then(Value::as_f64),
        Some(0.4)
    );
}

#[test]
fn provider_only_config_still_inherits_the_default_profile() {
    let value = load_config_from_json(
        r#"{
  "providers": {
    "custom": {
      "apiBase": "https://models.example.test/v1"
    }
  }
}"#,
    )
    .expect("load provider-only config");

    assert_eq!(
        value
            .pointer("/agents/defaults/defaultProfile")
            .and_then(Value::as_str),
        Some("openai:gpt-4.1-mini")
    );
    assert_eq!(
        value
            .pointer("/providers/custom/apiBase")
            .and_then(Value::as_str),
        Some("https://models.example.test/v1")
    );
}

#[test]
fn codex_profiles_are_accepted_when_the_raw_codex_provider_block_is_present() {
    let value = load_config_from_json(
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/sidekick",
      "maxToolIterations": 20,
      "defaultProfile": "codex:gpt-5-codex"
    },
    "profiles": {
      "codex:gpt-5-codex": {
        "provider": "codex",
        "model": "gpt-5-codex",
        "request": {}
      }
    }
  },
  "providers": {
    "codex": {
      "authFile": "~/.codex/auth.json",
      "apiBase": "https://chatgpt.com/backend-api/codex"
    }
  }
}"#,
    )
    .expect("codex profile should load");

    assert_eq!(
        value
            .pointer("/agents/defaults/defaultProfile")
            .and_then(Value::as_str),
        Some("codex:gpt-5-codex")
    );
    assert_eq!(
        value
            .pointer("/agents/profiles/codex:gpt-5-codex/provider")
            .and_then(Value::as_str),
        Some("codex")
    );
    assert_eq!(
        value
            .pointer("/providers/codex/authFile")
            .and_then(Value::as_str),
        Some("~/.codex/auth.json")
    );
    assert_eq!(
        value
            .pointer("/providers/codex/apiBase")
            .and_then(Value::as_str),
        Some("https://chatgpt.com/backend-api/codex")
    );
}

#[test]
fn codex_default_profile_fails_when_the_raw_codex_provider_block_is_missing() {
    let err = load_config_from_json(
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/sidekick",
      "maxToolIterations": 20,
      "defaultProfile": "codex:gpt-5-codex"
    },
    "profiles": {
      "codex:gpt-5-codex": {
        "provider": "codex",
        "model": "gpt-5-codex",
        "request": {}
      }
    }
  }
}"#,
    )
    .expect_err("missing raw codex provider block should fail");

    assert!(err.to_string().contains("providers.codex"));
}

#[test]
fn codex_profile_elsewhere_still_requires_the_raw_codex_provider_block() {
    let err = load_config_from_json(
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/sidekick",
      "maxToolIterations": 20,
      "defaultProfile": "openai:gpt-4.1-mini"
    },
    "profiles": {
      "openai:gpt-4.1-mini": {
        "provider": "openai",
        "model": "gpt-4.1-mini",
        "request": {}
      },
      "codex:gpt-5-codex": {
        "provider": "codex",
        "model": "gpt-5-codex",
        "request": {}
      }
    }
  }
}"#,
    )
    .expect_err("codex profile should require raw codex provider block");

    assert!(err.to_string().contains("providers.codex"));
}
