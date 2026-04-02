# Session Parallelism and Message Debounce Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the global agent lock with session-scoped concurrency, add session-level message debounce, and make outbound channel delivery concurrent while preserving FIFO per destination.

**Architecture:** Keep the agent’s correctness boundary at the session level. Add a session lock table and a session ingress buffer in `AgentLoop`, then replace the global outbound bottleneck in `ChannelManager` with tracked delivery-key workers. Control commands bypass debounce, and `/stop` is the only command that preempts the running task for its session.

**Tech Stack:** Rust, Tokio async tasks and channels, existing `AgentLoop`, `MessageBus`, `ChannelManager`, existing test suites in `tests/agent.rs` and `tests/channels.rs`

---

### Task 1: Add configuration coverage for message debounce

**Files:**
- Modify: `<repo-root>/src/config/mod.rs`
- Test: `<repo-root>/tests/providers.rs`
- Test: `<repo-root>/tests/model_profiles.rs`

- [x] **Step 1: Write the failing config test**

Add a test that asserts:

```rust
let value = serde_json::to_value(Config::default()).unwrap();
assert_eq!(
    value.pointer("/agents/defaults/messageDebounceMs").and_then(Value::as_u64),
    Some(0)
);
```

And a load test that accepts a non-zero value:

```rust
assert_eq!(config.agents.defaults.message_debounce_ms, 1500);
```

- [x] **Step 2: Run the targeted tests to verify they fail**

Run:

```bash
cd <repo-root>
cargo test --target-dir /tmp/sidekick-target-session-debounce --test providers --test model_profiles
```

Expected:
- failure because `messageDebounceMs` does not exist yet

- [x] **Step 3: Implement the minimal config change**

In `<repo-root>/src/config/mod.rs`:
- add `message_debounce_ms: u64` to `AgentDefaults`
- set default to `0`
- include it in raw config deserialization
- include it in serialized defaults output as `messageDebounceMs`

- [x] **Step 4: Run the targeted tests again**

Run:

```bash
cd <repo-root>
cargo test --target-dir /tmp/sidekick-target-session-debounce --test providers --test model_profiles
```

Expected:
- both suites pass

- [x] **Step 5: Commit**

```bash
git add <repo-root>/src/config/mod.rs \
        <repo-root>/tests/providers.rs \
        <repo-root>/tests/model_profiles.rs
git commit -m "feat: add session debounce config"
```

### Task 2: Replace the global processing lock with session-scoped locking

**Files:**
- Modify: `<repo-root>/src/agent/mod.rs`
- Test: `<repo-root>/tests/agent.rs`

- [x] **Step 1: Write the failing concurrency test**

Add a test that:
- sends one message to `session-a`
- sends one message to `session-b`
- uses a provider double that blocks the first session mid-flight
- asserts the second session can still complete before the first is released

Use a provider pattern similar to the existing concurrency helpers in `<repo-root>/tests/agent.rs`.

- [x] **Step 2: Write the failing same-session serialization test**

Add a test that:
- sends two messages to the same session
- uses a provider double that records call ordering
- asserts the second message does not begin processing until the first same-session turn releases

- [x] **Step 3: Run the targeted test to verify failure**

- [x] **Step 3: Write the failing web direct same-session serialization test**

Add a test that:
- exercises `process_direct()` or `process_direct_logged()` directly
- sends two overlapping direct requests for the same `session_key`
- uses a provider double that blocks the first request
- asserts the second same-session direct request does not overlap it

- [x] **Step 4: Run the targeted test to verify failure**

- [x] **Step 4: Write the failing web direct cross-session concurrency test**

Add a test that:
- exercises `process_direct()` or `process_direct_logged()` directly
- sends overlapping direct requests for two different `session_key`s
- uses a provider double that blocks one direct request
- asserts the other session’s direct request can still complete

- [x] **Step 5: Run the targeted test to verify failure**

Run:

```bash
cd <repo-root>
cargo test --target-dir /tmp/sidekick-target-session-lock --test agent
```

