# Config TOML Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make all human-edited `nanobot-rs` configuration use `config.toml` while keeping machine-managed runtime state unchanged.

**Architecture:** Keep the Rust `Config` type as the only config schema, but make TOML the canonical on-disk format for root and per-user config. Preserve JSON read compatibility long enough to migrate existing installs, then route the web settings editor through TOML text on the client side while the server continues to operate on structured `Config` values.

**Tech Stack:** Rust (`anyhow`, `serde`, `toml`, `axum`), vanilla JS frontend with Vite/Vitest, one small browser TOML parser/stringifier dependency.

---

## File Map

### Rust

- Modify: `/Users/sage/nanobot/nanobot-rs/src/config/mod.rs`
  - Canonical TOML path resolution, JSON fallback, atomic TOML writes, stale JSON cleanup.
- Modify: `/Users/sage/nanobot/nanobot-rs/src/control/mod.rs`
  - Per-user config paths, user config migration, legacy compatibility.
- Modify: `/Users/sage/nanobot/nanobot-rs/src/cli/mod.rs`
  - `onboard`, `migrate-legacy`, `show-config`, and user-facing path/help output.
- Modify: `/Users/sage/nanobot/nanobot-rs/src/web/api.rs`
  - Accept structured config JSON for saves instead of raw JSON/TOML text, leaving parsing to the browser editor bridge.

### Frontend

- Modify: `/Users/sage/nanobot/nanobot-rs/frontend/package.json`
  - Add TOML parser/stringifier dependency.
- Modify: `/Users/sage/nanobot/nanobot-rs/frontend/src/api.js`
  - Send structured config JSON on save.
- Modify: `/Users/sage/nanobot/nanobot-rs/frontend/src/main.js`
  - Render advanced config as TOML text and parse TOML before save.
- Modify: `/Users/sage/nanobot/nanobot-rs/frontend/index.html`
  - Update editor label from JSON to TOML.
- Modify: `/Users/sage/nanobot/nanobot-rs/frontend/src/i18n.js`
  - Update strings mentioning JSON.

### Tests

- Modify: `/Users/sage/nanobot/nanobot-rs/tests/providers.rs`
  - Config path precedence and TOML save behavior.
- Modify: `/Users/sage/nanobot/nanobot-rs/tests/control.rs`
  - Per-user `config.toml` migration and control-store behavior.
- Modify: `/Users/sage/nanobot/nanobot-rs/tests/cli.rs`
  - CLI bootstrap, legacy migration, and `show-config` TOML output.
- Modify: `/Users/sage/nanobot/nanobot-rs/tests/web_server.rs`
  - Web config save path uses structured config payloads and persists TOML-backed config.
- Modify: `/Users/sage/nanobot/nanobot-rs/frontend/test/render.test.js`
  - UI labels and source wiring for TOML editor behavior.

### Reference Spec

- Read: `/Users/sage/nanobot/nanobot-rs/docs/superpowers/specs/2026-04-01-config-toml-design.md`

## Task 1: Canonicalize Config File Resolution and TOML Persistence

**Files:**
- Modify: `/Users/sage/nanobot/nanobot-rs/src/config/mod.rs`
- Test: `/Users/sage/nanobot/nanobot-rs/tests/providers.rs`

- [ ] **Step 1: Write the failing config-path tests**

```rust
#[test]
fn load_config_prefers_toml_when_both_files_exist() {
    let dir = tempdir().expect("tempdir");
    let toml_path = dir.path().join("config.toml");
    let json_path = dir.path().join("config.json");
    std::fs::write(&toml_path, toml_fixture("codex:gpt-5.4")).expect("write toml");
    std::fs::write(&json_path, json_fixture("openai:gpt-4.1-mini")).expect("write json");

    let config = load_config(Some(&toml_path)).expect("load config");
    assert_eq!(config.agents.defaults.default_profile, "codex:gpt-5.4");
}

#[test]
fn save_config_to_toml_replaces_stale_json_copy() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(dir.path().join("config.json"), "{}").expect("write stale json");

    save_config(&Config::default(), Some(&path)).expect("save config");

    assert!(path.exists());
    assert!(!dir.path().join("config.json").exists());
}
```

- [ ] **Step 2: Run the targeted Rust tests and confirm they fail**

Run: `cargo test --test providers`
Expected: FAIL because `default_config_path()` still points at `config.json` and `save_config()` does not remove the legacy JSON file.

- [ ] **Step 3: Implement canonical TOML helpers in the config layer**

