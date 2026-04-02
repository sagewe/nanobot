use std::process::Command;

use serde_json::Value;
use sidekick::config::{load_config, save_config};
use tempfile::tempdir;

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
    let user_config = dir.path().join("users").join(user_id).join("config.toml");
    let config_raw = std::fs::read_to_string(&user_config).expect("read config");
    let config_value: toml::Value = config_raw.parse().expect("parse config");
    assert_eq!(
        config_value
            .get("agents")
            .and_then(toml::Value::as_table)
            .and_then(|agents| agents.get("defaults"))
            .and_then(toml::Value::as_table)
            .and_then(|defaults| defaults.get("workspace"))
            .and_then(toml::Value::as_str),
        Some(
            dir.path()
                .join("users")
                .join(user_id)
                .join("workspace")
                .to_string_lossy()
                .as_ref()
        )
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
            .map(|value| value == legacy_workspace.display().to_string()),
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
