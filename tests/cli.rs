use std::collections::VecDeque;
use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use serde_json::Value;
use sidekick::channels::weixin::{WeixinAccountState, WeixinAccountStore};
use sidekick::config::{load_config, save_config};
use sidekick::control::ControlStore;
use tempfile::tempdir;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

#[derive(Clone)]
struct CliWeixinServerState {
    responses: Arc<Mutex<VecDeque<Value>>>,
}

async fn cli_weixin_qr_response(State(state): State<CliWeixinServerState>) -> Json<Value> {
    let response = state.responses.lock().await.pop_front().unwrap_or_else(
        || serde_json::json!({"data": {"qrcode": "unused", "qrcode_img_content": "unused"}}),
    );
    Json(response)
}

async fn cli_weixin_status_response(State(state): State<CliWeixinServerState>) -> Json<Value> {
    let response = state
        .responses
        .lock()
        .await
        .pop_front()
        .unwrap_or_else(|| serde_json::json!({"data": {"status": "wait"}}));
    Json(response)
}

struct CliWeixinServer {
    api_base: String,
    handle: tokio::task::JoinHandle<()>,
}

impl CliWeixinServer {
    async fn spawn(responses: Vec<Value>) -> Self {
        let state = CliWeixinServerState {
            responses: Arc::new(Mutex::new(responses.into())),
        };
        let app = Router::new()
            .route("/ilink/bot/get_bot_qrcode", get(cli_weixin_qr_response))
            .route(
                "/ilink/bot/get_qrcode_status",
                get(cli_weixin_status_response),
            )
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr: SocketAddr = listener.local_addr().expect("local addr");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve weixin test api");
        });
        Self {
            api_base: format!("http://{addr}"),
            handle,
        }
    }
}

impl Drop for CliWeixinServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

fn first_user_id(root: &Path) -> String {
    let users_raw =
        std::fs::read_to_string(root.join("control").join("users.json")).expect("users");
    let users_value: Value = serde_json::from_str(&users_raw).expect("parse users");
    users_value["users"][0]["user_id"]
        .as_str()
        .expect("user id")
        .to_string()
}

fn default_workspace_info(root: &Path) -> (String, std::path::PathBuf) {
    let store = ControlStore::new(root).expect("control store");
    let user_id = first_user_id(root);
    let workspace = store
        .default_workspace_for_user(&user_id)
        .expect("load default workspace")
        .expect("default workspace");
    (
        workspace.workspace_id.clone(),
        store.workspace_dir(&workspace.workspace_id),
    )
}

#[test]
fn status_reports_core_operator_summary_fields() {
    let dir = tempdir().expect("tempdir");

    let output = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("status")
        .output()
        .expect("run status");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Config:"), "{stdout}");
    assert!(stdout.contains("Workspace:"), "{stdout}");
    assert!(stdout.contains("Default profile:"), "{stdout}");
    assert!(stdout.contains("Control plane:"), "{stdout}");
}

