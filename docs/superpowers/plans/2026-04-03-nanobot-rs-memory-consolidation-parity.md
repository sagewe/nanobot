# nanobot-rs Memory and Consolidation Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring `nanobot-rs` closer to the Python memory model by adding durable `MEMORY.md` and `HISTORY.md` storage, a `save_memory`-driven consolidation flow, token-budget-based compaction, and session-boundary handling that survives persistence and restarts.

**Architecture:** Keep the session log as the source of truth in `src/session/mod.rs`, and keep consolidation orchestration in a dedicated agent-side memory module instead of spreading it across the CLI or control plane. The new memory layer should own the `save_memory` prompt/tool contract, append-only history writes, and the compaction watermark, while `AgentLoop` remains responsible for deciding when to invoke it and for persisting the updated session state. Do not mirror Python literally: Rust should use the existing session/session-store layout, a deterministic token approximation, and a narrow internal consolidation tool definition rather than a global runtime tool.

**Tech Stack:** Rust 2024, Tokio, serde/serde_json, chrono, the existing `AgentLoop`/`SessionStore`/`ToolRegistry` architecture, and the integration tests in `tests/agent.rs`, `tests/session.rs`, `tests/cli.rs`, and `tests/control.rs`.

---

### Task 1: Add storage primitives and workspace templates for long-term memory

**Files:**
- Create: `/Users/sage/nanobot/nanobot-rs/src/agent/memory.rs`
- Modify: `/Users/sage/nanobot/nanobot-rs/src/agent/mod.rs`
- Modify: `/Users/sage/nanobot/nanobot-rs/src/session/mod.rs`
- Modify: `/Users/sage/nanobot/nanobot-rs/src/control/mod.rs`
- Modify: `/Users/sage/nanobot/nanobot-rs/src/cli/mod.rs`
- Test: `/Users/sage/nanobot/nanobot-rs/tests/session.rs`
- Test: `/Users/sage/nanobot/nanobot-rs/tests/control.rs`
- Test: `/Users/sage/nanobot/nanobot-rs/tests/cli.rs`

- [ ] **Step 1: Write the failing storage-template tests**

Add tests that assert:
- bootstrap paths create both `memory/MEMORY.md` and `memory/HISTORY.md`
- existing memory files are not overwritten when the workspace already contains custom content
- `SessionStore` round-trips `last_consolidated`
- `Session::clear()` resets the consolidation watermark back to zero

Suggested test additions:

```rust
assert!(workspace.join("memory").join("MEMORY.md").exists());
assert!(workspace.join("memory").join("HISTORY.md").exists());
assert_eq!(loaded.last_consolidated, 4);
assert_eq!(cleared.last_consolidated, 0);
```

- [ ] **Step 2: Run the targeted tests to verify the new assertions fail**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-memory-storage --test session --test control --test cli
```

Expected:
- the new `HISTORY.md` assertions fail because only `MEMORY.md` is seeded today
- the `last_consolidated`/`clear()` assertions may fail if the session helpers are not yet explicit enough for the new memory flow

- [ ] **Step 3: Implement the storage primitives**

In `/Users/sage/nanobot/nanobot-rs/src/agent/memory.rs`:
- add a `MemoryStore` that creates `memory/`, reads `MEMORY.md`, writes `MEMORY.md`, appends to `HISTORY.md`, and creates both files lazily if they do not exist
- keep `HISTORY.md` append-only and formatted for grep search, not JSONL
- include a small raw-archive helper for the fallback path used when consolidation fails repeatedly

In `/Users/sage/nanobot/nanobot-rs/src/session/mod.rs`:
- keep `last_consolidated` as the persisted compaction watermark
- add small helper methods that make the unconsolidated tail and safe reset behavior explicit, so the consolidator does not have to reach into private slicing logic
- keep `get_history()` as the read path for LLM prompts; do not change its semantics unless the helper extraction forces it

In `/Users/sage/nanobot/nanobot-rs/src/control/mod.rs` and `/Users/sage/nanobot/nanobot-rs/src/cli/mod.rs`:
- extend workspace bootstrapping so the default templates include `memory/HISTORY.md`
- preserve existing user content when those files already exist
- keep the workspace template text short and intentionally generic

In `/Users/sage/nanobot/nanobot-rs/src/agent/mod.rs`:
- add the new memory module declaration so the consolidator can be wired in later

- [ ] **Step 4: Re-run the focused tests**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-memory-storage --test session --test control --test cli
```