Expected:
- cross-session concurrency test fails because the global lock still serializes everything
- direct same-session overlap test fails until the direct path also shares the session lock discipline
- direct cross-session test fails until direct traffic is also using session-scoped locking

- [x] **Step 6: Implement session lock lookup**

In `<repo-root>/src/agent/mod.rs`:
- replace `processing_lock: Arc<Mutex<()>>` with a session lock table
- add a helper like `session_lock(&self, session_key: &str) -> Arc<Mutex<()>>`
- acquire only the lock for `msg.session_key()` inside `dispatch()`
- apply the same session lock discipline to `process_direct()` / `process_direct_logged()` so web direct traffic cannot overlap within one session either

- [x] **Step 7: Run the agent tests again**

Run:

```bash
cd <repo-root>
cargo test --target-dir /tmp/sidekick-target-session-lock --test agent
```

Expected:
- new concurrency tests pass
- existing agent tests stay green

- [x] **Step 8: Commit**

```bash
git add <repo-root>/src/agent/mod.rs \
        <repo-root>/tests/agent.rs
git commit -m "refactor: use session-scoped agent locks"
```

### Task 3: Add session ingress debounce with command bypass

**Files:**
- Modify: `<repo-root>/src/agent/mod.rs`
- Test: `<repo-root>/tests/agent.rs`

- [x] **Step 1: Write the failing burst merge test**

Add a test that:
- enables `messageDebounceMs = 1500`
- sends three normal user messages to the same session inside the window
- uses a provider double to capture the final user content
- asserts there is only one provider call
- asserts the final user content contains:

```text
[Compressed user burst]
1. ...
2. ...
3. ...
```

- [x] **Step 2: Write the failing out-of-window split test**

Add a test that:
- enables debounce
- sends two messages to the same session with a gap larger than the debounce window
- asserts two provider calls occur

- [x] **Step 3: Write the failing command bypass tests**

Add tests for:
- `/help` does not get merged into a normal user burst
- `/models` does not get merged into a normal user burst
- `/model ...` does not get merged into a normal user burst
- `/new` clears any pending buffered burst before resetting the session
- `/stop` bypasses debounce and cancels the current same-session task
- a bypassed command arriving while a debounce timer is already pending is dispatched immediately rather than waiting behind the buffered burst

- [x] **Step 4: Write the failing cross-session isolation tests**

Add tests that:
- send burst traffic to two distinct session keys within the same channel transport, for example `telegram:chat-a` and `telegram:chat-b`, inside the debounce window
- assert they are never merged together
- assert the persisted session history for the merged session contains one merged user turn rather than separate raw turns

- [x] **Step 5: Run the targeted agent tests to verify failure**

Run:

```bash
cd <repo-root>
cargo test --target-dir /tmp/sidekick-target-ingress --test agent
```

Expected:
- debounce and command-bypass tests fail before implementation

- [x] **Step 6: Implement `SessionIngressBuffer`**

In `<repo-root>/src/agent/mod.rs`:
- add a session-scoped pending-burst structure
- collect ordinary user messages by `session_key`
- arm a debounce timer only when `messageDebounceMs > 0`
- emit one merged `InboundMessage` after the window closes
- persist only the merged user turn, not the raw burst as separate turns

- [x] **Step 6: Implement command bypass semantics**

Implement the agreed rules:
- `/stop`
  - bypasses debounce
  - aborts the running same-session task
  - clears any pending buffered burst for that session
- `/new`, `/help`, `/models`, `/model ...`
  - bypass debounce
  - do not merge with ordinary text
  - do not preempt an already-running same-session task
  - `/new` also clears any pending buffered burst for that session before reset semantics take effect

- [x] **Step 7: Re-run the targeted agent tests**

Run:

```bash
cd <repo-root>
cargo test --target-dir /tmp/sidekick-target-ingress --test agent
```

Expected:
- new debounce and command tests pass
- existing agent tests stay green

- [x] **Step 8: Commit**