#[test]
fn status_reports_user_summary_when_control_plane_is_bootstrapped() {
    let dir = tempdir().expect("tempdir");

    let bootstrap = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("onboard")
        .arg("--admin-username")
        .arg("alice")
        .arg("--admin-password")
        .arg("password123")
        .output()
        .expect("run onboard");
    assert!(bootstrap.status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("status")
        .output()
        .expect("run status");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Users:"), "{stdout}");
    assert!(stdout.contains("alice"), "{stdout}");
}

#[test]
fn onboard_wizard_bootstraps_admin_and_seeds_runtime_config() {
    let dir = tempdir().expect("tempdir");

    let mut child = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("onboard")
        .arg("--wizard")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn onboard wizard");

    {
        let stdin = child.stdin.as_mut().expect("stdin");
        writeln!(stdin, "Docs").expect("workspace input");
        writeln!(stdin, "wizard-admin").expect("username input");
        writeln!(stdin, "wizard-password").expect("password input");
        writeln!(stdin, "Wizard Admin").expect("display name input");
        writeln!(stdin, "2").expect("default profile input");
        writeln!(stdin, "{}/codex-auth.json", dir.path().display()).expect("codex auth input");
        writeln!(stdin, "y").expect("weixin enable input");
    }

    let output = child.wait_with_output().expect("wait onboard wizard");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let users_raw =
        std::fs::read_to_string(dir.path().join("control").join("users.json")).expect("users");
    let users_value: Value = serde_json::from_str(&users_raw).expect("parse users");
    let users = users_value["users"].as_array().expect("users array");
    assert_eq!(users.len(), 1);
    assert_eq!(users[0]["username"].as_str(), Some("wizard-admin"));

    let user_id = users[0]["user_id"].as_str().expect("user id");
    let store = ControlStore::new(dir.path()).expect("control store");
    let workspace = store
        .default_workspace_for_user(user_id)
        .expect("load default workspace")
        .expect("default workspace");
    let config = store
        .load_runtime_config(user_id, &workspace.workspace_id)
        .expect("load runtime config");
    assert_eq!(workspace.name, "Docs");
    assert_eq!(
        config.workspace_path(),
        store.workspace_dir(&workspace.workspace_id)
    );
    assert_eq!(config.agents.defaults.default_profile, "codex:gpt-5.4");
    assert_eq!(
        config.providers.codex.auth_file,
        format!("{}/codex-auth.json", dir.path().display())
    );
    assert!(config.channels.weixin.enabled);
}

#[test]
fn channels_status_lists_built_in_channels_and_weixin_state() {
    let dir = tempdir().expect("tempdir");

    let bootstrap = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("onboard")
        .arg("--admin-username")
        .arg("alice")
        .arg("--admin-password")
        .arg("password123")
        .output()
        .expect("run onboard");
    assert!(bootstrap.status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("channels")
        .arg("status")
        .output()
        .expect("run channels status");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("cli"), "{stdout}");
    assert!(stdout.contains("telegram"), "{stdout}");
    assert!(stdout.contains("wecom"), "{stdout}");
    assert!(stdout.contains("feishu"), "{stdout}");
    assert!(stdout.contains("weixin"), "{stdout}");
    assert!(stdout.contains("weixin: disabled"), "{stdout}");
    assert!(stdout.contains("not logged in"), "{stdout}");

    let user_id = first_user_id(dir.path());
    let (workspace_id, workspace_path) = default_workspace_info(dir.path());
    let store = ControlStore::new(dir.path()).expect("control store");
    let mut config = store
        .load_runtime_config(&user_id, &workspace_id)
        .expect("load runtime config");
    config.channels.weixin.enabled = true;
    store
        .write_runtime_config(&user_id, &workspace_id, &config)
        .expect("save runtime config");

    let weixin_store = WeixinAccountStore::new(&workspace_path).expect("weixin store");
    weixin_store
        .save_account(&WeixinAccountState {
            bot_token: "bot-token".to_string(),
            ilink_bot_id: "bot@im.bot".to_string(),
            baseurl: "https://ilinkai.weixin.qq.com".to_string(),
            ilink_user_id: Some("alice@im.wechat".to_string()),
            get_updates_buf: String::new(),
            longpolling_timeout_ms: 35_000,
            status: "expired".to_string(),
            updated_at: Utc::now(),
        })
        .expect("save expired account");

    let enabled_output = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("channels")
        .arg("status")
        .output()
        .expect("run channels status with account");
    assert!(
        enabled_output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&enabled_output.stdout),
        String::from_utf8_lossy(&enabled_output.stderr)
    );
    let enabled_stdout = String::from_utf8_lossy(&enabled_output.stdout);
    assert!(
        enabled_stdout.contains("weixin: enabled"),
        "{enabled_stdout}"
    );
    assert!(enabled_stdout.contains("| expired |"), "{enabled_stdout}");
    assert!(
        enabled_stdout.contains("account status=expired"),
        "{enabled_stdout}"
    );
}

