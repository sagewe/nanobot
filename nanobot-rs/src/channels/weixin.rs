use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeixinAccountState {
    pub bot_token: String,
    pub ilink_bot_id: String,
    pub baseurl: String,
    pub ilink_user_id: Option<String>,
    pub get_updates_buf: String,
    pub status: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeixinLoginSession {
    pub account: WeixinAccountState,
    #[serde(default)]
    pub context_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WeixinAccountStore {
    dir: PathBuf,
}

impl WeixinAccountStore {
    pub fn new(workspace: &Path) -> Result<Self> {
        let dir = workspace.join("channels").join("weixin");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;
        Ok(Self { dir })
    }

    pub fn account_path(&self) -> PathBuf {
        self.dir.join("account.json")
    }

    fn context_tokens_path(&self) -> PathBuf {
        self.dir.join("context_tokens.json")
    }

    pub fn load_account(&self) -> Result<Option<WeixinAccountState>> {
        let path = self.account_path();
        if !path.exists() {
            return Ok(None);
        }
        let account = read_json::<WeixinAccountState>(&path)?;
        Ok(Some(account))
    }

    pub fn save_account(&self, account: &WeixinAccountState) -> Result<()> {
        write_json(&self.account_path(), account)
    }

    pub fn clear_account(&self) -> Result<()> {
        remove_if_exists(&self.account_path())
    }

    pub fn load_context_token(&self, peer_user_id: &str) -> Result<Option<String>> {
        let path = self.context_tokens_path();
        if !path.exists() {
            return Ok(None);
        }
        let tokens = read_json::<BTreeMap<String, String>>(&path)?;
        Ok(tokens.get(peer_user_id).cloned())
    }

    pub fn save_context_token(&self, peer_user_id: &str, token: &str) -> Result<()> {
        let mut tokens = if self.context_tokens_path().exists() {
            read_json::<BTreeMap<String, String>>(&self.context_tokens_path())?
        } else {
            BTreeMap::new()
        };
        tokens.insert(peer_user_id.to_string(), token.to_string());
        write_json(&self.context_tokens_path(), &tokens)
    }

    pub fn clear_all(&self) -> Result<()> {
        self.clear_account()?;
        remove_if_exists(&self.context_tokens_path())
    }
}

fn read_json<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let value = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(value)
}

fn write_json<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(value).context("failed to serialize json")?;
    std::fs::write(path, raw).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}
