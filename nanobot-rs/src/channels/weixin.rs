use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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

#[derive(Debug)]
pub struct WeixinAccountStore {
    dir: PathBuf,
    workspace_lock: Arc<Mutex<()>>,
}

impl WeixinAccountStore {
    pub fn new(workspace: &Path) -> Result<Self> {
        let dir = workspace.join("channels").join("weixin");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;
        let workspace_lock = workspace_lock(&dir)?;
        Ok(Self {
            dir,
            workspace_lock,
        })
    }

    fn account_path(&self) -> PathBuf {
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
        let _guard = self
            .workspace_lock
            .lock()
            .map_err(|_| anyhow!("weixin workspace lock poisoned"))?;
        write_json(&self.account_path(), account)
    }

    pub fn clear_account(&self) -> Result<()> {
        let _guard = self
            .workspace_lock
            .lock()
            .map_err(|_| anyhow!("weixin workspace lock poisoned"))?;
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
        let _guard = self
            .workspace_lock
            .lock()
            .map_err(|_| anyhow!("weixin workspace lock poisoned"))?;
        let mut tokens = if self.context_tokens_path().exists() {
            read_json::<BTreeMap<String, String>>(&self.context_tokens_path())?
        } else {
            BTreeMap::new()
        };
        tokens.insert(peer_user_id.to_string(), token.to_string());
        write_json(&self.context_tokens_path(), &tokens)
    }

    pub fn clear_all(&self) -> Result<()> {
        let _guard = self
            .workspace_lock
            .lock()
            .map_err(|_| anyhow!("weixin workspace lock poisoned"))?;
        remove_if_exists(&self.account_path())?;
        remove_if_exists(&self.context_tokens_path())
    }
}

fn workspace_lock(dir: &Path) -> Result<Arc<Mutex<()>>> {
    static WORKSPACE_LOCKS: OnceLock<Mutex<std::collections::HashMap<PathBuf, Arc<Mutex<()>>>>> =
        OnceLock::new();

    let mut locks = WORKSPACE_LOCKS
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .map_err(|_| anyhow!("weixin workspace lock registry poisoned"))?;
    Ok(locks
        .entry(dir.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone())
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
    let temp_path = path.with_extension(format!(
        "{}.tmp-{}",
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("json"),
        Uuid::new_v4()
    ));
    {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }
        let mut file = options
            .open(&temp_path)
            .with_context(|| format!("failed to create {}", temp_path.display()))?;
        file.write_all(raw.as_bytes())
            .with_context(|| format!("failed to write {}", temp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync {}", temp_path.display()))?;
    }
    std::fs::rename(&temp_path, path).with_context(|| {
        let _ = std::fs::remove_file(&temp_path);
        format!(
            "failed to atomically replace {} with {}",
            path.display(),
            temp_path.display()
        )
    })?;
    #[cfg(unix)]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to set permissions on {}", path.display()))?;
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove {}", path.display())),
    }
}
