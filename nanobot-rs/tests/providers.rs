use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use async_trait::async_trait;
use nanobot_rs::config::Config;
use nanobot_rs::config::load_config;
use nanobot_rs::providers::{
    LlmProvider, LlmResponse, ProviderError, ProviderKind, ProviderRegistry,
};
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn config_defaults_expose_the_new_default_profile_shape() {
    let config = Config::default();
    let value = serde_json::to_value(&config).expect("serialize default config");

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
        config.providers.openai.api_base,
        "https://api.openai.com/v1"
    );
    assert_eq!(config.providers.custom.api_base, "http://localhost:8000/v1");
    assert_eq!(
        config.providers.openrouter.api_base,
        "https://openrouter.ai/api/v1"
    );
    assert_eq!(
        config.providers.ollama.api_base,
        "http://localhost:11434/v1"
    );
    assert_eq!(config.tools.web.search.provider, "duckduckgo");
    assert_eq!(config.tools.web.search.max_results, 5);
    assert_eq!(config.tools.web.fetch.max_chars, 20_000);
    assert_eq!(
        value
            .pointer("/channels/weixin/enabled")
            .and_then(Value::as_bool),
        Some(false)
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
    assert!(value.pointer("/channels/weixin/api_base").is_none());
    assert!(value.pointer("/channels/weixin/cdn_base").is_none());
}

#[test]
fn provider_registry_resolves_explicit_provider_defaults() {
    let registry = ProviderRegistry::default();

    let openai = registry.resolve("openai").expect("openai");
    assert_eq!(openai.kind, ProviderKind::OpenAi);
    assert!(openai.requires_api_key);
    assert_eq!(openai.default_api_base, "https://api.openai.com/v1");

    let custom = registry.resolve("custom").expect("custom");
    assert_eq!(custom.kind, ProviderKind::Custom);
    assert!(!custom.requires_api_key);
    assert_eq!(custom.default_api_base, "http://localhost:8000/v1");

    let openrouter = registry.resolve("openrouter").expect("openrouter");
    assert_eq!(openrouter.kind, ProviderKind::OpenRouter);
    assert!(openrouter.requires_api_key);
    assert_eq!(openrouter.default_api_base, "https://openrouter.ai/api/v1");

    let ollama = registry.resolve("ollama").expect("ollama");
    assert_eq!(ollama.kind, ProviderKind::Ollama);
    assert!(!ollama.requires_api_key);
    assert_eq!(ollama.default_api_base, "http://localhost:11434/v1");

    let codex = registry.resolve("codex").expect("codex");
    assert_eq!(codex.kind, ProviderKind::Codex);
    assert!(!codex.requires_api_key);
    assert_eq!(codex.default_api_base, "https://chatgpt.com/backend-api");
}

#[test]
fn provider_registry_builds_provider_configs_with_defaults() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("config.json");
    fs::write(
        &path,
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/nanobot",
      "defaultProfile": "openai:gpt-4.1-mini",
      "maxToolIterations": 20
    },
    "profiles": {
      "ollama:llama3.2": {
        "provider": "ollama",
        "model": "llama3.2",
        "request": {
          "temperature": 0.3
        }
      },
      "openai:gpt-4.1-mini": {
        "provider": "openai",
        "model": "gpt-4.1-mini",
        "request": {
          "temperature": 0.2
        }
      }
    }
  },
  "providers": {
    "openai": {
      "apiKey": "sk-test"
    }
  }
}"#,
    )
    .expect("write config");

    let config = load_config(Some(&path)).expect("load config");

    let registry = ProviderRegistry::default();
    let built = registry.build_config(&config).expect("build config");

    assert_eq!(built.kind, ProviderKind::OpenAi);
    assert_eq!(built.api_base, "https://api.openai.com/v1");
    assert_eq!(built.default_model, "gpt-4.1-mini");
    assert_eq!(built.api_key, "sk-test");
}

#[test]
fn config_defaults_include_a_concrete_codex_provider_block() {
    let config = Config::default();
    let value = serde_json::to_value(&config).expect("serialize default config");

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
        Some("https://chatgpt.com/backend-api")
    );
}

