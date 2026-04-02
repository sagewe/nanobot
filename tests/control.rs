use std::path::Path;

use sidekick::config::Config;
use tempfile::tempdir;

use sidekick::control::{AuthService, BootstrapAdmin, ControlStore, Role, RuntimeManager};

fn legacy_config_with_workspace(workspace: &Path) -> Config {
    let mut config = Config::default();
    config.agents.defaults.workspace = workspace.display().to_string();
    config
}

#[test]
fn bootstrap_first_admin_creates_control_files_and_user_paths() {
    let dir = tempdir().expect("tempdir");
    let store = ControlStore::new(dir.path()).expect("control store");

    let admin = store
        .bootstrap_first_admin(&BootstrapAdmin {
            username: "alice".to_string(),
            password: "password123".to_string(),
            display_name: "Alice".to_string(),
        })
        .expect("bootstrap admin");

    assert_eq!(admin.username, "alice");
    assert_eq!(admin.role, Role::Admin);
    assert!(store.control_dir().join("system.json").exists());
    assert!(store.control_dir().join("users.json").exists());
    assert!(store.control_dir().join("web_sessions.json").exists());
    assert!(store.control_dir().join("audit.jsonl").exists());
    assert!(store.user_config_path(&admin.user_id).exists());
    assert_eq!(
        store.user_config_path(&admin.user_id),
        store.user_dir(&admin.user_id).join("config.toml")
    );
    assert!(!store.user_dir(&admin.user_id).join("config.json").exists());
    assert!(
        store
            .user_workspace_path(&admin.user_id)
            .join("memory")
            .exists()
    );
    assert!(
        store
            .user_workspace_path(&admin.user_id)
            .join("memory")
            .join("MEMORY.md")
            .exists()
    );
    assert!(
        store
            .user_workspace_path(&admin.user_id)
            .join("memory")
            .join("HISTORY.md")
            .exists()
    );
}

#[test]
fn bootstrap_migrates_legacy_config_and_workspace_into_first_admin() {
    let dir = tempdir().expect("tempdir");
    let legacy_root = dir.path().join("legacy");
    let legacy_workspace = legacy_root.join("workspace");
    std::fs::create_dir_all(legacy_workspace.join("memory")).expect("legacy workspace");
    std::fs::write(
        legacy_workspace.join("memory").join("MEMORY.md"),
        "legacy memory",
    )
    .expect("legacy memory");
    std::fs::write(
        legacy_workspace.join("memory").join("HISTORY.md"),
        "legacy history",
    )
    .expect("legacy history");
    let legacy_config_path = legacy_root.join("config.json");
    sidekick::config::save_config(
        &legacy_config_with_workspace(&legacy_workspace),
        Some(&legacy_config_path),
    )
    .expect("save legacy config");

    let store = ControlStore::new(dir.path()).expect("control store");
    let admin = store
        .bootstrap_from_legacy(
            &BootstrapAdmin {
                username: "admin".to_string(),
                password: "password123".to_string(),
                display_name: "Admin".to_string(),
            },
            &legacy_config_path,
            &legacy_workspace,
        )
        .expect("bootstrap from legacy");

    assert!(store.control_dir().join("migration.json").exists());
    assert!(store.user_config_path(&admin.user_id).exists());
    assert_eq!(
        store.user_config_path(&admin.user_id),
        store.user_dir(&admin.user_id).join("config.toml")
    );
    assert!(!store.user_dir(&admin.user_id).join("config.json").exists());
    assert_eq!(
        std::fs::read_to_string(
            store
                .user_workspace_path(&admin.user_id)
                .join("memory")
                .join("MEMORY.md")
        )
        .expect("migrated memory"),
        "legacy memory"
    );
    assert_eq!(
        std::fs::read_to_string(
            store
                .user_workspace_path(&admin.user_id)
                .join("memory")
                .join("HISTORY.md")
        )
        .expect("migrated history"),
        "legacy history"
    );
}

#[test]
fn bootstrap_migrates_legacy_workspace_preserving_custom_memory_content() {
    let dir = tempdir().expect("tempdir");
    let legacy_root = dir.path().join("legacy-preserve");
    let legacy_workspace = legacy_root.join("workspace");
    std::fs::create_dir_all(legacy_workspace.join("memory")).expect("legacy workspace");
    std::fs::write(
        legacy_workspace.join("memory").join("MEMORY.md"),
        "custom memory",
    )
    .expect("legacy memory");
    std::fs::write(
        legacy_workspace.join("memory").join("HISTORY.md"),
        "custom history",
    )
    .expect("legacy history");
    let legacy_config_path = legacy_root.join("config.json");
    sidekick::config::save_config(
        &legacy_config_with_workspace(&legacy_workspace),
        Some(&legacy_config_path),
    )
    .expect("save legacy config");

    let store = ControlStore::new(dir.path()).expect("control store");
    let admin = store
        .bootstrap_from_legacy(
            &BootstrapAdmin {
                username: "admin".to_string(),
                password: "password123".to_string(),
                display_name: "Admin".to_string(),
            },
            &legacy_config_path,
            &legacy_workspace,
        )
        .expect("bootstrap from legacy");

    assert_eq!(
        std::fs::read_to_string(
            store
                .user_workspace_path(&admin.user_id)
                .join("memory")
                .join("MEMORY.md")
        )
        .expect("preserved memory"),
        "custom memory"
    );
    assert_eq!(
        std::fs::read_to_string(
            store
                .user_workspace_path(&admin.user_id)
                .join("memory")
                .join("HISTORY.md")
        )
        .expect("preserved history"),
        "custom history"
    );
}

