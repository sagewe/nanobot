use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::channels::weixin::WeixinAccountStore;
use crate::config::{Config, load_config};
use crate::control::{ControlStore, Role, UserRecord};

#[derive(Debug, Clone, Default)]
pub struct AuthStatusLines {
    pub lines: Vec<String>,
}

pub fn render_status(
    root: &Path,
    auth: &AuthStatusLines,
    selected_user: Option<&str>,
    selected_workspace: Option<&str>,
) -> Result<String> {
    let store = ControlStore::new(root)?;
    let users = store.list_users()?;
    let bootstrapped = !users.is_empty();

    let fallback_config_path = root.join("config.toml");
    let fallback_config = load_or_default(&fallback_config_path);
    let selected_target =
        if bootstrapped && (selected_user.is_some() || selected_workspace.is_some()) {
            Some(super::load_runtime_target(
                &store,
                selected_user,
                selected_workspace,
            )?)
        } else if let Some(user) = users.first() {
            Some(super::load_runtime_target(
                &store,
                Some(&user.username),
                None,
            )?)
        } else {
            None
        };

    let (config_path, config) = if let Some(target) = &selected_target {
        (
            store.workspace_config_path(&target.workspace.workspace_id),
            target.config.clone(),
        )
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

    if let Some(target) = &selected_target {
        lines.push(format!("Selected user: {}", target.user.username));
        lines.push(format!(
            "Selected workspace: {} ({})",
            target.workspace.name, target.workspace.slug
        ));
        lines.push(format!(
            "Workspace path: {}",
            config.workspace_path().display()
        ));
        let resources = store.list_workspace_resources(&target.workspace.workspace_id)?;
        if !resources.is_empty() {
            let mut counts = std::collections::BTreeMap::new();
            for resource in &resources {
                *counts.entry(resource.kind.as_str()).or_insert(0usize) += 1;
            }
            let summary = counts
                .into_iter()
                .map(|(kind, count)| format!("{kind}={count}"))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("Resources: {} ({summary})", resources.len()));
        }
    }

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

pub async fn run(
    root: PathBuf,
    selected_user: Option<String>,
    selected_workspace: Option<String>,
) -> Result<()> {
    let output = render_status(
        &root,
        &AuthStatusLines::default(),
        selected_user.as_deref(),
        selected_workspace.as_deref(),
    )?;
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
    let workspace = match store.default_workspace_for_user(&user.user_id) {
        Ok(Some(workspace)) => workspace,
        Ok(None) => return "default workspace missing".to_string(),
        Err(error) => return format!("default workspace error ({error})"),
    };
    match store.load_runtime_config(&user.user_id, &workspace.workspace_id) {
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
        Err(error) => format!(
            "config invalid at {} ({error})",
            store.workspace_dir(&workspace.workspace_id).display()
        ),
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