Expected:
- bootstrap tests pass with both memory files present
- session watermark tests pass

- [ ] **Step 5: Commit**

```bash
git add /Users/sage/nanobot/nanobot-rs/src/agent/memory.rs \
        /Users/sage/nanobot/nanobot-rs/src/agent/mod.rs \
        /Users/sage/nanobot/nanobot-rs/src/session/mod.rs \
        /Users/sage/nanobot/nanobot-rs/src/control/mod.rs \
        /Users/sage/nanobot/nanobot-rs/src/cli/mod.rs \
        /Users/sage/nanobot/nanobot-rs/tests/session.rs \
        /Users/sage/nanobot/nanobot-rs/tests/control.rs \
        /Users/sage/nanobot/nanobot-rs/tests/cli.rs
git commit -m "feat: add memory storage primitives"
```

### Task 2: Add save_memory-driven consolidation orchestration in the agent layer

**Files:**
- Modify: `/Users/sage/nanobot/nanobot-rs/src/agent/memory.rs`
- Modify: `/Users/sage/nanobot/nanobot-rs/src/agent/mod.rs`
- Modify: `/Users/sage/nanobot/nanobot-rs/src/session/mod.rs`
- Test: `/Users/sage/nanobot/nanobot-rs/tests/agent.rs`
- Test: `/Users/sage/nanobot/nanobot-rs/tests/session.rs`

- [ ] **Step 1: Write the failing consolidation-orchestration tests**

Add tests that assert:
- a consolidation run appends a formatted entry to `HISTORY.md`
- the same run rewrites `MEMORY.md` only when the model returns a new memory document
- `last_consolidated` advances only after both file writes succeed
- the consolidator skips already-consolidated messages on a restart or duplicate session

Use a provider double that captures the messages it receives and returns a single `save_memory` tool call with `history_entry` and `memory_update`.

Example expectations:

```rust
assert!(history.contains("2026-04-03"));
assert!(memory.contains("important fact"));
assert_eq!(session.last_consolidated, expected_boundary);
```

- [ ] **Step 2: Run the targeted tests to verify the new flow fails**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-memory-consolidation --test agent --test session
```

Expected:
- the new `save_memory`-driven tests fail because the consolidator does not exist yet
- the restart/duplicate-session case fails until the compaction watermark is used as the source of truth

- [ ] **Step 3: Implement the consolidation orchestrator**

In `/Users/sage/nanobot/nanobot-rs/src/agent/memory.rs`:
- add a `MemoryConsolidator` that owns the memory store, the provider/model handle, a per-session lock table, and the compaction policy
- define a narrow internal `save_memory` tool schema with only `history_entry` and `memory_update`
- ask the model for a `save_memory` call using a dedicated consolidation prompt that includes the current `MEMORY.md` and the selected conversation slice
- validate the response strictly: if the provider does not return a `save_memory` tool call, treat the run as failed and fall back to raw archiving after the retry threshold
- append to `HISTORY.md` first, then rewrite `MEMORY.md` if the model produced an updated document, and only then advance `last_consolidated`
- keep a small failure counter so repeated consolidation failures eventually raw-archive the chunk instead of dropping it

In `/Users/sage/nanobot/nanobot-rs/src/agent/mod.rs`:
- instantiate the consolidator from `AgentLoop`
- expose a single entry point that the loop can call after turn completion or before a prompt build
- keep the orchestration in the agent layer; do not move this into `SessionStore`

In `/Users/sage/nanobot/nanobot-rs/src/session/mod.rs`:
- expose the minimum boundary helpers the consolidator needs to compute a safe consolidation slice without reimplementing the session read logic

Design note:
- Rust's provider abstraction does not expose Python's forced `tool_choice`, so the plan should emulate that behavior by narrowing the tool definition list and validating the returned tool call explicitly instead of depending on provider-side enforcement.

- [ ] **Step 4: Re-run the focused tests**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-memory-consolidation --test agent --test session
```

Expected:
- consolidation writes both memory files
- the watermark moves only after successful persistence
- duplicate/restarted sessions do not re-consolidate old history

- [ ] **Step 5: Commit**

```bash
git add /Users/sage/nanobot/nanobot-rs/src/agent/memory.rs \
        /Users/sage/nanobot/nanobot-rs/src/agent/mod.rs \
        /Users/sage/nanobot/nanobot-rs/src/session/mod.rs \
        /Users/sage/nanobot/nanobot-rs/tests/agent.rs \
        /Users/sage/nanobot/nanobot-rs/tests/session.rs
git commit -m "feat: add save_memory consolidation orchestration"
```

