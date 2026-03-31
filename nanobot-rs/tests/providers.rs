use std::env;
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use anyhow::Result;
use async_trait::async_trait;
use nanobot_rs::config::Config;
use nanobot_rs::config::default_config_path;
use nanobot_rs::config::load_config;
use nanobot_rs::config::save_config;
use nanobot_rs::providers::{
    LlmProvider, LlmResponse, ProviderError, ProviderKind, ProviderPool, ProviderRegistry,
    ProviderRequestDescriptor,
};
use serde_json::{Map, Value};
use tempfile::tempdir;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn with_home_dir<F>(home_dir: &std::path::Path, f: F)
where
    F: FnOnce(),
{
    let _guard = env_lock().lock().expect("env lock");
    let previous_home = env::var_os("HOME");

    unsafe {
        env::set_var("HOME", home_dir);
    }

    f();

    match previous_home {
        Some(previous) => unsafe {
            env::set_var("HOME", previous);
        },
        None => unsafe {
            env::remove_var("HOME");
        },
    }
}

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
            .pointer("/agents/defaults/messageDebounceMs")
            .and_then(Value::as_u64),
        Some(0)
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
fn load_config_prefers_toml_when_both_files_exist() {
    let home_dir = tempdir().expect("tempdir");
    let config_dir = home_dir.path().join(".nanobot-rs");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join("config.toml"),
        r#"[agents.defaults]
workspace = "/tmp/nanobot"
defaultProfile = "openai:gpt-4.1-mini"
maxToolIterations = 20
messageDebounceMs = 0

[agents.profiles."openai:gpt-4.1-mini"]
provider = "openai"
model = "gpt-4.1-mini"
request = {}
"#,
    )
    .expect("write toml config");
    fs::write(
        config_dir.join("config.json"),
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/nanobot",
      "defaultProfile": "codex:gpt-5.4",
      "maxToolIterations": 20
    },
    "profiles": {
      "codex:gpt-5.4": {
        "provider": "codex",
        "model": "gpt-5.4",
        "request": {}
      }
    }
  }
}"#,
    )
    .expect("write json config");

    with_home_dir(home_dir.path(), || {
        let config = load_config(None).expect("load config");
        assert_eq!(
            config.agents.defaults.default_profile,
            "openai:gpt-4.1-mini"
        );
        assert_eq!(config.agents.defaults.provider, "openai");
        assert_eq!(config.agents.defaults.model, "gpt-4.1-mini");
    });
}

#[test]
fn save_config_to_toml_replaces_stale_json_copy() {
    let dir = tempdir().expect("tempdir");
    with_home_dir(dir.path(), || {
        let path = default_config_path();
        fs::create_dir_all(path.parent().expect("config parent")).expect("create config dir");
        fs::write(path.with_file_name("config.json"), "{}").expect("write stale json");

        let written = save_config(&Config::default(), None).expect("save config");

        assert_eq!(written, path);
        assert!(path.exists());
        assert!(!path.with_file_name("config.json").exists());
    });
}

#[test]
fn save_config_to_noncanonical_toml_preserves_config_json() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("custom.toml");
    fs::write(dir.path().join("config.json"), "{}").expect("write json sibling");

    let written = save_config(&Config::default(), Some(&path)).expect("save config");

    assert_eq!(written, path);
    assert!(path.exists());
    assert!(dir.path().join("config.json").exists());
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
    assert_eq!(
        codex.default_api_base,
        "https://chatgpt.com/backend-api/codex"
    );
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

#[tokio::test]
async fn provider_pool_routes_codex_profiles_to_codex_provider() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("config.json");
    let missing_auth_path = dir.path().join("missing-codex-auth.json");
    fs::write(
        &path,
        format!(
            r#"{{
  "agents": {{
    "defaults": {{
      "workspace": "/tmp/nanobot",
      "defaultProfile": "codex:gpt-5.4",
      "maxToolIterations": 20
    }},
    "profiles": {{
      "codex:gpt-5.4": {{
        "provider": "codex",
        "model": "gpt-5.4",
        "request": {{}}
      }}
    }}
  }},
  "providers": {{
    "codex": {{
      "authFile": "{}",
      "apiBase": "https://chatgpt.com/backend-api"
    }}
  }}
}}"#,
            missing_auth_path.display()
        ),
    )
    .expect("write config");

    let config = load_config(Some(&path)).expect("load config");
    let pool = ProviderPool::new(config);
    let request = ProviderRequestDescriptor::new("codex", "gpt-5.4", Map::new());

    let err = pool
        .chat_with_request(vec![], vec![], &request)
        .await
        .expect_err("missing auth file should fail");

    assert!(err.to_string().contains("auth file"), "{err}");
}

#[tokio::test]
async fn provider_pool_default_chat_routes_codex_default_profile_to_codex_provider() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("config.json");
    let missing_auth_path = dir.path().join("missing-codex-auth.json");
    fs::write(
        &path,
        format!(
            r#"{{
  "agents": {{
    "defaults": {{
      "workspace": "/tmp/nanobot",
      "defaultProfile": "codex:gpt-5.4",
      "maxToolIterations": 20
    }},
    "profiles": {{
      "codex:gpt-5.4": {{
        "provider": "codex",
        "model": "gpt-5.4",
        "request": {{}}
      }}
    }}
  }},
  "providers": {{
    "codex": {{
      "authFile": "{}",
      "apiBase": "https://chatgpt.com/backend-api"
    }}
  }}
}}"#,
            missing_auth_path.display()
        ),
    )
    .expect("write config");

    let config = load_config(Some(&path)).expect("load config");
    let pool = ProviderPool::new(config);

    let err = pool
        .chat(vec![], vec![], "gpt-5.4")
        .await
        .expect_err("missing auth file should fail");

    assert!(err.to_string().contains("auth file"), "{err}");
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
        Some("https://chatgpt.com/backend-api/codex")
    );
    assert!(value.pointer("/providers/codex/serviceTier").is_none());
}

#[test]
fn save_config_toml_template_omits_legacy_default_provider_fields() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    let config = Config::default();

    let written = save_config(&config, Some(&path)).expect("save config");
    assert_eq!(written, path);

    let raw = fs::read_to_string(&path).expect("read toml config");
    let value: toml::Value = raw.parse().expect("parse toml config");
    let defaults = value
        .get("agents")
        .and_then(toml::Value::as_table)
        .and_then(|agents| agents.get("defaults"))
        .and_then(toml::Value::as_table)
        .expect("agents.defaults table");
    assert!(defaults.get("provider").is_none());
    assert!(defaults.get("model").is_none());
    assert!(
        value
            .get("providers")
            .and_then(toml::Value::as_table)
            .and_then(|providers| providers.get("codex"))
            .and_then(toml::Value::as_table)
            .and_then(|codex| codex.get("serviceTier"))
            .is_none()
    );
}

#[test]
fn provider_registry_builds_codex_configs_with_the_correct_default_base() {
    let registry = ProviderRegistry::default();
    let config = Config::default();

    let built = registry
        .build_config_for_provider(&config, "codex", "gpt-5-codex")
        .expect("build codex config");

    assert_eq!(built.kind, ProviderKind::Codex);
    assert_eq!(built.api_base, "https://chatgpt.com/backend-api/codex");
    assert_eq!(built.default_model, "gpt-5-codex");
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