#[test]
fn config_rejects_default_profile_keys_that_are_missing_from_profiles() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("config.json");
    fs::write(
        &path,
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/nanobot",
      "defaultProfile": "openai:missing",
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
    .expect("write config");

    let err = load_config(Some(&path)).expect_err("missing default profile key should fail");
    assert!(err.to_string().contains("openai:missing"));
}

#[test]
fn provider_registry_preserves_custom_extra_headers() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("config.json");
    fs::write(
        &path,
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/nanobot",
      "defaultProfile": "custom:demo",
      "maxToolIterations": 20
    },
    "profiles": {
      "custom:demo": {
        "provider": "custom",
        "model": "demo",
        "request": {}
      }
    }
  },
  "providers": {
    "custom": {
      "apiBase": "https://models.example.test/v1",
      "apiKey": "secret",
      "extraHeaders": {
        "X-Trace": "abc123",
        "HTTP-Referer": "https://example.test"
      }
    }
  }
}"#,
    )
    .expect("write config");

    let config = load_config(Some(&path)).expect("load config");

    let registry = ProviderRegistry::default();
    let built = registry.build_config(&config).expect("build config");

    assert_eq!(built.kind, ProviderKind::Custom);
    assert_eq!(built.api_base, "https://models.example.test/v1");
    assert_eq!(built.api_key, "secret");
    assert_eq!(built.extra_headers["X-Trace"], "abc123");
    assert_eq!(built.extra_headers["HTTP-Referer"], "https://example.test");
}

#[test]
fn provider_registry_uses_the_selected_default_profile_from_the_new_shape() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("config.json");
    fs::write(
        &path,
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/nanobot",
      "defaultProfile": "ollama:llama3.2",
      "maxToolIterations": 20
    },
    "profiles": {
      "ollama:llama3.2": {
        "provider": "ollama",
        "model": "llama3.2",
        "request": {
          "temperature": 0.3
        }
      },
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
    .expect("write config");

    let config = load_config(Some(&path)).expect("load config");
    assert_eq!(config.agents.defaults.default_profile, "ollama:llama3.2");
    assert_eq!(config.agents.defaults.provider, "ollama");
    assert_eq!(config.agents.defaults.model, "llama3.2");
    let mut config = config;
    config.agents.defaults.provider = "openai".to_string();
    config.agents.defaults.model = "gpt-4.1-mini".to_string();
    let registry = ProviderRegistry::default();
    let built = registry.build_config(&config).expect("build config");

    assert_eq!(built.kind, ProviderKind::Ollama);
    assert_eq!(built.default_model, "llama3.2");
}

#[derive(Clone)]
struct RetryStubProvider {
    calls: Arc<AtomicUsize>,
    mode: RetryMode,
}

#[derive(Clone, Copy)]
enum RetryMode {
    Fatal,
    AlwaysRetryable,
}

#[async_trait]
impl LlmProvider for RetryStubProvider {
    fn default_model(&self) -> &str {
        "stub-model"
    }

    async fn chat(
        &self,
        _messages: Vec<Value>,
        _tools: Vec<Value>,
        _model: &str,
    ) -> Result<LlmResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.mode {
            RetryMode::Fatal => Err(ProviderError::fatal("fatal stub failure").into()),
            RetryMode::AlwaysRetryable => {
                Err(ProviderError::retryable("transient stub failure").into())
            }
        }
    }
}

#[tokio::test]
async fn chat_with_retry_propagates_fatal_errors() {
    let provider = RetryStubProvider {
        calls: Arc::new(AtomicUsize::new(0)),
        mode: RetryMode::Fatal,
    };

    let err = provider
        .chat_with_retry(vec![], vec![], "stub-model")
        .await
        .expect_err("fatal failures should remain errors");

    assert!(err.to_string().contains("fatal stub failure"));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn chat_with_retry_propagates_exhausted_retryable_errors() {
    let provider = RetryStubProvider {
        calls: Arc::new(AtomicUsize::new(0)),
        mode: RetryMode::AlwaysRetryable,
    };
    let calls = provider.calls.clone();

    let err = provider
        .chat_with_retry(vec![], vec![], "stub-model")
        .await
        .expect_err("exhausted retries should remain errors");

    assert!(err.to_string().contains("transient stub failure"));
    assert_eq!(calls.load(Ordering::SeqCst), 4);
}