#[test]
fn load_user_config_reads_legacy_json_when_toml_is_missing() {
    let dir = tempdir().expect("tempdir");
    let store = ControlStore::new(dir.path()).expect("control store");
    let admin = store
        .bootstrap_first_admin(&BootstrapAdmin {
            username: "alice".to_string(),
            password: "password123".to_string(),
            display_name: "Alice".to_string(),
        })
        .expect("bootstrap admin");

    let legacy_workspace = dir.path().join("legacy-workspace");
    let mut legacy = legacy_config_with_workspace(&legacy_workspace);
    legacy.channels.send_tool_hints = true;
    let legacy_json_path = store.user_dir(&admin.user_id).join("config.json");
    sidekick::config::save_config(&legacy, Some(&legacy_json_path)).expect("write legacy json");
    std::fs::remove_file(store.user_config_path(&admin.user_id)).expect("remove canonical toml");

    let loaded = store
        .load_user_config(&admin.user_id)
        .expect("load legacy user config");

    assert_eq!(
        loaded.agents.defaults.workspace,
        legacy_workspace.display().to_string()
    );
    assert!(loaded.channels.send_tool_hints);
}

#[test]
fn validation_rejects_duplicate_telegram_wecom_and_feishu_connectors() {
    let dir = tempdir().expect("tempdir");
    let store = ControlStore::new(dir.path()).expect("control store");
    let admin = store
        .bootstrap_first_admin(&BootstrapAdmin {
            username: "alice".to_string(),
            password: "password123".to_string(),
            display_name: "Alice".to_string(),
        })
        .expect("bootstrap admin");
    let user = store
        .create_user("bob", "Bob", Role::User, "password456")
        .expect("create user");

    let mut first = Config::default();
    first.channels.telegram.enabled = true;
    first.channels.telegram.token = "token-a".to_string();
    first.channels.wecom.enabled = true;
    first.channels.wecom.bot_id = "bot-a".to_string();
    first.channels.wecom.secret = "secret-a".to_string();
    first.channels.feishu.enabled = true;
    first.channels.feishu.app_id = "cli_a1".to_string();
    first.channels.feishu.app_secret = "secret-a".to_string();
    store
        .write_user_config(&admin.user_id, &first)
        .expect("write first config");

    let mut second = Config::default();
    second.channels.telegram.enabled = true;
    second.channels.telegram.token = "token-a".to_string();
    let telegram_error = store
        .validate_user_config(&user.user_id, &second)
        .expect_err("duplicate telegram token");
    assert!(telegram_error.to_string().contains("telegram"));

    second.channels.telegram.token = "token-b".to_string();
    second.channels.wecom.enabled = true;
    second.channels.wecom.bot_id = "bot-a".to_string();
    second.channels.wecom.secret = "secret-a".to_string();
    let wecom_error = store
        .validate_user_config(&user.user_id, &second)
        .expect_err("duplicate wecom credentials");
    assert!(wecom_error.to_string().contains("wecom"));

    second.channels.wecom.enabled = false;
    second.channels.feishu.enabled = true;
    second.channels.feishu.app_id = "cli_a1".to_string();
    second.channels.feishu.app_secret = "secret-a".to_string();
    let feishu_error = store
        .validate_user_config(&user.user_id, &second)
        .expect_err("duplicate feishu credentials");
    assert!(feishu_error.to_string().contains("feishu"));
}

