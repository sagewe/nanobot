use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::channels::weixin::WeixinAccountStore;
use crate::config::{Config, load_config};
use crate::control::{ControlStore, Role, UserRecord};

#[derive(Debug, Clone, Default)]
pub struct AuthStatusLines {
    pub lines: Vec<String>,
}

pub fn render_status(root: &Path, auth: &AuthStatusLines) -> Result<String> {
    let store = ControlStore::new(root)?;
    let users = store.list_users()?;
    let bootstrapped = !users.is_empty();

    let fallback_config_path = root.join("config.toml");
    let fallback_config = load_or_default(&fallback_config_path);

    let (config_path, config) = if let Some(user) = users.first() {
        let user_config_path = store.user_config_read_path(&user.user_id);
        (user_config_path.clone(), load_or_default(&user_config_path))
    } else {
        (fallback_config_path.clone(), fallback_config)
    };

    let mut lines = vec![
        format!("Root: {}", root.display()),
        format!("Config: {}", config_path.display()),
        format!("Workspace: {}", config.workspace_path().display()),
        format!(
            "Default profile: {}",
            config.agents.defaults.default_profile
        ),
        format!(
            "Control plane: {}",
            if bootstrapped {
                format!("bootstrapped ({} user{})", users.len(), plural(users.len()))
            } else {
                "not bootstrapped".to_string()
            }
        ),
    ];

    if bootstrapped {
        lines.push(format!("Users: {}", users.len()));
        for user in users {
            let runtime = runtime_summary(&store, &user);
            lines.push(format!(
                "- {} ({}, {}) | {}",
                user.username,
                role_label(&user.role),
                if user.enabled { "enabled" } else { "disabled" },
                runtime
            ));
        }
    }

    lines.extend(auth.lines.iter().cloned());
    Ok(lines.join("\n"))
}

pub async fn run(root: PathBuf) -> Result<()> {
    let output = render_status(&root, &AuthStatusLines::default())?;
    println!("{output}");
    Ok(())
}

fn load_or_default(path: &Path) -> Config {
    if !path.exists() {
        return Config::default();
    }
    load_config(Some(path)).unwrap_or_else(|_| Config::default())
}

fn runtime_summary(store: &ControlStore, user: &UserRecord) -> String {
    let config_path = store.user_config_read_path(&user.user_id);
    if !config_path.exists() {
        return format!("config missing at {}", config_path.display());
    }
    match load_config(Some(&config_path)) {
        Ok(config) => {
            let mut summary = format!(
                "workspace={} profile={}",
                config.workspace_path().display(),
                config.agents.defaults.default_profile
            );
            if let Ok(weixin_store) = WeixinAccountStore::new(&config.workspace_path()) {
                if let Ok(weixin) = weixin_store.login_status_summary() {
                    if weixin.configured || config.channels.weixin.enabled {
                        summary.push_str(&format!(
                            " weixin={} ({})",
                            weixin.login_state, weixin.account_status
                        ));
                    }
                }
            }
            summary
        }
        Err(error) => format!("config invalid at {} ({error})", config_path.display()),
    }
}

fn role_label(role: &Role) -> &'static str {
    match role {
        Role::Admin => "admin",
        Role::User => "user",
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}
