use std::collections::HashMap;

use nanobot_rs::config::Config;
use nanobot_rs::providers::{ProviderKind, ProviderRegistry};

#[test]
fn config_defaults_include_explicit_provider_and_web_settings() {
    let config = Config::default();

    assert_eq!(config.agents.defaults.provider, "openai");
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
}

#[test]
fn provider_registry_builds_provider_configs_with_defaults() {
    let mut config = Config::default();
    config.agents.defaults.model = "demo-model".to_string();
    config.agents.defaults.provider = "ollama".to_string();

    let registry = ProviderRegistry::default();
    let built = registry.build_config(&config).expect("build config");

    assert_eq!(built.kind, ProviderKind::Ollama);
    assert_eq!(built.api_base, "http://localhost:11434/v1");
    assert_eq!(built.default_model, "demo-model");
    assert!(built.api_key.is_empty());
}

#[test]
fn provider_registry_preserves_custom_extra_headers() {
    let mut config = Config::default();
    config.agents.defaults.provider = "custom".to_string();
    config.providers.custom.api_base = "https://models.example.test/v1".to_string();
    config.providers.custom.api_key = "secret".to_string();
    config.providers.custom.extra_headers = HashMap::from([
        ("X-Trace".to_string(), "abc123".to_string()),
        (
            "HTTP-Referer".to_string(),
            "https://example.test".to_string(),
        ),
    ]);

    let registry = ProviderRegistry::default();
    let built = registry.build_config(&config).expect("build config");

    assert_eq!(built.kind, ProviderKind::Custom);
    assert_eq!(built.api_base, "https://models.example.test/v1");
    assert_eq!(built.api_key, "secret");
    assert_eq!(built.extra_headers["X-Trace"], "abc123");
    assert_eq!(built.extra_headers["HTTP-Referer"], "https://example.test");
}