#[tokio::test]
async fn channels_login_weixin_persists_confirmed_account() {
    let dir = tempdir().expect("tempdir");

    let bootstrap = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("onboard")
        .arg("--admin-username")
        .arg("alice")
        .arg("--admin-password")
        .arg("password123")
        .output()
        .expect("run onboard");
    assert!(bootstrap.status.success());

    let server = CliWeixinServer::spawn(vec![
        serde_json::json!({
            "data": {
                "qrcode": "qr-token",
                "qrcode_img_content": "data:image/png;base64,abc"
            }
        }),
        serde_json::json!({
            "data": {
                "status": "confirmed",
                "bot_token": "bot-token",
                "ilink_bot_id": "bot@im.bot",
                "baseurl": "https://alt.example",
                "ilink_user_id": "user@im.wechat"
            }
        }),
    ])
    .await;

    let user_id = first_user_id(dir.path());
    let (workspace_id, workspace_path) = default_workspace_info(dir.path());
    let store = ControlStore::new(dir.path()).expect("control store");
    let mut config = store
        .load_runtime_config(&user_id, &workspace_id)
        .expect("load runtime config");
    config.channels.weixin.enabled = true;
    config.channels.weixin.api_base = server.api_base.clone();
    store
        .write_runtime_config(&user_id, &workspace_id, &config)
        .expect("save runtime config");

    let root = dir.path().to_path_buf();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_sidekick"))
            .arg("--root")
            .arg(root)
            .arg("channels")
            .arg("login")
            .arg("weixin")
            .arg("--max-polls")
            .arg("4")
            .arg("--poll-interval-ms")
            .arg("1")
            .output()
            .expect("run channels login weixin")
    })
    .await
    .expect("join channels login weixin");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Started Weixin QR login"), "{stdout}");
    assert!(stdout.contains("qr-token"), "{stdout}");
    assert!(
        stdout.contains("Weixin login status: confirmed"),
        "{stdout}"
    );
    assert!(stdout.contains("Weixin login confirmed"), "{stdout}");

    let store = WeixinAccountStore::new(&workspace_path).expect("weixin store");
    let account = store
        .load_account()
        .expect("load weixin account")
        .expect("weixin account");
    assert_eq!(account.bot_token, "bot-token");
    assert_eq!(account.ilink_bot_id, "bot@im.bot");
    assert_eq!(account.status, "confirmed");
}

#[test]
fn provider_login_codex_reports_valid_account_id() {
    let dir = tempdir().expect("tempdir");

    let bootstrap = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("onboard")
        .arg("--admin-username")
        .arg("alice")
        .arg("--admin-password")
        .arg("password123")
        .output()
        .expect("run onboard");
    assert!(bootstrap.status.success());

    let auth_path = dir.path().join("codex-auth.json");
    std::fs::write(
        &auth_path,
        serde_json::json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "access_token": "access-token",
                "refresh_token": "refresh-token",
                "id_token": "id-token",
                "account_id": "acct-valid"
            }
        })
        .to_string(),
    )
    .expect("write codex auth");

    let user_id = first_user_id(dir.path());
    let (workspace_id, _) = default_workspace_info(dir.path());
    let store = ControlStore::new(dir.path()).expect("control store");
    let mut config = store
        .load_runtime_config(&user_id, &workspace_id)
        .expect("load runtime config");
    config.providers.codex.auth_file = auth_path.display().to_string();
    store
        .write_runtime_config(&user_id, &workspace_id, &config)
        .expect("save runtime config");

    let output = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("provider")
        .arg("login")
        .arg("codex")
        .output()
        .expect("run provider login codex");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Codex auth file:"), "{stdout}");
    assert!(stdout.contains("Account ID: acct-valid"), "{stdout}");
}