```bash
git add <repo-root>/src/agent/mod.rs \
        <repo-root>/tests/agent.rs
git commit -m "feat: add session ingress debounce"
```

### Task 4: Replace global outbound serialization with delivery-key workers

**Files:**
- Modify: `<repo-root>/src/channels/mod.rs`
- Test: `<repo-root>/tests/channels.rs`

- [x] **Step 1: Write the failing outbound concurrency test**

Add a test channel double that:
- blocks `send()` for one `channel:chat_id`
- immediately succeeds for another
- asserts the second destination is not blocked by the first

- [x] **Step 2: Write the failing FIFO-per-destination test**

Add a test that:
- enqueues two outbound messages for the same `channel:chat_id`
- uses a channel double that records send order
- asserts the messages are sent FIFO for that destination

- [x] **Step 3: Write the failing bounded-worker behavior test**

Add a test that:
- constrains a worker queue to a tiny capacity in the test setup
- asserts that overflow does not stall a different delivery key
- asserts the overflowed enqueue is explicitly surfaced via drop/log path or equivalent observable behavior

- [x] **Step 4: Run the targeted channel tests to verify failure**

Run:

```bash
cd <repo-root>
cargo test --target-dir /tmp/sidekick-target-outbound --test channels
```

Expected:
- concurrency/FIFO tests fail before implementation

- [x] **Step 5: Implement delivery-key workers**

In `<repo-root>/src/channels/mod.rs`:
- derive `delivery_key = "{channel}:{chat_id}"`
- keep a tracked worker table
- give each worker a bounded queue
- preserve FIFO inside one worker
- allow different workers to run concurrently
- ensure workers retire after idle timeout and remove themselves from the table
- ensure `stop_all()` stops dispatch plus active workers

- [x] **Step 6: Add lifecycle tests for worker retirement and shutdown**

Add tests that:
- confirm an idle worker removes itself from the tracked table after the idle timeout
- confirm `stop_all()` stops active workers and does not leave delivery-key workers running after shutdown

- [x] **Step 7: Re-run the targeted channel tests**

Run:

```bash
cd <repo-root>
cargo test --target-dir /tmp/sidekick-target-outbound --test channels
```

Expected:
- outbound concurrency tests pass
- existing channel tests remain green

- [x] **Step 8: Commit**

```bash
git add <repo-root>/src/channels/mod.rs \
        <repo-root>/tests/channels.rs
git commit -m "feat: parallelize outbound delivery by destination"
```

### Task 5: Full verification and integration check

**Files:**
- Verify: `<repo-root>/src/config/mod.rs`
- Verify: `<repo-root>/src/agent/mod.rs`
- Verify: `<repo-root>/src/channels/mod.rs`
- Verify: `<repo-root>/tests/agent.rs`
- Verify: `<repo-root>/tests/channels.rs`

- [x] **Step 1: Run the focused suites together**

Run:

```bash
cd <repo-root>
cargo test --target-dir /tmp/sidekick-target-session-parallelism --test agent --test channels --test providers --test model_profiles
```

Expected:
- all focused suites pass

- [x] **Step 2: Run the full suite**

Run:

```bash
cd <repo-root>
cargo test --target-dir /tmp/sidekick-target-session-parallelism
```

Expected:
- full suite passes

- [x] **Step 3: Sanity-check the key behaviors manually if needed**

Optional manual smoke:

```bash
cd <repo-root>
cargo run --release -- gateway
```

Check:
- different sessions can progress without global serialization
- same session remains ordered
- outbound replies across different destinations are not globally blocked

- [x] **Step 4: Final commit if verification required follow-up changes**

If verification required fixes:

```bash
git add <repo-root>/src/config/mod.rs \
        <repo-root>/src/agent/mod.rs \
        <repo-root>/src/channels/mod.rs \
        <repo-root>/tests/agent.rs \
        <repo-root>/tests/channels.rs \
        <repo-root>/tests/providers.rs \
        <repo-root>/tests/model_profiles.rs
git commit -m "fix: tighten session parallelism behavior"
```

Otherwise, do not create an extra commit.
