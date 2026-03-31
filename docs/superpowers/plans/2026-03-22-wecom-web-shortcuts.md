# WeCom And Web Shortcut Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `Ctrl+Enter` / `Cmd+Enter` send shortcuts to the web UI and add a text-only WeCom smart-bot long-connection channel to `gateway`.

**Architecture:** Keep the current bus/agent/channel split intact. The web change stays isolated to the embedded page script, while WeCom is implemented as a new `Channel` that owns its WebSocket session, maps inbound bot callbacks into `InboundMessage`, and uses cached reply context to send outbound text replies without changing public bus types.

**Tech Stack:** Rust, Tokio, Axum, Reqwest, Serde, Tracing, `tokio-tungstenite`, `futures-util`

---

### Task 1: Add Web Composer Send Shortcuts

**Files:**
- Modify: `/Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/src/web/page.rs`
- Test: `/Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/tests/web_page.rs`

- [x] **Step 1: Write the failing tests**

Add assertions to `tests/web_page.rs` that the rendered page includes a textarea keyboard handler checking both modifier variants and that plain `Enter` does not share that path.

```rust
#[test]
fn page_shell_supports_ctrl_and_cmd_enter_submission() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("messageInput.addEventListener(\"keydown\""));
    assert!(html.contains("event.key === \"Enter\""));
    assert!(html.contains("event.ctrlKey || event.metaKey"));
    assert!(html.contains("composer.requestSubmit()"));
}
```

- [x] **Step 2: Run the targeted tests to verify they fail**

Run: `cargo test --target-dir /tmp/nanobot-rs-target --test web_page`
Expected: FAIL because the page does not yet include the keyboard shortcut handler.

- [x] **Step 3: Implement the minimal page-script change**

Update `src/web/page.rs` so the existing textarea listens for `keydown` and calls `composer.requestSubmit()` only when:

```js
if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
  event.preventDefault();
  composer.requestSubmit();
}
```

Do not duplicate submit logic. Reuse the existing form `submit` path so trim/clear/busy/error behavior stays centralized.

- [x] **Step 4: Re-run the targeted tests**

Run: `cargo test --target-dir /tmp/nanobot-rs-target --test web_page`
Expected: PASS with the new shortcut assertions and all existing page tests still green.

- [x] **Step 5: Commit**

```bash
git add /Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/src/web/page.rs /Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/tests/web_page.rs
git commit -m "feat: add web send shortcuts"
```

### Task 2: Add WeCom Config And Channel Wiring

**Files:**
- Modify: `/Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/src/config/mod.rs`
- Modify: `/Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/src/channels/mod.rs`
- Create: `/Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/src/channels/wecom.rs`
- Modify: `/Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/tests/channels.rs`
- Modify: `/Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/tests/cli.rs`

- [x] **Step 1: Write the failing config and manager tests**

Extend `tests/channels.rs` and `tests/cli.rs` to cover:
- `ChannelManager` does not register WeCom when disabled
- `ChannelManager` does register WeCom when enabled
- `onboard` config output contains the `channels.wecom` block with defaults

```rust
#[tokio::test]
async fn channel_manager_registers_wecom_when_enabled() {
    let mut config = Config::default();
    config.channels.wecom.enabled = true;
    config.channels.wecom.bot_id = "bot".to_string();
    config.channels.wecom.secret = "secret".to_string();

    let manager = ChannelManager::new(&config, MessageBus::new(32));
    assert!(manager.enabled_channels().contains(&"wecom".to_string()));
}
```

- [x] **Step 2: Run the targeted tests to verify they fail**

Run: `cargo test --target-dir /tmp/nanobot-rs-target --test channels --test cli`
Expected: FAIL because `Config` and `ChannelManager` do not yet know about WeCom.

- [x] **Step 3: Implement config and registration**

Add `WecomConfig` to `src/config/mod.rs`:

```rust
pub struct WecomConfig {
    pub enabled: bool,
    pub bot_id: String,
    pub secret: String,
    pub ws_base: String,
    pub allow_from: Vec<String>,
}
```

Wire it into `ChannelsConfig::default()` and into `ChannelManager::new()`. Create `src/channels/wecom.rs` with a temporary no-op `WecomBotChannel` placeholder that satisfies the trait and lets the wiring tests compile.

- [x] **Step 4: Re-run the targeted tests**

Run: `cargo test --target-dir /tmp/nanobot-rs-target --test channels --test cli`
Expected: PASS with the WeCom config defaults and manager registration checks.

- [x] **Step 5: Commit**

```bash
git add /Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/src/config/mod.rs /Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/src/channels/mod.rs /Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/src/channels/wecom.rs /Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/tests/channels.rs /Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/tests/cli.rs
git commit -m "feat: wire wecom channel configuration"
```

### Task 3: Build WeCom Protocol Primitives

