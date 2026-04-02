use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use clap::{Args, Subcommand};

use crate::channels::weixin::{WeixinAccountStore, WeixinLoginManager};
use crate::control::{ControlStore, UserRecord};
use crate::providers::CodexProvider;

#[derive(Debug, Clone, Subcommand)]
pub enum ChannelsCommand {
    Status {
        #[arg(long)]
        user: Option<String>,
    },
    Login {
        #[command(subcommand)]
        channel: ChannelLoginCommand,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum ChannelLoginCommand {
    Weixin(WeixinLoginArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum ProviderCommand {
    Login {
        #[command(subcommand)]
        provider: ProviderLoginCommand,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum ProviderLoginCommand {
    Codex(CodexLoginArgs),
}

#[derive(Debug, Clone, Args)]
pub struct WeixinLoginArgs {
    #[arg(long)]
    pub user: Option<String>,
    #[arg(long, default_value_t = 60)]
    pub max_polls: u32,
    #[arg(long, default_value_t = 1_000)]
    pub poll_interval_ms: u64,
}

#[derive(Debug, Clone, Args)]
pub struct CodexLoginArgs {
    #[arg(long)]
    pub user: Option<String>,
}

pub async fn run_channels(root: PathBuf, action: ChannelsCommand) -> Result<()> {
    let store = ControlStore::new(&root)?;
    super::ensure_bootstrapped(&store)?;
    match action {
        ChannelsCommand::Status { user } => channels_status(store, user).await,
        ChannelsCommand::Login { channel } => match channel {
            ChannelLoginCommand::Weixin(args) => channels_login_weixin(store, args).await,
        },
    }
}

pub async fn run_provider(root: PathBuf, action: ProviderCommand) -> Result<()> {
    let store = ControlStore::new(&root)?;
    super::ensure_bootstrapped(&store)?;
    match action {
        ProviderCommand::Login { provider } => match provider {
            ProviderLoginCommand::Codex(args) => provider_login_codex(store, args).await,
        },
    }
}

pub fn status_lines(root: &Path) -> Result<Vec<String>> {
    let store = ControlStore::new(root)?;
    let users = store.list_users()?;
    let mut lines = Vec::new();

    for user in users {
        let config = store.load_user_config(&user.user_id)?;
        let summary = CodexProvider::auth_summary(&config.providers.codex);
        if summary.parse_valid {
            lines.push(format!(
                "Codex: ready | user={} | account_id={} | auth={}",
                user.username,
                summary.account_id.as_deref().unwrap_or("unknown"),
                summary.auth_path.display()
            ));
        } else {
            lines.push(format!(
                "Codex: not ready | user={} | auth={} | error={}",
                user.username,
                summary.auth_path.display(),
                summary
                    .error
                    .as_deref()
                    .unwrap_or("unknown codex auth parsing error")
            ));
        }
    }

    Ok(lines)
}

async fn channels_status(store: ControlStore, user: Option<String>) -> Result<()> {
    let users = resolve_target_users(&store, user.as_deref())?;
    for user in users {
        let config = store.load_user_config(&user.user_id)?;
        let weixin_store = WeixinAccountStore::new(&config.workspace_path())?;
        let weixin = weixin_store.login_status_summary()?;
        println!("User: {}", user.username);
        println!("  cli: enabled (built-in)");
        println!(
            "  telegram: {}",
            if config.channels.telegram.enabled {
                "enabled"
            } else {
                "disabled"
            }
        );
        println!(
            "  wecom: {}",
            if config.channels.wecom.enabled {
                "enabled"
            } else {
                "disabled"
            }
        );
        println!(
            "  feishu: {}",
            if config.channels.feishu.enabled {
                "enabled"
            } else {
                "disabled"
            }
        );
        println!(
            "  weixin: {} | {} | account status={}",
            if config.channels.weixin.enabled {
                "enabled"
            } else {
                "disabled"
            },
            weixin.login_state,
            weixin.account_status
        );
    }
    Ok(())
}

async fn channels_login_weixin(store: ControlStore, args: WeixinLoginArgs) -> Result<()> {
    let user = resolve_single_target_user(&store, args.user.as_deref())?;
    let config = store.load_user_config(&user.user_id)?;
    let weixin_store = WeixinAccountStore::new(&config.workspace_path())?;
    let manager = WeixinLoginManager::new(
        config.channels.weixin.api_base.clone(),
        weixin_store.clone(),
        env!("CARGO_PKG_VERSION"),
    );

    let login = manager.start_login().await?;
    println!("Started Weixin QR login for user {}", user.username);
    println!("QR code token: {}", login.qrcode);
    println!("QR code data URL: {}", login.qrcode_img_content);

    let max_polls = args.max_polls.max(1);
    for _ in 0..max_polls {
        let status = manager.poll_login_status().await?;
        println!("Weixin login status: {}", status.status);

        if status.status.eq_ignore_ascii_case("confirmed") {
            let account = weixin_store
                .load_account()?
                .ok_or_else(|| anyhow!("weixin login confirmed but account state is missing"))?;
            println!("Weixin login confirmed: {}", account.ilink_bot_id);
            manager.clear_login_session()?;
            return Ok(());
        }
        if status.status.eq_ignore_ascii_case("expired") {
            manager.clear_login_session()?;
            bail!("weixin login session expired; run `sidekick channels login weixin` again");
        }
        tokio::time::sleep(Duration::from_millis(args.poll_interval_ms.max(1))).await;
    }

    bail!(
        "weixin login timed out after {} polls; run `sidekick channels login weixin` again",
        max_polls
    )
}

async fn provider_login_codex(store: ControlStore, args: CodexLoginArgs) -> Result<()> {
    let user = resolve_single_target_user(&store, args.user.as_deref())?;
    let config = store.load_user_config(&user.user_id)?;
    let summary = CodexProvider::auth_summary(&config.providers.codex);

    println!("Codex auth file: {}", summary.auth_path.display());
    if !summary.parse_valid {
        bail!(
            "codex provider auth is not ready: {}",
            summary
                .error
                .unwrap_or_else(|| "unknown codex auth parsing error".to_string())
        );
    }

    println!(
        "Account ID: {}",
        summary.account_id.as_deref().unwrap_or("unknown")
    );
    println!("Codex provider auth is ready.");
    Ok(())
}

fn resolve_target_users(store: &ControlStore, username: Option<&str>) -> Result<Vec<UserRecord>> {
    if let Some(username) = username {
        let user = store
            .get_user_by_username(username)?
            .ok_or_else(|| anyhow!("unknown user '{}'", username))?;
        return Ok(vec![user]);
    }
    Ok(store.list_users()?)
}

fn resolve_single_target_user(store: &ControlStore, username: Option<&str>) -> Result<UserRecord> {
    if let Some(username) = username {
        return store
            .get_user_by_username(username)?
            .ok_or_else(|| anyhow!("unknown user '{}'", username));
    }
    let users = store.list_users()?;
    if users.len() == 1 {
        return Ok(users[0].clone());
    }
    bail!("multiple users found; pass --user <username>")
}
