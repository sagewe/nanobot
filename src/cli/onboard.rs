use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde_json::Map;

use crate::config::{AgentProfileConfig, Config, save_config};
use crate::control::{BootstrapAdmin, ControlStore};

#[derive(Debug, Clone)]
pub struct OnboardOptions {
    pub wizard: bool,
    pub admin_username: Option<String>,
    pub admin_password: Option<String>,
    pub admin_display_name: String,
}

#[derive(Debug, Clone)]
struct WizardInputs {
    workspace_path: PathBuf,
    admin_username: String,
    admin_password: String,
    admin_display_name: String,
    default_profile: String,
    codex_auth_file: Option<String>,
    enable_weixin: bool,
}

pub async fn run(root: PathBuf, options: OnboardOptions) -> Result<()> {
    if options.wizard {
        run_wizard(root).await
    } else {
        run_non_wizard(root, options).await
    }
}

async fn run_non_wizard(root: PathBuf, options: OnboardOptions) -> Result<()> {
    let admin_username = options
        .admin_username
        .ok_or_else(|| anyhow!("--admin-username is required unless --wizard is provided"))?;
    let admin_password = options
        .admin_password
        .ok_or_else(|| anyhow!("--admin-password is required unless --wizard is provided"))?;
    let display_name = if options.admin_display_name.trim().is_empty() {
        admin_username.clone()
    } else {
        options.admin_display_name
    };

    let store = ControlStore::new(&root)?;
    let admin = BootstrapAdmin {
        username: admin_username,
        password: admin_password,
        display_name,
    };
    let user = store.bootstrap_first_admin(&admin)?;
    println!("Initialized multi-user control plane at {}", root.display());
    println!("Created first admin user {}", user.username);
    println!("{}", super::ONBOARD_TEMPLATE_SUMMARY);
    Ok(())
}

async fn run_wizard(root: PathBuf) -> Result<()> {
    let mut stdin = io::BufReader::new(io::stdin().lock());
    let mut stdout = io::stdout();

    writeln!(
        stdout,
        "Sidekick onboarding wizard (line-oriented; suitable for scripted stdin)."
    )?;
    stdout.flush()?;

    let defaults = Config::default();
    let inputs = prompt_wizard_inputs(root.as_path(), &defaults, &mut stdin, &mut stdout)?;

    let store = ControlStore::new(&root)?;
    let display_name = if inputs.admin_display_name.trim().is_empty() {
        inputs.admin_username.clone()
    } else {
        inputs.admin_display_name.clone()
    };
    let user = store.bootstrap_first_admin(&BootstrapAdmin {
        username: inputs.admin_username.clone(),
        password: inputs.admin_password.clone(),
        display_name,
    })?;

    let mut config = store.load_user_config(&user.user_id)?;
    apply_profile(&mut config, &inputs.default_profile)?;
    config.agents.defaults.workspace = inputs.workspace_path.display().to_string();
    if let Some(auth_file) = inputs.codex_auth_file.as_deref() {
        config.providers.codex.auth_file = auth_file.to_string();
    }
    config.channels.weixin.enabled = inputs.enable_weixin;

    store.validate_user_config(&user.user_id, &config)?;
    save_config(&config, Some(&store.user_config_path(&user.user_id)))?;
    super::ensure_workspace(&config.workspace_path())?;

    println!("Initialized multi-user control plane at {}", root.display());
    println!("Created first admin user {}", user.username);
    println!("Workspace set to {}", config.workspace_path().display());
    println!(
        "Default profile set to {}",
        config.agents.defaults.default_profile
    );
    println!("{}", super::ONBOARD_TEMPLATE_SUMMARY);
    Ok(())
}

fn prompt_wizard_inputs(
    root: &Path,
    defaults: &Config,
    reader: &mut impl BufRead,
    writer: &mut impl Write,
) -> Result<WizardInputs> {
    let workspace_default = root.join("workspace");
    let workspace = prompt(
        reader,
        writer,
        "Workspace path",
        Some(&workspace_default.display().to_string()),
    )?;
    let admin_username = prompt(reader, writer, "First admin username", None)?;
    let admin_password = prompt(reader, writer, "First admin password", None)?;
    let admin_display_name = prompt(reader, writer, "Admin display name (optional)", Some(""))?;

    let profile_default = defaults.agents.defaults.default_profile.clone();
    writeln!(writer, "Default profile options:")?;
    writeln!(writer, "  1) {profile_default}")?;
    writeln!(writer, "  2) codex:gpt-5.4")?;
    writeln!(writer, "  3) ollama:llama3.2")?;
    writer.flush()?;
    let profile_choice = prompt(
        reader,
        writer,
        "Select default profile (1/2/3 or provider:model)",
        Some("1"),
    )?;
    let default_profile = match profile_choice.trim() {
        "1" => profile_default,
        "2" => "codex:gpt-5.4".to_string(),
        "3" => "ollama:llama3.2".to_string(),
        custom => custom.to_string(),
    };

    let codex_auth_file = prompt(
        reader,
        writer,
        "Codex auth file path (optional; blank keeps current)",
        Some(""),
    )?;
    let codex_auth_file = if codex_auth_file.trim().is_empty() {
        None
    } else {
        Some(codex_auth_file.trim().to_string())
    };
    let enable_weixin = parse_yes_no(&prompt(
        reader,
        writer,
        "Enable Weixin channel now? [y/N]",
        Some("n"),
    )?);

    Ok(WizardInputs {
        workspace_path: PathBuf::from(workspace),
        admin_username,
        admin_password,
        admin_display_name,
        default_profile,
        codex_auth_file,
        enable_weixin,
    })
}

fn prompt(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    label: &str,
    default: Option<&str>,
) -> Result<String> {
    match default {
        Some(default) if !default.is_empty() => write!(writer, "{label} [{default}]: ")?,
        _ => write!(writer, "{label}: ")?,
    }
    writer.flush()?;

    let mut line = String::new();
    reader.read_line(&mut line)?;
    let line = line.trim_end_matches(['\n', '\r']).trim().to_string();
    if line.is_empty() {
        if let Some(default) = default {
            return Ok(default.to_string());
        }
    }
    if line.is_empty() {
        return Err(anyhow!("{label} is required"));
    }
    Ok(line)
}

fn parse_yes_no(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "y" | "yes" | "true" | "1"
    )
}

fn apply_profile(config: &mut Config, profile_key: &str) -> Result<()> {
    let (provider, model) = profile_key
        .split_once(':')
        .with_context(|| format!("invalid profile '{profile_key}'; expected provider:model"))?;
    let provider = provider.trim();
    let model = model.trim();
    if provider.is_empty() || model.is_empty() {
        return Err(anyhow!(
            "invalid profile '{profile_key}'; expected provider:model"
        ));
    }

    if !config.agents.profiles.contains_key(profile_key) {
        config.agents.profiles.insert(
            profile_key.to_string(),
            AgentProfileConfig {
                provider: provider.to_string(),
                model: model.to_string(),
                request: Map::new(),
            },
        );
    }
    config.agents.defaults.default_profile = profile_key.to_string();
    config.agents.defaults.provider = provider.to_string();
    config.agents.defaults.model = model.to_string();
    Ok(())
}