#[test]
fn provider_login_codex_fails_for_missing_or_malformed_auth_file() {
    let dir = tempdir().expect("tempdir");

    let bootstrap = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("onboard")
        .arg("--admin-username")
        .arg("alice")
        .arg("--admin-password")
        .arg("password123")
        .output()
        .expect("run onboard");
    assert!(bootstrap.status.success());

    let user_id = first_user_id(dir.path());
    let (workspace_id, _) = default_workspace_info(dir.path());
    let store = ControlStore::new(dir.path()).expect("control store");
    let mut config = store
        .load_runtime_config(&user_id, &workspace_id)
        .expect("load runtime config");
    config.providers.codex.auth_file = dir.path().join("missing-auth.json").display().to_string();
    store
        .write_runtime_config(&user_id, &workspace_id, &config)
        .expect("save runtime config");

    let missing_output = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("provider")
        .arg("login")
        .arg("codex")
        .output()
        .expect("run provider login codex missing");
    assert!(
        !missing_output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&missing_output.stdout),
        String::from_utf8_lossy(&missing_output.stderr)
    );
    let missing_stderr = String::from_utf8_lossy(&missing_output.stderr);
    assert!(
        missing_stderr.contains("failed to read codex auth file"),
        "{missing_stderr}"
    );

    let malformed_auth_path = dir.path().join("malformed-auth.json");
    std::fs::write(
        &malformed_auth_path,
        serde_json::json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "access_token": "access-token",
                "refresh_token": "refresh-token"
            }
        })
        .to_string(),
    )
    .expect("write malformed auth");
    config.providers.codex.auth_file = malformed_auth_path.display().to_string();
    store
        .write_runtime_config(&user_id, &workspace_id, &config)
        .expect("save runtime config malformed");

    let malformed_output = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("provider")
        .arg("login")
        .arg("codex")
        .output()
        .expect("run provider login codex malformed");
    assert!(
        !malformed_output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&malformed_output.stdout),
        String::from_utf8_lossy(&malformed_output.stderr)
    );
    let malformed_stderr = String::from_utf8_lossy(&malformed_output.stderr);
    assert!(
        malformed_stderr.contains("missing required field"),
        "{malformed_stderr}"
    );
}

#[test]
fn status_includes_codex_readiness_line_when_auth_is_valid() {
    let dir = tempdir().expect("tempdir");

    let bootstrap = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("onboard")
        .arg("--admin-username")
        .arg("alice")
        .arg("--admin-password")
        .arg("password123")
        .output()
        .expect("run onboard");
    assert!(bootstrap.status.success());

    let auth_path = dir.path().join("codex-auth.json");
    std::fs::write(
        &auth_path,
        serde_json::json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "access_token": "access-token",
                "refresh_token": "refresh-token",
                "id_token": "id-token",
                "account_id": "acct-status"
            }
        })
        .to_string(),
    )
    .expect("write codex auth");

    let user_id = first_user_id(dir.path());
    let (workspace_id, _) = default_workspace_info(dir.path());
    let store = ControlStore::new(dir.path()).expect("control store");
    let mut config = store
        .load_runtime_config(&user_id, &workspace_id)
        .expect("load runtime config");
    config.providers.codex.auth_file = auth_path.display().to_string();
    store
        .write_runtime_config(&user_id, &workspace_id, &config)
        .expect("save runtime config");

    let output = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("status")
        .output()
        .expect("run status");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Codex: ready"), "{stdout}");
    assert!(stdout.contains("acct-status"), "{stdout}");
}

#[test]
fn onboard_bootstraps_control_plane_and_first_admin() {
    let dir = tempdir().expect("tempdir");

    let output = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("onboard")
        .arg("--admin-username")
        .arg("alice")
        .arg("--admin-password")
        .arg("password123")
        .arg("--admin-display-name")
        .arg("Alice")
        .output()
        .expect("run onboard");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let users_raw =
        std::fs::read_to_string(dir.path().join("control").join("users.json")).expect("users");
    let users_value: Value = serde_json::from_str(&users_raw).expect("parse users");
    let users = users_value["users"].as_array().expect("users array");
    assert_eq!(users.len(), 1);
    assert_eq!(users[0]["username"].as_str(), Some("alice"));
    assert_eq!(users[0]["role"].as_str(), Some("admin"));

    let user_id = users[0]["user_id"].as_str().expect("user id");
    let store = ControlStore::new(dir.path()).expect("control store");
    let workspace = store
        .default_workspace_for_user(user_id)
        .expect("load default workspace")
        .expect("default workspace");
    let config = store
        .load_runtime_config(user_id, &workspace.workspace_id)
        .expect("load runtime config");
    assert_eq!(
        config.workspace_path(),
        store.workspace_dir(&workspace.workspace_id)
    );
}