**Files:**
- Modify: `/Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/Cargo.toml`
- Modify: `/Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/src/channels/wecom.rs`
- Modify: `/Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/src/channels/mod.rs`
- Test: `/Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/tests/wecom.rs`

- [x] **Step 1: Write the failing protocol tests**

Create `tests/wecom.rs` with focused tests for:
- subscribe payload includes `cmd = "aibot_subscribe"` and credentials
- heartbeat payload uses the documented ping command shape
- inbound text callback parsing extracts `userid`, conversation id, and text
- outbound reply payload uses the reply command for normal text messages

```rust
#[test]
fn subscribe_request_contains_bot_credentials() {
    let request = wecom::build_subscribe_request("bot-id", "secret", "req-1");
    assert_eq!(request["cmd"], "aibot_subscribe");
    assert_eq!(request["body"]["botid"], "bot-id");
    assert_eq!(request["body"]["secret"], "secret");
}
```

- [x] **Step 2: Run the targeted tests to verify they fail**

Run: `cargo test --target-dir /tmp/nanobot-rs-target --test wecom`
Expected: FAIL because the WeCom module and protocol helpers do not exist.

- [x] **Step 3: Implement the protocol layer**

Add `tokio-tungstenite` and `futures-util` in `Cargo.toml`, then build `src/channels/wecom.rs` with:
- request/response structs or helper builders for:
  - `aibot_subscribe`
  - heartbeat ping
  - `aibot_respond_msg`
- inbound parsing helpers for:
  - `aibot_msg_callback`
  - supported text body extraction
- a small reply-context type keyed by `chat_id`

Keep this file focused on protocol and channel-local state. Do not put manager wiring or unrelated channel dispatch logic here.

- [x] **Step 4: Re-run the targeted tests**

Run: `cargo test --target-dir /tmp/nanobot-rs-target --test wecom`
Expected: PASS with the protocol builder/parser tests.

- [x] **Step 5: Commit**

```bash
git add /Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/Cargo.toml /Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/src/channels/wecom.rs /Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/src/channels/mod.rs /Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/tests/wecom.rs
git commit -m "feat: add wecom protocol primitives"
```

### Task 4: Implement WeCom Runtime Behavior And Full Verification

**Files:**
- Modify: `/Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/src/channels/wecom.rs`
- Modify: `/Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/src/channels/mod.rs`
- Modify: `/Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/tests/wecom.rs`
- Modify: `/Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/tests/channels.rs`

- [x] **Step 1: Write the failing runtime tests**

Extend `tests/wecom.rs` with a local mock WebSocket server that verifies:
- startup fails clearly when credentials are missing
- successful subscribe publishes text callbacks to `MessageBus`
- `allowFrom` filtering is enforced
- `send(OutboundMessage)` uses cached reply context
- heartbeat timeout or disconnect triggers reconnect instead of killing the loop

```rust
#[tokio::test]
async fn wecom_channel_publishes_text_callback_to_bus() {
    let (channel, bus, mock) = spawn_mock_wecom_channel().await;
    mock.push_text_callback("alice", "conv-1", "hello").await;

    let inbound = bus.consume_inbound().await.expect("message");
    assert_eq!(inbound.channel, "wecom");
    assert_eq!(inbound.sender_id, "alice");
    assert_eq!(inbound.chat_id, "conv-1");
    assert_eq!(inbound.content, "hello");
}
```

- [x] **Step 2: Run the targeted tests to verify they fail**

Run: `cargo test --target-dir /tmp/nanobot-rs-target --test wecom --test channels`
Expected: FAIL because the placeholder channel does not yet manage a real connection lifecycle.

- [x] **Step 3: Implement the runtime loop**

Finish `WecomBotChannel` with:
- credential validation in `start()`
- WebSocket connect + subscribe handshake
- heartbeat task using the documented 30-second cadence
- capped exponential reconnect with jitter
- inbound message parsing and `publish_inbound()`
- reply-context cache updates per accepted callback
- outbound text replies via `aibot_respond_msg`

Keep unsupported event/message types as logged no-ops. Do not add markdown, media, cards, or streaming in this task.

- [x] **Step 4: Run full verification**

Run:

```bash
cargo test --target-dir /tmp/nanobot-rs-target --test web_page --test channels --test wecom
cargo test --target-dir /tmp/nanobot-rs-target
```

Expected: PASS for the new targeted suites and then PASS for the full Rust test suite.

- [x] **Step 5: Commit**

```bash
git add /Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/src/channels/wecom.rs /Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/src/channels/mod.rs /Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/tests/wecom.rs /Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/tests/channels.rs /Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/src/web/page.rs /Users/sage/nanobot/.worktrees/wecom-web-shortcuts/nanobot-rs/tests/web_page.rs
git commit -m "feat: add wecom bot channel"
```
