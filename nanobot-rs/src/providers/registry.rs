use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};

use crate::config::{Config, ProviderConfig};

use super::{LlmProvider, OpenAICompatibleProvider};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    OpenAi,
    Custom,
    OpenRouter,
    Ollama,
}

#[derive(Debug, Clone)]
pub struct ProviderSpec {
    pub kind: ProviderKind,
    pub name: &'static str,
    pub default_api_base: &'static str,
    pub requires_api_key: bool,
    pub default_headers: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedProviderConfig {
    pub kind: ProviderKind,
    pub name: String,
    pub api_key: String,
    pub api_base: String,
    pub default_model: String,
    pub extra_headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderRegistry;

impl ProviderRegistry {
    pub fn resolve(&self, name: &str) -> Result<ProviderSpec> {
        match name.trim().to_ascii_lowercase().as_str() {
            "openai" => Ok(ProviderSpec {
                kind: ProviderKind::OpenAi,
                name: "openai",
                default_api_base: "https://api.openai.com/v1",
                requires_api_key: true,
                default_headers: HashMap::new(),
            }),
            "custom" => Ok(ProviderSpec {
                kind: ProviderKind::Custom,
                name: "custom",
                default_api_base: "http://localhost:8000/v1",
                requires_api_key: false,
                default_headers: HashMap::new(),
            }),
            "openrouter" => Ok(ProviderSpec {
                kind: ProviderKind::OpenRouter,
                name: "openrouter",
                default_api_base: "https://openrouter.ai/api/v1",
                requires_api_key: true,
                default_headers: HashMap::new(),
            }),
            "ollama" => Ok(ProviderSpec {
                kind: ProviderKind::Ollama,
                name: "ollama",
                default_api_base: "http://localhost:11434/v1",
                requires_api_key: false,
                default_headers: HashMap::new(),
            }),
            other => bail!("unknown provider '{other}'"),
        }
    }

    pub fn build_config(&self, config: &Config) -> Result<ResolvedProviderConfig> {
        let profile_name = &config.agents.defaults.default_profile;
        let profile = config.agents.profiles.get(profile_name).with_context(|| {
            format!(
                "agents.defaults.defaultProfile '{profile_name}' does not match any configured profile"
            )
        })?;
        let spec = self.resolve(&profile.provider)?;
        let provider_config = select_provider_config(config, spec.kind);
        let api_base = if provider_config.api_base.trim().is_empty() {
            spec.default_api_base.to_string()
        } else {
            provider_config.api_base.clone()
        };
        if spec.requires_api_key && provider_config.api_key.trim().is_empty() {
            return Err(anyhow!("provider '{}' requires apiKey", spec.name));
        }
        let mut extra_headers = spec.default_headers.clone();
        extra_headers.extend(provider_config.extra_headers.clone());
        Ok(ResolvedProviderConfig {
            kind: spec.kind,
            name: spec.name.to_string(),
            api_key: provider_config.api_key.clone(),
            api_base,
            default_model: profile.model.clone(),
            extra_headers,
        })
    }

    pub fn build_provider(&self, config: &Config) -> Result<Arc<dyn LlmProvider>> {
        let provider_config = self.build_config(config)?;
        Ok(Arc::new(OpenAICompatibleProvider::from_config(
            provider_config,
        )?))
    }
}

fn select_provider_config<'a>(config: &'a Config, kind: ProviderKind) -> &'a ProviderConfig {
    match kind {
        ProviderKind::OpenAi => &config.providers.openai,
        ProviderKind::Custom => &config.providers.custom,
        ProviderKind::OpenRouter => &config.providers.openrouter,
        ProviderKind::Ollama => &config.providers.ollama,
    }
}