#[test]
fn users_list_shows_bootstrapped_accounts() {
    let dir = tempdir().expect("tempdir");

    let bootstrap = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("onboard")
        .arg("--admin-username")
        .arg("alice")
        .arg("--admin-password")
        .arg("password123")
        .output()
        .expect("run onboard");
    assert!(bootstrap.status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("users")
        .arg("list")
        .output()
        .expect("run users list");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("alice"), "{stdout}");
    assert!(stdout.contains("admin"), "{stdout}");
}

#[test]
fn users_commands_manage_accounts_and_configs() {
    let dir = tempdir().expect("tempdir");

    let bootstrap = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("onboard")
        .arg("--admin-username")
        .arg("alice")
        .arg("--admin-password")
        .arg("password123")
        .output()
        .expect("run onboard");
    assert!(bootstrap.status.success());

    let create = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("users")
        .arg("create")
        .arg("--username")
        .arg("bob")
        .arg("--password")
        .arg("secret123")
        .arg("--display-name")
        .arg("Bob")
        .arg("--role")
        .arg("user")
        .output()
        .expect("run users create");
    assert!(
        create.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&create.stdout),
        String::from_utf8_lossy(&create.stderr)
    );

    for args in [
        vec!["users", "disable", "--username", "bob"],
        vec!["users", "set-role", "--username", "bob", "--role", "admin"],
        vec![
            "users",
            "set-password",
            "--username",
            "bob",
            "--password",
            "newsecret456",
        ],
        vec!["users", "enable", "--username", "bob"],
        vec!["users", "validate-config", "--username", "bob"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_sidekick"))
            .arg("--root")
            .arg(dir.path())
            .args(args)
            .output()
            .expect("run users command");
        assert!(
            output.status.success(),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let users_raw =
        std::fs::read_to_string(dir.path().join("control").join("users.json")).expect("users");
    let users_value: Value = serde_json::from_str(&users_raw).expect("parse users");
    let bob_user_id = users_value["users"]
        .as_array()
        .expect("users array")
        .iter()
        .find(|user| user["username"].as_str() == Some("bob"))
        .and_then(|user| user["user_id"].as_str())
        .expect("bob user id");
    let bob_config_toml = dir.path().join("users").join(bob_user_id);
    let bob_config_toml = bob_config_toml.join("config.toml");
    let bob_config_json = bob_config_toml.with_file_name("config.json");
    let mut bob_config = load_config(Some(&bob_config_toml)).expect("load bob config");
    let legacy_workspace = dir.path().join("legacy-bob-workspace");
    bob_config.agents.defaults.workspace = legacy_workspace.display().to_string();
    save_config(&bob_config, Some(&bob_config_json)).expect("write legacy bob config");
    std::fs::remove_file(&bob_config_toml).expect("remove bob toml");

    let store = ControlStore::new(dir.path()).expect("control store");
    let bob_workspace = store
        .default_workspace_for_user(bob_user_id)
        .expect("load bob default workspace")
        .expect("bob default workspace");

    let show_config = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("users")
        .arg("show-config")
        .arg("--username")
        .arg("bob")
        .output()
        .expect("run users show-config");
    assert!(show_config.status.success());
    let config_value: toml::Value = String::from_utf8_lossy(&show_config.stdout)
        .parse()
        .expect("show-config toml output");
    assert_eq!(
        config_value
            .get("agents")
            .and_then(toml::Value::as_table)
            .and_then(|agents| agents.get("defaults"))
            .and_then(toml::Value::as_table)
            .and_then(|defaults| defaults.get("workspace"))
            .and_then(toml::Value::as_str)
            .map(|value| value
                == store
                    .workspace_dir(&bob_workspace.workspace_id)
                    .display()
                    .to_string()),
        Some(true)
    );

    let users_raw =
        std::fs::read_to_string(dir.path().join("control").join("users.json")).expect("users");
    let users_value: Value = serde_json::from_str(&users_raw).expect("parse users");
    let users = users_value["users"].as_array().expect("users array");
    let bob = users
        .iter()
        .find(|user| user["username"].as_str() == Some("bob"))
        .expect("bob");
    assert_eq!(bob["role"].as_str(), Some("admin"));
    assert_eq!(bob["enabled"].as_bool(), Some(true));
}

#[test]
fn status_can_target_a_named_workspace() {
    let dir = tempdir().expect("tempdir");

    let bootstrap = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("onboard")
        .arg("--admin-username")
        .arg("alice")
        .arg("--admin-password")
        .arg("password123")
        .output()
        .expect("run onboard");
    assert!(bootstrap.status.success());

    let store = ControlStore::new(dir.path()).expect("control store");
    let user_id = first_user_id(dir.path());
    let docs = store
        .create_workspace(&user_id, "Docs", Some("docs"))
        .expect("create docs workspace");

    let output = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("status")
        .arg("--user")
        .arg("alice")
        .arg("--workspace")
        .arg("docs")
        .output()
        .expect("run targeted status");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Selected user: alice"), "{stdout}");
    assert!(
        stdout.contains("Selected workspace: Docs (docs)"),
        "{stdout}"
    );
    assert!(
        stdout.contains(
            store
                .workspace_dir(&docs.workspace_id)
                .display()
                .to_string()
                .as_str()
        ),
        "{stdout}"
    );
    assert!(
        stdout.contains("Default profile: openai:gpt-4.1-mini"),
        "{stdout}"
    );
}

#[test]
fn channels_status_can_target_a_named_workspace() {
    let dir = tempdir().expect("tempdir");

    let bootstrap = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("onboard")
        .arg("--admin-username")
        .arg("alice")
        .arg("--admin-password")
        .arg("password123")
        .output()
        .expect("run onboard");
    assert!(bootstrap.status.success());

    let store = ControlStore::new(dir.path()).expect("control store");
    let user_id = first_user_id(dir.path());
    let docs = store
        .create_workspace(&user_id, "Docs", Some("docs"))
        .expect("create docs workspace");
    let mut config = store
        .load_runtime_config(&user_id, &docs.workspace_id)
        .expect("load docs runtime config");
    config.channels.weixin.enabled = true;
    store
        .write_runtime_config(&user_id, &docs.workspace_id, &config)
        .expect("save docs runtime config");

    let weixin_store = WeixinAccountStore::new(&store.workspace_dir(&docs.workspace_id))
        .expect("docs weixin store");
    weixin_store
        .save_account(&WeixinAccountState {
            bot_token: "bot-token".to_string(),
            ilink_bot_id: "bot@im.bot".to_string(),
            baseurl: "https://ilinkai.weixin.qq.com".to_string(),
            ilink_user_id: Some("alice@im.wechat".to_string()),
            get_updates_buf: String::new(),
            longpolling_timeout_ms: 35_000,
            status: "confirmed".to_string(),
            updated_at: Utc::now(),
        })
        .expect("save weixin account");

    let output = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("channels")
        .arg("status")
        .arg("--user")
        .arg("alice")
        .arg("--workspace")
        .arg("docs")
        .output()
        .expect("run channels status");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Workspace: Docs (docs)"), "{stdout}");
    assert!(stdout.contains("weixin: enabled | logged in"), "{stdout}");
}

#[test]
fn users_migrate_legacy_subcommand_is_rejected() {
    let dir = tempdir().expect("tempdir");

    let output = Command::new(env!("CARGO_BIN_EXE_sidekick"))
        .arg("--root")
        .arg(dir.path())
        .arg("users")
        .arg("migrate-legacy")
        .arg("--admin-username")
        .arg("alice")
        .arg("--admin-password")
        .arg("password123")
        .output()
        .expect("run users migrate-legacy");

    assert!(
        !output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("migrate-legacy"),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