### Task 3: Add token-budget trigger policy and safe consolidation boundaries

**Files:**
- Modify: `/Users/sage/nanobot/nanobot-rs/src/agent/memory.rs`
- Modify: `/Users/sage/nanobot/nanobot-rs/src/session/mod.rs`
- Test: `/Users/sage/nanobot/nanobot-rs/tests/agent.rs`
- Test: `/Users/sage/nanobot/nanobot-rs/tests/session.rs`

- [ ] **Step 1: Write the failing policy tests**

Add tests that assert:
- a session below the configured token budget does not consolidate
- a session that crosses the budget consolidates only up to a safe user-turn boundary
- the consolidator never cuts through an assistant/tool pair
- `_exclude_from_context` messages stay out of the consolidation slice

Because Rust does not yet have a tokenizer dependency here, use a deterministic approximation in the test data so the trigger condition is stable.

Example expectations:

```rust
assert!(triggered);
assert_eq!(boundary % 2, 0);
assert!(!history.iter().any(|m| m.excluded_from_context()));
```

- [ ] **Step 2: Run the targeted tests to verify the policy fails**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-memory-policy --test session --test agent
```

Expected:
- the token-budget test fails because the policy is not wired yet
- the safe-boundary test fails until the consolidator reuses the session boundary helpers rather than slicing blindly

- [ ] **Step 3: Implement the policy layer**

In `/Users/sage/nanobot/nanobot-rs/src/agent/memory.rs`:
- add a deterministic token estimator suitable for Rust, such as a weighted word/punctuation approximation
- keep it isolated behind a helper so a later pass can swap in a model-specific tokenizer without rewriting the consolidation flow
- compute the compaction target from `context_window_tokens`, then select the oldest safe chunk that removes enough estimated tokens to move the session back under budget
- stop after a bounded number of consolidation rounds if the session still cannot be compacted safely

In `/Users/sage/nanobot/nanobot-rs/src/session/mod.rs`:
- extract or expose the legal-start logic so both `get_history()` and the consolidator can agree on what counts as a valid turn boundary
- make sure timeline-only or otherwise excluded messages never become consolidation anchors

Policy intent:
- use token pressure to decide when to consolidate
- use session-turn boundaries to decide where to cut
- never trade correctness for a slightly smaller prompt

- [ ] **Step 4: Re-run the focused tests**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-memory-policy --test session --test agent
```

Expected:
- budget-based consolidation triggers deterministically
- the selected chunk ends at a valid session boundary
- excluded and timeline-only messages remain out of the compaction slice

- [ ] **Step 5: Commit**

```bash
git add /Users/sage/nanobot/nanobot-rs/src/agent/memory.rs \
        /Users/sage/nanobot/nanobot-rs/src/session/mod.rs \
        /Users/sage/nanobot/nanobot-rs/tests/agent.rs \
        /Users/sage/nanobot/nanobot-rs/tests/session.rs
git commit -m "feat: add token-based memory consolidation policy"
```

### Task 4: Wire consolidation into AgentLoop and session persistence boundaries

**Files:**
- Modify: `/Users/sage/nanobot/nanobot-rs/src/agent/mod.rs`
- Modify: `/Users/sage/nanobot/nanobot-rs/src/session/mod.rs`
- Test: `/Users/sage/nanobot/nanobot-rs/tests/agent.rs`
- Test: `/Users/sage/nanobot/nanobot-rs/tests/session.rs`

- [ ] **Step 1: Write the failing integration tests**

Add tests that assert:
- `AgentLoop` triggers consolidation after a turn is saved when the session is over budget
- `/new` flushes or archives the current unconsolidated tail before resetting the session
- a restarted session resumes consolidation from the persisted `last_consolidated` watermark
- duplicating a session preserves the watermark so the copy does not reprocess old history

Suggested assertions:

```rust
assert_eq!(session.last_consolidated, previous_watermark);
assert!(history_file_contents.contains("[RAW]"));
assert!(session.messages.is_empty());
```

