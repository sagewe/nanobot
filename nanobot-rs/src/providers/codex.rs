use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use crate::config::CodexProviderConfig;

#[derive(Debug, Clone)]
pub struct CodexProvider {
    config: CodexProviderConfig,
    auth: CodexAuthFile,
    auth_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
struct CodexAuthFile {
    auth_mode: String,
    tokens: CodexAuthTokens,
}

#[derive(Debug, Clone, Deserialize)]
struct CodexAuthTokens {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
}

impl CodexProvider {
    pub fn from_config(config: CodexProviderConfig) -> Result<Self> {
        let auth_path = resolve_auth_path(&config.auth_file)?;
        let auth = load_auth_file(&auth_path)?;
        Ok(Self {
            config,
            auth,
            auth_path,
        })
    }

    #[allow(dead_code)]
    pub fn auth_path(&self) -> &Path {
        &self.auth_path
    }

    #[allow(dead_code)]
    pub fn api_base(&self) -> &str {
        &self.config.api_base
    }

    #[allow(dead_code)]
    fn auth(&self) -> &CodexAuthFile {
        &self.auth
    }
}

fn resolve_auth_path(raw_path: &str) -> Result<PathBuf> {
    let path = raw_path.trim();
    if path.is_empty() {
        bail!("codex auth file path must not be empty");
    }
    if path == "~" {
        return home_dir()
            .ok_or_else(|| anyhow!("failed to resolve home directory for codex auth file"));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return Ok(home_dir()
            .ok_or_else(|| anyhow!("failed to resolve home directory for codex auth file"))?
            .join(rest));
    }
    Ok(PathBuf::from(path))
}

fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

fn load_auth_file(path: &Path) -> Result<CodexAuthFile> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read codex auth file at {}", path.display()))?;
    let auth: CodexAuthFile = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse codex auth file at {}", path.display()))?;

    validate_auth(&auth)?;
    Ok(auth)
}

fn validate_auth(auth: &CodexAuthFile) -> Result<()> {
    if auth.auth_mode != "chatgpt" {
        bail!(
            "codex auth file auth_mode must be 'chatgpt' (found '{}')",
            auth.auth_mode
        );
    }

    validate_required_token("access_token", auth.tokens.access_token.as_deref())?;
    validate_required_token("refresh_token", auth.tokens.refresh_token.as_deref())?;
    validate_required_token("id_token", auth.tokens.id_token.as_deref())?;
    validate_required_token("account_id", auth.tokens.account_id.as_deref())?;
    Ok(())
}

fn validate_required_token(field: &str, value: Option<&str>) -> Result<()> {
    let value = value.ok_or_else(|| anyhow!("codex auth file missing required field '{field}'"))?;
    if value.trim().is_empty() {
        bail!("codex auth file {field} must not be empty");
    }
    Ok(())
}
