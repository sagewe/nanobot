use std::env;
use std::fs;
use std::sync::{Mutex, OnceLock};

use nanobot_rs::providers::{CodexProvider, CodexProviderConfig};
use tempfile::tempdir;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn with_openai_api_key<F>(value: &str, f: F)
where
    F: FnOnce(),
{
    let _guard = env_lock().lock().expect("env lock");
    let previous = env::var_os("OPENAI_API_KEY");

    unsafe {
        env::set_var("OPENAI_API_KEY", value);
    }

    f();

    match previous {
        Some(previous) => unsafe {
            env::set_var("OPENAI_API_KEY", previous);
        },
        None => unsafe {
            env::remove_var("OPENAI_API_KEY");
        },
    }
}

fn write_auth_file(dir: &tempfile::TempDir, content: &str) -> String {
    let path = dir.path().join("auth.json");
    fs::write(&path, content).expect("write auth file");
    path.display().to_string()
}

fn with_home_dir<F>(home_dir: &str, f: F)
where
    F: FnOnce(),
{
    let _guard = env_lock().lock().expect("env lock");
    let previous_home = env::var_os("HOME");

    unsafe {
        env::set_var("HOME", home_dir);
    }

    f();

    match previous_home {
        Some(previous) => unsafe {
            env::set_var("HOME", previous);
        },
        None => unsafe {
            env::remove_var("HOME");
        },
    }
}

#[test]
fn codex_provider_rejects_missing_auth_file_and_ignores_openai_api_key() {
    with_openai_api_key("sk-test-openai-key", || {
        let err = CodexProvider::from_config(CodexProviderConfig {
            auth_file: "/tmp/does-not-exist-codex-auth.json".to_string(),
            api_base: "https://chatgpt.com/backend-api".to_string(),
        })
        .expect_err("missing auth file should fail");

        assert!(err.to_string().contains("auth file"));
    });
}

#[test]
fn codex_provider_rejects_malformed_json() {
    let dir = tempdir().expect("tempdir");
    let auth_file = write_auth_file(&dir, "{");

    with_openai_api_key("sk-test-openai-key", || {
        let err = CodexProvider::from_config(CodexProviderConfig {
            auth_file,
            api_base: "https://chatgpt.com/backend-api".to_string(),
        })
        .expect_err("malformed auth json should fail");

        let message = err.to_string();
        assert!(message.contains("parse"));
        assert!(!message.contains("OPENAI_API_KEY"));
    });
}

#[test]
fn codex_provider_rejects_unreadable_existing_auth_path() {
    let dir = tempdir().expect("tempdir");
    with_openai_api_key("sk-test-openai-key", || {
        let err = CodexProvider::from_config(CodexProviderConfig {
            auth_file: dir.path().display().to_string(),
            api_base: "https://chatgpt.com/backend-api".to_string(),
        })
        .expect_err("directory path should fail to read");

        assert!(err.to_string().contains("read"));
    });
}

#[test]
fn codex_provider_expands_home_directory_in_auth_file_path() {
    let home = tempdir().expect("tempdir");
    let auth_path = home.path().join("auth.json");
    fs::write(
        &auth_path,
        r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "access-token",
    "refresh_token": "refresh-token",
    "id_token": "id-token",
    "account_id": "account-id"
  }
}"#,
    )
    .expect("write auth file");

    with_home_dir(home.path().to_str().expect("home dir path"), || {
        let provider = CodexProvider::from_config(CodexProviderConfig {
            auth_file: "~/auth.json".to_string(),
            api_base: "https://chatgpt.com/backend-api".to_string(),
        })
        .expect("home directory auth path should load");

        assert_eq!(provider.auth_path(), auth_path.as_path());
        assert_eq!(provider.api_base(), "https://chatgpt.com/backend-api");
    });
}