- [ ] **Step 2: Run the targeted tests to verify the integration fails**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-memory-integration --test agent --test session
```

Expected:
- the post-turn compaction case fails until `AgentLoop` invokes the consolidator at the right point
- the `/new` boundary case fails until the tail is flushed before the session reset

- [ ] **Step 3: Implement the session-boundary wiring**

In `/Users/sage/nanobot/nanobot-rs/src/agent/mod.rs`:
- call the consolidator after `SessionStore.save()` for a completed turn, not before the session write
- schedule the compaction work in the existing background-task path so the main loop stays responsive
- keep a per-session lock around consolidation work so the background compactor and the next inbound turn cannot race on the same session
- on `/new`, archive any remaining unconsolidated messages, reset the session, persist the blank session, and then clear the in-memory cache entry
- keep the session save as the persistence synchronization point: only save `last_consolidated` after the memory files have been updated successfully

In `/Users/sage/nanobot/nanobot-rs/src/session/mod.rs`:
- preserve `last_consolidated` on load/save/duplicate flows
- keep `clear()` resetting the watermark to zero because a brand-new session must not inherit the previous compaction state

Boundary intent:
- the session log remains append-only
- `MEMORY.md` and `HISTORY.md` persist across session resets
- `/new` starts a fresh session, not a fresh memory store

- [ ] **Step 4: Re-run the targeted tests**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-memory-integration --test agent --test session
```

Expected:
- compaction runs after turn persistence
- `/new` preserves long-term memory while resetting the transient session log
- duplicate/reloaded sessions retain the correct watermark

- [ ] **Step 5: Commit**

```bash
git add /Users/sage/nanobot/nanobot-rs/src/agent/mod.rs \
        /Users/sage/nanobot/nanobot-rs/src/session/mod.rs \
        /Users/sage/nanobot/nanobot-rs/tests/agent.rs \
        /Users/sage/nanobot/nanobot-rs/tests/session.rs
git commit -m "feat: wire memory consolidation into session persistence"
```

### Task 5: Full verification and regression sweep

**Files:**
- Verify: `/Users/sage/nanobot/nanobot-rs/src/agent/memory.rs`
- Verify: `/Users/sage/nanobot/nanobot-rs/src/agent/mod.rs`
- Verify: `/Users/sage/nanobot/nanobot-rs/src/session/mod.rs`
- Verify: `/Users/sage/nanobot/nanobot-rs/src/control/mod.rs`
- Verify: `/Users/sage/nanobot/nanobot-rs/src/cli/mod.rs`
- Verify: `/Users/sage/nanobot/nanobot-rs/tests/agent.rs`
- Verify: `/Users/sage/nanobot/nanobot-rs/tests/session.rs`
- Verify: `/Users/sage/nanobot/nanobot-rs/tests/control.rs`
- Verify: `/Users/sage/nanobot/nanobot-rs/tests/cli.rs`

- [ ] **Step 1: Run the focused suites together**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-memory-parity --test agent --test session --test control --test cli
```

Expected:
- all targeted suites pass
- the memory bootstrap, compaction, boundary handling, and `/new` cases stay green together

- [ ] **Step 2: Run formatting and the full test suite**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo fmt --all --check
cargo test --target-dir /tmp/nanobot-rs-target-memory-parity
```

Expected:
- formatting check passes
- the full Rust suite passes without regressions in control, channels, providers, or agent behavior

- [ ] **Step 3: Optional manual smoke test**

If a manual check is useful, run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo run -- gateway
```

Check:
- `memory/MEMORY.md` and `memory/HISTORY.md` exist in a fresh workspace
- long-lived memory survives `/new`
- the session watermark resumes correctly after a restart

- [ ] **Step 4: Final commit only if this phase produced fixes**

If verification found follow-up changes, stage and commit the whole parity set:

```bash
git add /Users/sage/nanobot/nanobot-rs/src/agent/memory.rs \
        /Users/sage/nanobot/nanobot-rs/src/agent/mod.rs \
        /Users/sage/nanobot/nanobot-rs/src/session/mod.rs \
        /Users/sage/nanobot/nanobot-rs/src/control/mod.rs \
        /Users/sage/nanobot/nanobot-rs/src/cli/mod.rs \
        /Users/sage/nanobot/nanobot-rs/tests/agent.rs \
        /Users/sage/nanobot/nanobot-rs/tests/session.rs \
        /Users/sage/nanobot/nanobot-rs/tests/control.rs \
        /Users/sage/nanobot/nanobot-rs/tests/cli.rs
git commit -m "feat: complete memory and consolidation parity"
```

Otherwise, stop after the verification pass and do not create an extra commit.