#[test]
fn validation_rejects_duplicates_from_legacy_json_user_configs() {
    let dir = tempdir().expect("tempdir");
    let store = ControlStore::new(dir.path()).expect("control store");
    let admin = store
        .bootstrap_first_admin(&BootstrapAdmin {
            username: "alice".to_string(),
            password: "password123".to_string(),
            display_name: "Alice".to_string(),
        })
        .expect("bootstrap admin");
    let user = store
        .create_user("bob", "Bob", Role::User, "password456")
        .expect("create user");

    let mut legacy = Config::default();
    legacy.channels.telegram.enabled = true;
    legacy.channels.telegram.token = "token-a".to_string();
    legacy.channels.wecom.enabled = true;
    legacy.channels.wecom.bot_id = "bot-a".to_string();
    legacy.channels.wecom.secret = "secret-a".to_string();
    legacy.channels.feishu.enabled = true;
    legacy.channels.feishu.app_id = "cli_a1".to_string();
    legacy.channels.feishu.app_secret = "secret-a".to_string();
    let legacy_json_path = store.user_dir(&admin.user_id).join("config.json");
    sidekick::config::save_config(&legacy, Some(&legacy_json_path)).expect("write legacy json");
    std::fs::remove_file(store.user_config_path(&admin.user_id)).expect("remove canonical toml");

    let mut second = Config::default();
    second.channels.telegram.enabled = true;
    second.channels.telegram.token = "token-a".to_string();
    let telegram_error = store
        .validate_user_config(&user.user_id, &second)
        .expect_err("duplicate telegram token");
    assert!(telegram_error.to_string().contains("telegram"));

    second.channels.telegram.token = "token-b".to_string();
    second.channels.wecom.enabled = true;
    second.channels.wecom.bot_id = "bot-a".to_string();
    second.channels.wecom.secret = "secret-a".to_string();
    let wecom_error = store
        .validate_user_config(&user.user_id, &second)
        .expect_err("duplicate wecom credentials");
    assert!(wecom_error.to_string().contains("wecom"));

    second.channels.wecom.enabled = false;
    second.channels.feishu.enabled = true;
    second.channels.feishu.app_id = "cli_a1".to_string();
    second.channels.feishu.app_secret = "secret-a".to_string();
    let feishu_error = store
        .validate_user_config(&user.user_id, &second)
        .expect_err("duplicate feishu credentials");
    assert!(feishu_error.to_string().contains("feishu"));
}

#[test]
fn auth_service_creates_and_resolves_sessions() {
    let dir = tempdir().expect("tempdir");
    let store = ControlStore::new(dir.path()).expect("control store");
    let user = store
        .bootstrap_first_admin(&BootstrapAdmin {
            username: "alice".to_string(),
            password: "password123".to_string(),
            display_name: "Alice".to_string(),
        })
        .expect("bootstrap admin");

    let auth = AuthService::new(store.clone());
    let session = auth.login("alice", "password123").expect("login");
    let resolved = auth
        .authenticate_session(&session.session_id)
        .expect("authenticate")
        .expect("user");

    assert_eq!(resolved.user_id, user.user_id);
    assert_eq!(resolved.role, Role::Admin);

    auth.logout(&session.session_id).expect("logout");
    assert!(
        auth.authenticate_session(&session.session_id)
            .expect("authenticate after logout")
            .is_none()
    );
}

#[tokio::test]
async fn runtime_manager_starts_isolated_runtimes_per_user() {
    let dir = tempdir().expect("tempdir");
    let store = ControlStore::new(dir.path()).expect("control store");
    let admin = store
        .bootstrap_first_admin(&BootstrapAdmin {
            username: "alice".to_string(),
            password: "password123".to_string(),
            display_name: "Alice".to_string(),
        })
        .expect("bootstrap admin");
    let user = store
        .create_user("bob", "Bob", Role::User, "password456")
        .expect("create user");

    let manager = RuntimeManager::new(store.clone(), false);
    let alice_runtime = manager
        .get_or_start(&admin.user_id)
        .await
        .expect("alice runtime");
    let bob_runtime = manager
        .get_or_start(&user.user_id)
        .await
        .expect("bob runtime");

    assert_ne!(alice_runtime.user_id(), bob_runtime.user_id());
    assert_eq!(
        alice_runtime.workspace_path(),
        store.user_workspace_path(&admin.user_id).as_path()
    );
    assert_eq!(
        bob_runtime.workspace_path(),
        store.user_workspace_path(&user.user_id).as_path()
    );
}

#[tokio::test]
async fn runtime_manager_reload_swaps_only_the_target_user_runtime() {
    let dir = tempdir().expect("tempdir");
    let store = ControlStore::new(dir.path()).expect("control store");
    let admin = store
        .bootstrap_first_admin(&BootstrapAdmin {
            username: "alice".to_string(),
            password: "password123".to_string(),
            display_name: "Alice".to_string(),
        })
        .expect("bootstrap admin");
    let user = store
        .create_user("bob", "Bob", Role::User, "password456")
        .expect("create user");

    let manager = RuntimeManager::new(store.clone(), false);
    let alice_before = manager
        .get_or_start(&admin.user_id)
        .await
        .expect("alice runtime");
    let bob_before = manager
        .get_or_start(&user.user_id)
        .await
        .expect("bob runtime");

    let mut updated = store
        .load_user_config(&user.user_id)
        .expect("load user config");
    updated.channels.send_tool_hints = true;
    store
        .write_user_config(&user.user_id, &updated)
        .expect("write updated config");

    let bob_after = manager.reload(&user.user_id).await.expect("reload bob");
    let alice_after = manager
        .get_or_start(&admin.user_id)
        .await
        .expect("alice runtime");

    assert!(!std::sync::Arc::ptr_eq(&bob_before, &bob_after));
    assert!(std::sync::Arc::ptr_eq(&alice_before, &alice_after));
}