```rust
pub fn default_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".nanobot-rs")
        .join("config.toml")
}

fn legacy_config_path(path: &Path) -> Option<PathBuf> {
    (path.file_name() == Some(OsStr::new("config.toml")))
        .then(|| path.with_file_name("config.json"))
}
```

Implementation notes:
- make no-format path resolution prefer `config.toml`, then `config.json`
- keep `load_config_from_str()` dual-format by extension
- make `save_config()` write `config.toml.tmp`, `rename`, then delete `config.json` when the target is canonical TOML
- do not touch callers yet

- [ ] **Step 4: Re-run the targeted Rust tests**

Run: `cargo test --test providers`
Expected: PASS with TOML-first resolution and stale JSON cleanup covered.

- [ ] **Step 5: Commit the config-layer change**

```bash
git add src/config/mod.rs tests/providers.rs
git commit -m "refactor: make toml the canonical config format"
```

## Task 2: Move Control-Plane and CLI User Config Paths to `config.toml`

**Files:**
- Modify: `/Users/sage/nanobot/nanobot-rs/src/control/mod.rs`
- Modify: `/Users/sage/nanobot/nanobot-rs/src/cli/mod.rs`
- Test: `/Users/sage/nanobot/nanobot-rs/tests/control.rs`
- Test: `/Users/sage/nanobot/nanobot-rs/tests/cli.rs`

- [ ] **Step 1: Write failing control and CLI tests for TOML paths**

```rust
#[test]
fn bootstrap_first_admin_creates_user_config_toml() {
    let dir = tempdir().expect("tempdir");
    let store = ControlStore::new(dir.path()).expect("control store");
    let admin = store.bootstrap_first_admin(&bootstrap_admin()).expect("bootstrap");

    assert!(store.user_dir(&admin.user_id).join("config.toml").exists());
    assert!(!store.user_dir(&admin.user_id).join("config.json").exists());
}

#[test]
fn users_show_config_prints_toml() {
    let output = Command::new(env!("CARGO_BIN_EXE_nanobot-rs"))
        .args(["--root", dir.path().to_str().unwrap(), "users", "show-config", "--username", "bob"])
        .output()
        .expect("show config");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[agents.defaults]"));
}
```

- [ ] **Step 2: Run the targeted tests and confirm they fail**

Run: `cargo test --test control --test cli`
Expected: FAIL because `ControlStore::user_config_path()` still returns `config.json`, legacy bootstrap defaults to JSON, and `show-config` emits pretty JSON.

- [ ] **Step 3: Implement TOML canonical paths and legacy migration**

```rust
pub fn user_config_path(&self, user_id: &str) -> PathBuf {
    self.user_dir(user_id).join("config.toml")
}

fn legacy_root_config(root: &Path) -> PathBuf {
    let toml = root.join("config.toml");
    if toml.exists() { toml } else { root.join("config.json") }
}
```

Implementation notes:
- update `ControlStore::user_config_path()`
- let `load_user_config()` and `write_user_config()` rely on the canonical TOML path
- change onboarding and `migrate-legacy` to prefer `config.toml`, then fall back to `config.json`
- keep `users show-config` human-facing by serializing to TOML with `toml::to_string_pretty`
- make assertions in tests check that migrated user config lands in `config.toml`

- [ ] **Step 4: Re-run the targeted control and CLI tests**

Run: `cargo test --test control --test cli`
Expected: PASS with multi-user config files created as TOML and legacy JSON migrated away.

- [ ] **Step 5: Commit the control-plane and CLI change**

```bash
git add src/control/mod.rs src/cli/mod.rs tests/control.rs tests/cli.rs
git commit -m "feat: migrate user config paths to toml"
```

## Task 3: Switch the Settings Editor from JSON Text to TOML Text

**Files:**
- Modify: `/Users/sage/nanobot/nanobot-rs/src/web/api.rs`
- Modify: `/Users/sage/nanobot/nanobot-rs/frontend/package.json`
- Modify: `/Users/sage/nanobot/nanobot-rs/frontend/src/api.js`
- Modify: `/Users/sage/nanobot/nanobot-rs/frontend/src/main.js`
- Modify: `/Users/sage/nanobot/nanobot-rs/frontend/index.html`
- Modify: `/Users/sage/nanobot/nanobot-rs/frontend/src/i18n.js`
- Test: `/Users/sage/nanobot/nanobot-rs/tests/web_server.rs`
- Test: `/Users/sage/nanobot/nanobot-rs/frontend/test/render.test.js`

- [ ] **Step 1: Write the failing web and frontend tests**

