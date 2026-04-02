use std::env;
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Map, Value};
use sidekick::config::load_config;
use sidekick::config::save_config;
use sidekick::config::{Config, FeishuConfig};
use sidekick::providers::{
    CodexProvider, LlmProvider, LlmResponse, ProviderError, ProviderKind, ProviderPool,
    ProviderRegistry, ProviderRequestDescriptor,
};
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
    assert_eq!(
        value
            .pointer("/channels/feishu/enabled")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        value
            .pointer("/channels/feishu/apiBase")
            .and_then(Value::as_str),
        Some("https://open.feishu.cn/open-apis")
    );
    assert_eq!(
        value
            .pointer("/channels/feishu/wsBase")
            .and_then(Value::as_str),
        Some("wss://open.feishu.cn/open-apis/ws")
    );
    assert_eq!(
        value
            .pointer("/channels/feishu/reactEmoji")
            .and_then(Value::as_str),
        Some("THUMBSUP")
    );
    assert_eq!(
        value
            .pointer("/channels/feishu/groupPolicy")
            .and_then(Value::as_str),
        Some("mention")
    );
    assert_eq!(
        value
            .pointer("/channels/feishu/replyToMessage")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert!(value.pointer("/channels/feishu/app_id").is_none());
}

#[test]
fn sparse_feishu_config_keeps_legacy_shape_and_fills_new_defaults() {
    let value = serde_json::json!({
        "enabled": true,
        "appId": "cli_legacy",
        "appSecret": "secret",
        "apiBase": "https://open.feishu.cn/open-apis",
        "allowFrom": ["*"]
    });

    let config: FeishuConfig = serde_json::from_value(value).expect("legacy config");
    assert!(config.enabled);
    assert_eq!(config.app_id, "cli_legacy");
    assert_eq!(config.app_secret, "secret");
    assert_eq!(config.api_base, "https://open.feishu.cn/open-apis");
    assert_eq!(config.allow_from, vec!["*".to_string()]);
    assert_eq!(config.ws_base, "wss://open.feishu.cn/open-apis/ws");
    assert_eq!(config.encrypt_key, "");
    assert_eq!(config.verification_token, "");
    assert_eq!(config.react_emoji, "THUMBSUP");
    assert_eq!(config.group_policy, "mention");
    assert!(!config.reply_to_message);
}

#[test]
fn load_config_prefers_toml_when_both_files_exist() {
    let home_dir = tempdir().expect("tempdir");
    let config_dir = home_dir.path().join(".sidekick");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join("config.toml"),
        r#"[agents.defaults]
workspace = "/tmp/sidekick-home"
defaultProfile = "openrouter:anthropic/claude-sonnet-4"
maxToolIterations = 20
messageDebounceMs = 0

[agents.profiles."openrouter:anthropic/claude-sonnet-4"]
provider = "openrouter"
model = "anthropic/claude-sonnet-4"
request = {}
"#,
    )
    .expect("write toml config");
    fs::write(
        config_dir.join("config.json"),
        r#"{
  "agents": {
    "defaults": {
      "workspace": "/tmp/sidekick",
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
            "openrouter:anthropic/claude-sonnet-4"
        );
        assert_eq!(config.agents.defaults.provider, "openrouter");
        assert_eq!(config.agents.defaults.model, "anthropic/claude-sonnet-4");
        assert_eq!(config.agents.defaults.workspace, "/tmp/sidekick-home");
    });
}

#[test]
fn save_config_to_toml_replaces_stale_json_copy() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    fs::write(dir.path().join("config.json"), "{}").expect("write stale json");

    let written = save_config(&Config::default(), Some(&path)).expect("save config");

    assert_eq!(written, path);
    assert!(path.exists());
    assert!(!dir.path().join("config.json").exists());
    assert!(!dir.path().join("config.toml.tmp").exists());
}

#[test]
fn save_config_to_existing_toml_path_updates_content_on_second_write() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    let mut config = Config::default();

    config.agents.defaults.workspace = "/tmp/first".to_string();
    save_config(&config, Some(&path)).expect("first save");

    config.agents.defaults.workspace = "/tmp/second".to_string();
    save_config(&config, Some(&path)).expect("second save");

    let raw = fs::read_to_string(&path).expect("read config");
    let value: toml::Value = raw.parse().expect("parse toml");
    let workspace = value
        .get("agents")
        .and_then(toml::Value::as_table)
        .and_then(|agents| agents.get("defaults"))
        .and_then(toml::Value::as_table)
        .and_then(|defaults| defaults.get("workspace"))
        .and_then(toml::Value::as_str);

    assert_eq!(workspace, Some("/tmp/second"));
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
      "workspace": "/tmp/sidekick",
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
      "workspace": "/tmp/sidekick",
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
      "workspace": "/tmp/sidekick",
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
fn codex_auth_summary_reports_ready_state_for_valid_auth_file() {
    let dir = tempdir().expect("tempdir");
    let auth_path = dir.path().join("codex-auth.json");
    fs::write(
        &auth_path,
        serde_json::json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "access_token": "access-token",
                "refresh_token": "refresh-token",
                "id_token": "id-token",
                "account_id": "acct-ready"
            }
        })
        .to_string(),
    )
    .expect("write auth");

    let summary = CodexProvider::auth_summary(&sidekick::config::CodexProviderConfig {
        auth_file: auth_path.display().to_string(),
        api_base: "https://chatgpt.com/backend-api/codex".to_string(),
        service_tier: None,
    });

    assert!(summary.parse_valid);
    assert_eq!(summary.account_id.as_deref(), Some("acct-ready"));
    assert!(summary.error.is_none());
    assert_eq!(summary.auth_path, auth_path);
}

#[test]
fn codex_auth_summary_reports_errors_for_missing_and_malformed_auth_files() {
    let dir = tempdir().expect("tempdir");

    let missing_path = dir.path().join("missing-auth.json");
    let missing_summary = CodexProvider::auth_summary(&sidekick::config::CodexProviderConfig {
        auth_file: missing_path.display().to_string(),
        api_base: "https://chatgpt.com/backend-api/codex".to_string(),
        service_tier: None,
    });
    assert!(!missing_summary.parse_valid);
    assert!(missing_summary.account_id.is_none());
    assert!(
        missing_summary
            .error
            .as_deref()
            .is_some_and(|error| error.contains("failed to read codex auth file"))
    );
    assert_eq!(missing_summary.auth_path, missing_path);

    let malformed_path = dir.path().join("malformed-auth.json");
    fs::write(
        &malformed_path,
        serde_json::json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "access_token": "access-token",
                "refresh_token": "refresh-token"
            }
        })
        .to_string(),
    )
    .expect("write malformed auth");
    let malformed_summary = CodexProvider::auth_summary(&sidekick::config::CodexProviderConfig {
        auth_file: malformed_path.display().to_string(),
        api_base: "https://chatgpt.com/backend-api/codex".to_string(),
        service_tier: None,
    });
    assert!(!malformed_summary.parse_valid);
    assert!(malformed_summary.account_id.is_none());
    assert!(
        malformed_summary
            .error
            .as_deref()
            .is_some_and(|error| error.contains("missing required field"))
    );
    assert_eq!(malformed_summary.auth_path, malformed_path);
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
      "workspace": "/tmp/sidekick",
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
      "workspace": "/tmp/sidekick",
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
      "workspace": "/tmp/sidekick",
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
