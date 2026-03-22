use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};
use serde_json::{Map, Value};

use crate::providers::ProviderRegistry;

#[derive(Debug, Clone)]
pub struct AgentDefaults {
    pub workspace: String,
    pub default_profile: String,
    pub max_tool_iterations: usize,
    pub provider: String,
    pub model: String,
}

impl Default for AgentDefaults {
    fn default() -> Self {
        let provider = "openai".to_string();
        let model = "gpt-4.1-mini".to_string();
        Self {
            workspace: default_workspace_path().display().to_string(),
            default_profile: profile_key(&provider, &model),
            max_tool_iterations: 20,
            provider,
            model,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentsConfig {
    pub defaults: AgentDefaults,
    pub profiles: HashMap<String, AgentProfileConfig>,
}

impl Default for AgentsConfig {
    fn default() -> Self {
        let defaults = AgentDefaults::default();
        let mut profiles = HashMap::new();
        profiles.insert(
            defaults.default_profile.clone(),
            AgentProfileConfig {
                provider: defaults.provider.clone(),
                model: defaults.model.clone(),
                request: Map::new(),
            },
        );
        Self { defaults, profiles }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileConfig {
    pub provider: String,
    pub model: String,
    #[serde(default, deserialize_with = "deserialize_request_map")]
    pub request: Map<String, Value>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub agents: AgentsConfig,
    pub providers: ProvidersConfig,
    pub channels: ChannelsConfig,
    pub tools: ToolsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CodexProviderConfig {
    #[serde(default = "default_codex_auth_file")]
    pub auth_file: String,
    #[serde(default = "default_codex_api_base")]
    pub api_base: String,
}

impl Default for CodexProviderConfig {
    fn default() -> Self {
        Self {
            auth_file: default_codex_auth_file(),
            api_base: default_codex_api_base(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawAgentDefaults {
    pub workspace: String,
    pub max_tool_iterations: usize,
    pub default_profile: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

impl Default for RawAgentDefaults {
    fn default() -> Self {
        Self {
            workspace: default_workspace_path().display().to_string(),
            max_tool_iterations: 20,
            default_profile: None,
            provider: None,
            model: None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawAgentsConfig {
    pub defaults: RawAgentDefaults,
    pub profiles: HashMap<String, AgentProfileConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawConfig {
    pub agents: RawAgentsConfig,
    pub providers: RawProvidersConfig,
    pub channels: ChannelsConfig,
    pub tools: ToolsConfig,
}

impl RawConfig {
    fn into_config(self) -> Result<Config> {
        let RawConfig {
            agents,
            providers,
            channels,
            tools,
        } = self;
        let RawAgentsConfig { defaults, profiles } = agents;
        let RawAgentDefaults {
            workspace,
            max_tool_iterations,
            default_profile,
            provider,
            model,
        } = defaults;
        let registry = ProviderRegistry::default();
        let mut profiles = profiles;
        let sparse_default_profile = if profiles.is_empty() {
            Some(AgentDefaults::default())
        } else {
            None
        };

        let default_profile = match default_profile {
            Some(default_profile) => {
                if !profiles.contains_key(&default_profile) {
                    bail!(
                        "agents.defaults.defaultProfile '{}' does not match any configured profile",
                        default_profile
                    );
                }
                default_profile
            }
            None => {
                let provider = provider
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| {
                        sparse_default_profile
                            .as_ref()
                            .map(|defaults| defaults.provider.clone())
                    })
                    .ok_or_else(|| anyhow!("agents.defaults.defaultProfile is required"))?;
                let model = model
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| {
                        sparse_default_profile
                            .as_ref()
                            .map(|defaults| defaults.model.clone())
                    })
                    .ok_or_else(|| anyhow!("agents.defaults.defaultProfile is required"))?;
                let profile_name = profile_key(&provider, &model);
                let normalized_provider = registry
                    .resolve(&provider)
                    .with_context(|| {
                        format!(
                            "agents.defaults.defaultProfile '{profile_name}' uses unknown provider '{provider}'"
                        )
                    })?
                    .name
                    .to_string();
                match profiles.entry(profile_name.clone()) {
                    std::collections::hash_map::Entry::Occupied(entry) => {
                        let existing = entry.get();
                        let existing_provider = registry
                            .resolve(&existing.provider)
                            .with_context(|| {
                                format!(
                                    "agents.profiles.{profile_name}.provider '{}' is not a known provider",
                                    existing.provider
                                )
                            })?
                            .name
                            .to_string();
                        if existing_provider != normalized_provider || existing.model != model {
                            bail!(
                                "agents.profiles.{profile_name} must match legacy defaults.provider/model"
                            );
                        }
                    }
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(AgentProfileConfig {
                            provider,
                            model,
                            request: Map::new(),
                        });
                    }
                }
                profile_name
            }
        };

        for (profile_name, profile) in &profiles {
            registry.resolve(&profile.provider).with_context(|| {
                format!(
                    "agents.profiles.{profile_name}.provider '{}' is not a known provider",
                    profile.provider
                )
            })?;
        }

        let default_profile_config = profiles.get(&default_profile).with_context(|| {
            format!(
                "agents.defaults.defaultProfile '{default_profile}' does not match any configured profile"
            )
        })?;
        let codex_requested = profiles.values().any(|profile| {
            registry
                .resolve(&profile.provider)
                .map(|spec| spec.kind == crate::providers::ProviderKind::Codex)
                .unwrap_or(false)
        });
        let codex = match providers.codex {
            Some(codex) => codex,
            None if codex_requested => {
                bail!(
                    "agents.profiles or agents.defaults.defaultProfile reference provider 'codex' but providers.codex is missing"
                );
            }
            None => CodexProviderConfig::default(),
        };

        let workspace = if workspace.trim().is_empty() {
            default_workspace_path().display().to_string()
        } else {
            workspace
        };

        Ok(Config {
            agents: AgentsConfig {
                defaults: AgentDefaults {
                    workspace,
                    default_profile,
                    max_tool_iterations,
                    provider: default_profile_config.provider.clone(),
                    model: default_profile_config.model.clone(),
                },
                profiles,
            },
            providers: ProvidersConfig {
                openai: providers.openai,
                custom: providers.custom,
                openrouter: providers.openrouter,
                ollama: providers.ollama,
                codex,
            },
            channels,
            tools,
        })
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            agents: AgentsConfig::default(),
            providers: ProvidersConfig::default(),
            channels: ChannelsConfig::default(),
            tools: ToolsConfig::default(),
        }
    }
}

impl Serialize for Config {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Config", 4)?;
        state.serialize_field(
            "agents",
            &SerializableAgentsConfig {
                defaults: SerializableAgentDefaults::from(&self.agents.defaults),
                profiles: &self.agents.profiles,
            },
        )?;
        state.serialize_field("providers", &self.providers)?;
        state.serialize_field("channels", &self.channels)?;
        state.serialize_field("tools", &self.tools)?;
        state.end()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializableAgentsConfig<'a> {
    defaults: SerializableAgentDefaults<'a>,
    profiles: &'a HashMap<String, AgentProfileConfig>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializableAgentDefaults<'a> {
    workspace: &'a str,
    default_profile: &'a str,
    max_tool_iterations: usize,
}

impl<'a> From<&'a AgentDefaults> for SerializableAgentDefaults<'a> {
    fn from(defaults: &'a AgentDefaults) -> Self {
        Self {
            workspace: &defaults.workspace,
            default_profile: &defaults.default_profile,
            max_tool_iterations: defaults.max_tool_iterations,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderConfig {
    pub api_key: String,
    pub api_base: String,
    pub extra_headers: HashMap<String, String>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self::with_base("https://api.openai.com/v1")
    }
}

impl ProviderConfig {
    pub fn with_base(api_base: impl Into<String>) -> Self {
        Self {
            api_key: String::new(),
            api_base: api_base.into(),
            extra_headers: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProvidersConfig {
    pub openai: ProviderConfig,
    pub custom: ProviderConfig,
    pub openrouter: ProviderConfig,
    pub ollama: ProviderConfig,
    pub codex: CodexProviderConfig,
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        Self {
            openai: ProviderConfig::with_base("https://api.openai.com/v1"),
            custom: ProviderConfig::with_base("http://localhost:8000/v1"),
            openrouter: ProviderConfig::with_base("https://openrouter.ai/api/v1"),
            ollama: ProviderConfig::with_base("http://localhost:11434/v1"),
            codex: CodexProviderConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawProvidersConfig {
    pub openai: ProviderConfig,
    pub custom: ProviderConfig,
    pub openrouter: ProviderConfig,
    pub ollama: ProviderConfig,
    pub codex: Option<CodexProviderConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TelegramConfig {
    pub enabled: bool,
    pub token: String,
    pub allow_from: Vec<String>,
    #[serde(default = "default_telegram_api_base")]
    pub api_base: String,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            token: String::new(),
            allow_from: Vec::new(),
            api_base: default_telegram_api_base(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WeixinConfig {
    pub enabled: bool,
    #[serde(default = "default_weixin_api_base")]
    pub api_base: String,
    #[serde(default = "default_weixin_cdn_base")]
    pub cdn_base: String,
}

impl Default for WeixinConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_base: default_weixin_api_base(),
            cdn_base: default_weixin_cdn_base(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WecomConfig {
    pub enabled: bool,
    pub bot_id: String,
    pub secret: String,
    #[serde(default = "default_wecom_ws_base")]
    pub ws_base: String,
    pub allow_from: Vec<String>,
}

impl Default for WecomConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: String::new(),
            secret: String::new(),
            ws_base: default_wecom_ws_base(),
            allow_from: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChannelsConfig {
    pub send_progress: bool,
    pub send_tool_hints: bool,
    pub telegram: TelegramConfig,
    pub weixin: WeixinConfig,
    pub wecom: WecomConfig,
}

impl Default for ChannelsConfig {
    fn default() -> Self {
        Self {
            send_progress: true,
            send_tool_hints: false,
            telegram: TelegramConfig::default(),
            weixin: WeixinConfig::default(),
            wecom: WecomConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExecToolConfig {
    pub timeout: u64,
}

impl Default for ExecToolConfig {
    fn default() -> Self {
        Self { timeout: 60 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WebSearchToolConfig {
    pub provider: String,
    pub api_key: String,
    pub base_url: String,
    pub max_results: usize,
}

impl Default for WebSearchToolConfig {
    fn default() -> Self {
        Self {
            provider: "duckduckgo".to_string(),
            api_key: String::new(),
            base_url: String::new(),
            max_results: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WebFetchToolConfig {
    pub max_chars: usize,
}

impl Default for WebFetchToolConfig {
    fn default() -> Self {
        Self { max_chars: 20_000 }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WebToolsConfig {
    pub search: WebSearchToolConfig,
    pub fetch: WebFetchToolConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ToolsConfig {
    pub exec: ExecToolConfig,
    pub restrict_to_workspace: bool,
    pub web: WebToolsConfig,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            exec: ExecToolConfig::default(),
            restrict_to_workspace: false,
            web: WebToolsConfig::default(),
        }
    }
}

impl Config {
    pub fn workspace_path(&self) -> PathBuf {
        expand_tilde(Path::new(&self.agents.defaults.workspace))
    }

    pub fn config_path(user_path: Option<&Path>) -> PathBuf {
        user_path
            .map(Path::to_path_buf)
            .unwrap_or_else(default_config_path)
    }
}

pub fn default_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".nanobot-rs")
        .join("config.json")
}

pub fn default_workspace_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".nanobot-rs")
        .join("workspace")
}

pub fn expand_tilde(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if raw == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    }
    if let Some(stripped) = raw.strip_prefix("~/") {
        return dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(stripped);
    }
    path.to_path_buf()
}

pub fn load_config(path: Option<&Path>) -> Result<Config> {
    let config_path = Config::config_path(path);
    if !config_path.exists() {
        return Ok(Config::default());
    }
    let raw = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read config {}", config_path.display()))?;
    let raw_config: RawConfig = serde_json::from_str(&raw)
        .map_err(|err| anyhow!("failed to parse config {}: {err}", config_path.display()))?;
    raw_config
        .into_config()
        .map_err(|err| anyhow!("failed to validate config {}: {err}", config_path.display()))
}

pub fn save_config(config: &Config, path: Option<&Path>) -> Result<PathBuf> {
    let config_path = Config::config_path(path);
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(config)?;
    std::fs::write(&config_path, json)
        .with_context(|| format!("failed to write config {}", config_path.display()))?;
    Ok(config_path)
}

fn default_telegram_api_base() -> String {
    "https://api.telegram.org".to_string()
}

fn default_weixin_api_base() -> String {
    "https://ilinkai.weixin.qq.com".to_string()
}

fn default_weixin_cdn_base() -> String {
    "https://novac2c.cdn.weixin.qq.com/c2c".to_string()
}

fn default_wecom_ws_base() -> String {
    "wss://openws.work.weixin.qq.com".to_string()
}

fn default_codex_auth_file() -> String {
    "~/.codex/auth.json".to_string()
}

fn default_codex_api_base() -> String {
    "https://chatgpt.com/backend-api".to_string()
}

fn profile_key(provider: &str, model: &str) -> String {
    format!("{provider}:{model}")
}

fn deserialize_request_map<'de, D>(
    deserializer: D,
) -> std::result::Result<Map<String, Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Object(map) => Ok(map),
        other => Err(serde::de::Error::custom(format!(
            "agents.profiles[*].request must be a JSON object, got {other}"
        ))),
    }
}