```rust
#[tokio::test]
async fn put_my_config_accepts_structured_json_and_persists_toml_backing_file() {
    let body = serde_json::json!({
        "agents": { "defaults": { "defaultProfile": "codex:gpt-5.4" } },
        "channels": { "telegram": { "enabled": false, "token": "" } }
    });

    let response = app.oneshot(
        Request::builder()
            .method("PUT")
            .uri("/api/me/config")
            .header("content-type", "application/json")
            .header("cookie", cookie)
            .body(Body::from(body.to_string()))
            .unwrap(),
    ).await.expect("put config");

    assert_eq!(response.status(), StatusCode::OK);
}
```

```js
it("labels the advanced settings editor as TOML and parses TOML on submit", async () => {
  expect(html).toContain('data-i18n="settings_advanced_title"');
  expect(TRANSLATIONS.en.settings_advanced_title).toBe("Advanced TOML");
  expect(js).toContain("TOML.parse");
  expect(js).toContain("TOML.stringify");
});
```

- [ ] **Step 2: Run the focused Rust and frontend tests and confirm they fail**

Run: `cargo test --test web_server`
Expected: FAIL because `put_my_config()` still parses raw text as `config.json`.

Run: `cd frontend && npm test -- render.test.js`
Expected: FAIL because the UI still says `Advanced JSON` and uses `JSON.parse` / `JSON.stringify`.

- [ ] **Step 3: Implement the TOML editor bridge**

```js
import TOML from "@iarna/toml";

function stableTomlConfig(config) {
  return TOML.stringify(config || {});
}

const parsed = TOML.parse(configEditor.value || "");
await updateMyConfig(applyStructuredSettings(parsed));
```

```rust
pub async fn put_my_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(config): Json<Config>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // validate + persist as TOML-backed config
}
```

Implementation notes:
- add `@iarna/toml` to `frontend/package.json`
- rename the label and i18n strings from JSON to TOML
- render `configEditor` from `TOML.stringify(currentConfig)`
- parse TOML in the browser before submit; keep structured controls merged in before sending
- change `updateMyConfig()` to `JSON.stringify(nextConfig)` and keep `Content-Type: application/json`
- change `put_my_config()` to accept `Json<Config>` instead of `String`

- [ ] **Step 4: Re-run the focused web and frontend tests**

Run: `cargo test --test web_server`
Expected: PASS with config saves still validated on the server.

Run: `cd frontend && npm test -- render.test.js`
Expected: PASS with TOML wording and parser wiring covered.

- [ ] **Step 5: Commit the web/editor change**

```bash
git add src/web/api.rs frontend/package.json frontend/src/api.js frontend/src/main.js frontend/index.html frontend/src/i18n.js tests/web_server.rs frontend/test/render.test.js
git commit -m "feat: switch settings editor to toml"
```

## Task 4: Run Full Regression and Close the Migration Loop

**Files:**
- Modify as needed: `/Users/sage/nanobot/nanobot-rs/tests/control.rs`
- Modify as needed: `/Users/sage/nanobot/nanobot-rs/tests/cli.rs`
- Modify as needed: `/Users/sage/nanobot/nanobot-rs/tests/providers.rs`
- Modify as needed: `/Users/sage/nanobot/nanobot-rs/tests/web_server.rs`
- Modify as needed: `/Users/sage/nanobot/nanobot-rs/frontend/test/render.test.js`

- [ ] **Step 1: Add any missing regression assertions discovered during implementation**

```rust
assert!(dir.path().join("users").join(user_id).join("config.toml").exists());
assert!(!dir.path().join("users").join(user_id).join("config.json").exists());
```

- [ ] **Step 2: Run the full Rust regression suite**

Run: `cargo test`
Expected: PASS with CLI, control-plane, providers, and web tests all green.

- [ ] **Step 3: Run the full frontend regression suite**

Run: `cd frontend && npm test && npm run build`
Expected: PASS with the settings editor rendered and bundled as TOML-backed UI.

- [ ] **Step 4: Manually smoke-test one migrated root and one multi-user root**

Run:

```bash
ROOT="$(mktemp -d)"
cargo run -- --root "$ROOT" onboard --admin-username alice --admin-password password123
find "$ROOT" -maxdepth 3 | sort
```

Expected:
- `users/<id>/config.toml` exists
- `users/<id>/config.json` does not exist
- `control/*.json` still exists for machine-managed state

- [ ] **Step 5: Commit the regression pass**

```bash
git add tests/control.rs tests/cli.rs tests/providers.rs tests/web_server.rs frontend/test/render.test.js
git commit -m "test: cover toml config migration regressions"
```