#[test]
fn codex_provider_rejects_non_chatgpt_auth_mode_and_ignores_openai_api_key() {
    let dir = tempdir().expect("tempdir");
    let auth_file = write_auth_file(
        &dir,
        r#"{
  "auth_mode": "api_key",
  "tokens": {
    "access_token": "access-token",
    "refresh_token": "refresh-token",
    "id_token": "id-token",
    "account_id": "account-id"
  }
}"#,
    );

    with_openai_api_key("sk-test-openai-key", || {
        let err = CodexProvider::from_config(CodexProviderConfig {
            auth_file,
            api_base: "https://chatgpt.com/backend-api".to_string(),
        })
        .expect_err("invalid auth mode should fail");

        let message = err.to_string();
        assert!(message.contains("auth_mode"));
        assert!(message.contains("chatgpt"));
        assert!(!message.contains("OPENAI_API_KEY"));
    });
}

#[test]
fn codex_provider_rejects_missing_required_token_fields() {
    let cases = [
        (
            "access_token",
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "refresh_token": "refresh-token",
    "id_token": "id-token",
    "account_id": "account-id"
  }
}"#,
        ),
        (
            "refresh_token",
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "access-token",
    "id_token": "id-token",
    "account_id": "account-id"
  }
}"#,
        ),
        (
            "id_token",
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "access-token",
    "refresh_token": "refresh-token",
    "account_id": "account-id"
  }
}"#,
        ),
        (
            "account_id",
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "access-token",
    "refresh_token": "refresh-token",
    "id_token": "id-token"
  }
}"#,
        ),
    ];

    for (missing_field, content) in cases {
        let dir = tempdir().expect("tempdir");
        let auth_file = write_auth_file(&dir, content);
        let err = CodexProvider::from_config(CodexProviderConfig {
            auth_file,
            api_base: "https://chatgpt.com/backend-api".to_string(),
        })
        .expect_err("missing token field should fail");

        let message = err.to_string();
        assert!(
            message.contains(missing_field),
            "expected error to mention missing field {missing_field}, got: {message}"
        );
    }
}

#[test]
fn codex_provider_rejects_empty_required_token_fields() {
    let cases = [
        (
            "access_token",
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "",
    "refresh_token": "refresh-token",
    "id_token": "id-token",
    "account_id": "account-id"
  }
}"#,
        ),
        (
            "refresh_token",
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "access-token",
    "refresh_token": "",
    "id_token": "id-token",
    "account_id": "account-id"
  }
}"#,
        ),
        (
            "id_token",
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "access-token",
    "refresh_token": "refresh-token",
    "id_token": "",
    "account_id": "account-id"
  }
}"#,
        ),
        (
            "account_id",
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "access-token",
    "refresh_token": "refresh-token",
    "id_token": "id-token",
    "account_id": ""
  }
}"#,
        ),
    ];

    for (field, content) in cases {
        let dir = tempdir().expect("tempdir");
        let auth_file = write_auth_file(&dir, content);
        let err = CodexProvider::from_config(CodexProviderConfig {
            auth_file,
            api_base: "https://chatgpt.com/backend-api".to_string(),
        })
        .expect_err("empty token field should fail");

        let message = err.to_string();
        assert!(message.contains(field));
        assert!(message.contains("empty"));
    }
}

#[test]
fn codex_provider_loads_valid_auth_file() {
    let dir = tempdir().expect("tempdir");
    let auth_file = write_auth_file(
        &dir,
        r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "access-token",
    "refresh_token": "refresh-token",
    "id_token": "id-token",
    "account_id": "account-id"
  }
}"#,
    );

    let provider = CodexProvider::from_config(CodexProviderConfig {
        auth_file,
        api_base: "https://chatgpt.com/backend-api".to_string(),
    })
    .expect("valid auth file should load");

    assert_eq!(provider.api_base(), "https://chatgpt.com/backend-api");
    assert_eq!(provider.auth_path(), dir.path().join("auth.json").as_path());
}
