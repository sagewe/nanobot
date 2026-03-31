use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};
use serde::de::Error as _;
use serde_json::{Map, Value};

use crate::providers::ProviderRegistry;

#[derive(Debug, Clone)]
pub struct AgentDefaults {
    pub workspace: String,
    pub default_profile: String,
    pub max_tool_iterations: usize,
    pub message_debounce_ms: u64,
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
            message_debounce_ms: 0,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default = "default_codex_service_tier")]
    pub service_tier: Option<String>,
}

fn default_codex_service_tier() -> Option<String> {
    None
}

impl Default for CodexProviderConfig {
    fn default() -> Self {
        Self {
            auth_file: default_codex_auth_file(),
            api_base: default_codex_api_base(),
            service_tier: default_codex_service_tier(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawAgentDefaults {
    pub workspace: String,
    pub max_tool_iterations: usize,
    pub message_debounce_ms: u64,
    pub default_profile: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

impl Default for RawAgentDefaults {
    fn default() -> Self {
        Self {
            workspace: default_workspace_path().display().to_string(),
            max_tool_iterations: 20,
            message_debounce_ms: 0,
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
            message_debounce_ms,
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
                    message_debounce_ms,
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

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        RawConfig::deserialize(deserializer)?
            .into_config()
            .map_err(D::Error::custom)
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
    message_debounce_ms: u64,
}

impl<'a> From<&'a AgentDefaults> for SerializableAgentDefaults<'a> {
    fn from(defaults: &'a AgentDefaults) -> Self {
        Self {
            workspace: &defaults.workspace,
            default_profile: &defaults.default_profile,
            max_tool_iterations: defaults.max_tool_iterations,
            message_debounce_ms: defaults.message_debounce_ms,
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
pub struct HeartbeatConfig {
    pub enabled: bool,
    pub interval_s: u64,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_s: 30 * 60,
        }
    }
}

/// Configuration for a single MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct McpServerConfig {
    /// Transport type: "stdio", "sse", or "streamableHttp".
    /// Inferred from `command`/`url` when absent.
    #[serde(rename = "type")]
    pub transport_type: Option<String>,
    /// Command to spawn for stdio transport.
    pub command: Option<String>,
    /// Arguments for the stdio command.
    pub args: Vec<String>,
    /// Extra environment variables for the stdio process.
    pub env: HashMap<String, String>,
    /// URL for SSE / streamable-HTTP transport.
    pub url: Option<String>,
    /// Additional HTTP headers (SSE / streamable-HTTP only).
    pub headers: HashMap<String, String>,
    /// Tool names (raw or `mcp_{server}_{name}`) to expose.
    /// `["*"]` (the default) exposes all tools.
    pub enabled_tools: Vec<String>,
    /// Per-call timeout in seconds.
    pub tool_timeout: u64,
    /// Optional icon for display in the UI (emoji, URL, or data URI).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            transport_type: None,
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            url: None,
            headers: HashMap::new(),
            enabled_tools: vec!["*".to_string()],
            tool_timeout: 30,
            icon: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ToolsConfig {
    pub exec: ExecToolConfig,
    pub restrict_to_workspace: bool,
    pub web: WebToolsConfig,
    pub heartbeat: HeartbeatConfig,
    /// MCP server connections.  Keys are arbitrary server names.
    pub mcp: HashMap<String, McpServerConfig>,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            exec: ExecToolConfig::default(),
            restrict_to_workspace: false,
            web: WebToolsConfig::default(),
            heartbeat: HeartbeatConfig::default(),
            mcp: HashMap::new(),
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
        .join("config.toml")
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
    let config_path = resolve_config_path(path);
    if !config_path.exists() {
        return Ok(Config::default());
    }
    let raw = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read config {}", config_path.display()))?;
    let raw_config: RawConfig = parse_config_str(&config_path, &raw)?;
    raw_config
        .into_config()
        .map_err(|err| anyhow!("failed to validate config {}: {err}", config_path.display()))
}

pub fn load_config_from_str(path: &Path, raw: &str) -> Result<Config> {
    parse_config_str(path, raw)?
        .into_config()
        .map_err(|err| anyhow!("failed to validate config {}: {err}", path.display()))
}

fn resolve_config_path(path: Option<&Path>) -> PathBuf {
    if path.is_some() {
        return Config::config_path(path);
    }
    let base = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".nanobot-rs");
    let toml_path = base.join("config.toml");
    if toml_path.exists() {
        return toml_path;
    }
    let json_path = base.join("config.json");
    if json_path.exists() {
        return json_path;
    }
    toml_path
}

fn parse_config_str(path: &Path, raw: &str) -> Result<RawConfig> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("toml") => toml::from_str(raw)
            .map_err(|err| anyhow!("failed to parse config {}: {err}", path.display())),
        _ => serde_json::from_str(raw)
            .map_err(|err| anyhow!("failed to parse config {}: {err}", path.display())),
    }
}

pub fn save_config(config: &Config, path: Option<&Path>) -> Result<PathBuf> {
    let config_path = Config::config_path(path);
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let content = match config_path.extension().and_then(|e| e.to_str()) {
        Some("toml") => toml::to_string_pretty(config)
            .map_err(|err| anyhow!("failed to serialize config: {err}"))?,
        _ => serde_json::to_string_pretty(config)?,
    };
    if is_canonical_config_path(&config_path) {
        save_canonical_config_file(&config_path, &content)?;
    } else {
        std::fs::write(&config_path, content)
            .with_context(|| format!("failed to write config {}", config_path.display()))?;
    }
    Ok(config_path)
}

fn is_canonical_config_path(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("config.toml")
}

fn save_canonical_config_file(config_path: &Path, content: &str) -> Result<()> {
    let tmp_path = config_path.with_extension("toml.tmp");
    std::fs::write(&tmp_path, content)
        .with_context(|| format!("failed to write config {}", tmp_path.display()))?;
    replace_file(&tmp_path, config_path)?;

    let legacy_path = config_path.with_file_name("config.json");
    if legacy_path.exists() {
        std::fs::remove_file(&legacy_path)
            .with_context(|| format!("failed to remove {}", legacy_path.display()))?;
    }

    Ok(())
}

fn replace_file(from: &Path, to: &Path) -> Result<()> {
    replace_file_with(from, to, replace_file_impl)
}

fn replace_file_with(
    from: &Path,
    to: &Path,
    mut replace_impl: impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> Result<()> {
    replace_impl(from, to)
        .with_context(|| format!("failed to replace {} with {}", to.display(), from.display()))
}

#[cfg(not(windows))]
fn replace_file_impl(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

#[cfg(windows)]
fn replace_file_impl(from: &Path, to: &Path) -> std::io::Result<()> {
    windows_fs::replace_file(from, to)
}

#[cfg(windows)]
mod windows_fs {
    use std::ffi::OsStr;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(
            lpExistingFileName: *const u16,
            lpNewFileName: *const u16,
            dwFlags: u32,
        ) -> i32;
    }

    pub fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
        let from = to_wide(from.as_os_str());
        let to = to_wide(to.as_os_str());
        let ok = unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn to_wide(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }
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
    "https://chatgpt.com/backend-api/codex".to_string()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn replace_file_with_overwrites_existing_destination() {
        let dir = tempdir().expect("tempdir");
        let source = dir.path().join("config.toml.tmp");
        let target = dir.path().join("config.toml");
        fs::write(&source, "new").expect("write source");
        fs::write(&target, "old").expect("write target");

        let mut called = false;
        replace_file_with(&source, &target, |from, to| {
            called = true;
            fs::rename(from, to)
        })
        .expect("replace file");

        assert!(called);
        assert_eq!(fs::read_to_string(&target).expect("read target"), "new");
        assert!(!source.exists());
    }
}
