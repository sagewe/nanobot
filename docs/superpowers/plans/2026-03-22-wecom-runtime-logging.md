# WeCom Runtime Logging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add operator-facing WeCom runtime logs so `gateway` clearly shows connection, subscription, callback, reply, and reconnect activity.

**Architecture:** Keep the change local to the existing WeCom channel implementation. Add `info` logs for key lifecycle transitions and `debug` logs for high-frequency diagnostics, then verify them through a focused log-capture test built on the existing mock WeCom server.

**Tech Stack:** Rust, Tokio, Tracing, `tracing-subscriber`, existing WeCom mock WebSocket tests

---

### Task 1: Add WeCom Lifecycle Log Coverage

**Files:**
- Modify: `/Users/sage/nanobot/nanobot-rs/tests/wecom.rs`
- Modify: `/Users/sage/nanobot/nanobot-rs/src/channels/wecom.rs`

- [x] **Step 1: Write the failing log-capture test**

Extend `tests/wecom.rs` with a focused tracing capture test that reuses the mock WeCom server flow and asserts these `info` logs appear during a healthy session:
- `wecom connecting to`
- `wecom websocket connected`
- `wecom subscribe acknowledged`
- `wecom text callback sender=... chat=...`
- `wecom reply sent chat=...`

The test should capture logs with a local `tracing_subscriber` writer, similar to `tests/web_logging.rs`.

- [x] **Step 2: Run the targeted test to verify it fails**

Run: `cargo test --target-dir /tmp/nanobot-rs-target --test wecom wecom_logs_connection_lifecycle`
Expected: FAIL because the current WeCom channel does not emit the positive lifecycle logs.

- [x] **Step 3: Implement the minimal `info` logging**

Add `info!` logs in `src/channels/wecom.rs` for:

```rust
info!("wecom connecting to {}", self.config.ws_base);
info!("wecom websocket connected");
info!("wecom subscribe acknowledged");
info!("wecom text callback sender={} chat={}", parsed.sender_id, parsed.chat_id);
info!("wecom reply sent chat={}", msg.chat_id);
info!("wecom reconnecting in {:?} after: {}", self.timing.reconnect_delay, error);
info!("wecom channel stopped");
```

Do not log message bodies, secrets, or full payloads.

- [x] **Step 4: Re-run the targeted test**

Run: `cargo test --target-dir /tmp/nanobot-rs-target --test wecom wecom_logs_connection_lifecycle`
Expected: PASS with the new lifecycle logs present.

- [x] **Step 5: Commit**

```bash
git add /Users/sage/nanobot/nanobot-rs/src/channels/wecom.rs /Users/sage/nanobot/nanobot-rs/tests/wecom.rs
git commit -m "feat: add wecom runtime logs"
```

### Task 2: Add Debug Diagnostics And Full Verification

**Files:**
- Modify: `/Users/sage/nanobot/nanobot-rs/tests/wecom.rs`
- Modify: `/Users/sage/nanobot/nanobot-rs/src/channels/wecom.rs`

- [x] **Step 1: Write the failing diagnostic assertions**

Extend the WeCom logging test coverage so debug-level capture verifies:
- `wecom pong received`
- `wecom reply context updated chat=... req_id=...`
- `dropping wecom message from blocked sender ...`

Keep these assertions in one focused test. Do not assert unstable UUIDs or full error strings.

- [x] **Step 2: Run the targeted tests to verify they fail**

Run: `cargo test --target-dir /tmp/nanobot-rs-target --test wecom`
Expected: FAIL because the channel currently lacks the new debug diagnostics.

- [x] **Step 3: Implement the minimal `debug` logging**

Add `debug!` logs in `src/channels/wecom.rs` for:

```rust
debug!("wecom pong received");
debug!("wecom reply context updated chat={} req_id={}", parsed.chat_id, parsed.req_id);
debug!("dropping wecom message from blocked sender {}", parsed.sender_id);
```

Do not add `ping` logs or raw payload dumps.

- [x] **Step 4: Run full verification**

Run:

```bash
cargo fmt
cargo test --target-dir /tmp/nanobot-rs-target --test wecom
cargo test --target-dir /tmp/nanobot-rs-target
```

Expected: PASS for the WeCom logging tests and then PASS for the full Rust suite.

- [x] **Step 5: Commit**

```bash
git add /Users/sage/nanobot/nanobot-rs/src/channels/wecom.rs /Users/sage/nanobot/nanobot-rs/tests/wecom.rs
git commit -m "test: cover wecom logging diagnostics"
```
