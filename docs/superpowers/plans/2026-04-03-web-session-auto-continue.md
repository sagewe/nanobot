# Web Session Auto-Continue Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the browser continue non-`web` sessions by auto-duplicating into a writable `web` copy on first send, while also bringing README and smoke docs up to date with the current runtime.

**Architecture:** Keep session ownership unchanged: non-`web` sessions stay read-only and `web` sessions remain the only writable browser sessions. Implement the new behavior by teaching `POST /api/chat` to auto-duplicate non-`web` sessions into `web`, then update the frontend composer gating so duplicable sessions can submit without a manual duplicate. Finish by syncing README and runbook documentation and rerunning full verification.

**Tech Stack:** Rust, Axum, existing session store and web APIs, vanilla frontend JavaScript, Vitest, cargo test

---

### Task 1: Lock auto-continue behavior in web API tests

**Files:**
- Modify: `tests/web_server.rs`
- Modify: `src/web/api.rs`

- [ ] **Step 1: Write the failing backend regressions**

Add focused tests proving:

```rust
#[tokio::test]
async fn chat_endpoint_auto_duplicates_non_web_sessions_into_web() {
    // seed a telegram or weixin session, POST /api/chat against it,
    // assert the response returns channel=web, source history is preserved,
    // and the original external session remains unchanged.
}

#[tokio::test]
async fn chat_endpoint_without_session_id_still_rejects_non_web_sends() {
    // non-web sends still need a concrete source session to continue from
}
```

- [ ] **Step 2: Run the targeted backend tests to verify RED**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target-web-auto --test web_server chat_endpoint_auto_duplicates_non_web_sessions_into_web -- --exact
cargo test --target-dir /tmp/sidekick-target-web-auto --test web_server chat_endpoint_without_session_id_still_rejects_non_web_sends -- --exact
```

Expected: FAIL because `src/web/api.rs` still rejects every non-`web` chat request.

- [ ] **Step 3: Implement minimal backend support**

In `src/web/api.rs`:

- keep empty-message validation unchanged
- keep `channel == "web"` behavior unchanged
- when `channel != "web"` and a valid `sessionId` is provided:
  - resolve the existing chat service
  - duplicate the external session into a new `web` session
  - chat against the returned `web` session id
  - return the `web` session identity in the normal `ChatResponse`
- keep non-`web` requests without `sessionId` as `400`

Do not add a new endpoint or mutate the original external session.

- [ ] **Step 4: Re-run the targeted backend tests**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target-web-auto --test web_server chat_endpoint_auto_duplicates_non_web_sessions_into_web -- --exact
cargo test --target-dir /tmp/sidekick-target-web-auto --test web_server chat_endpoint_without_session_id_still_rejects_non_web_sends -- --exact
```

Expected: PASS.

### Task 2: Update frontend composer gating and shell regressions

**Files:**
- Modify: `tests/web_page.rs`
- Modify: `frontend/src/main.js`
- Modify: `frontend/src/i18n.js`

- [ ] **Step 1: Write the failing browser-shell regressions**

Add assertions that lock the new submit rules:

```rust
#[test]
fn page_shell_allows_submit_for_duplicable_read_only_sessions() {
    let html = sidekick::web::page::render_index_html();
    assert!(html.contains("messageInput.disabled = readOnly && !canDuplicate;"));
    assert!(html.contains("if (currentSessionReadOnly && !currentSessionCanDuplicate) {"));
}
```

If needed, add a second assertion proving the page still switches to the returned session after `sendChat(...)`.

- [ ] **Step 2: Run the targeted shell test to verify RED**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target-web-auto --test web_page page_shell_allows_submit_for_duplicable_read_only_sessions -- --exact
```

Expected: FAIL because the current shell still disables and rejects all read-only sessions.

- [ ] **Step 3: Implement minimal frontend changes**

In `frontend/src/main.js`:

- change composer gating so duplicable sessions stay editable
- change submit-time blocking so only non-sendable and non-duplicable sessions are rejected
- after `sendChat(...)`, keep the existing refresh-and-select flow so the returned `web` session becomes the active one
- optionally show a lightweight status note when the send switched channels

In `frontend/src/i18n.js`:

- add a short status string for auto-continuing into Web if the implementation needs one

Do not remove the explicit `Duplicate to Web` button.

- [ ] **Step 4: Re-run the targeted shell test**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target-web-auto --test web_page page_shell_allows_submit_for_duplicable_read_only_sessions -- --exact
```

Expected: PASS.

### Task 3: Sync README and smoke checklist with current behavior

**Files:**
- Modify: `README.md`
- Modify: `docs/runbooks/sidekick-runtime-smoke-checklist.md`

- [ ] **Step 1: Update README to reflect current runtime and browser continuation**

Document:

- `sidekick status`
- `sidekick onboard --wizard`
- `channels status`
- `provider login codex`
- `Skills` management in the web UI
- durable `memory/MEMORY.md` and `memory/HISTORY.md`
- browser sends from non-`web` sessions automatically continue in a new `web` session

- [ ] **Step 2: Update the smoke checklist**

Adjust the browser smoke instructions so they match the new continuation flow and the current implemented capabilities.

- [ ] **Step 3: Review docs for contradictions**

Search for stale phrases like:

```bash
rg -n "duplicated into a writable `web` session|read-only in the browser until duplicated" README.md docs/runbooks
```

Expected: only intentionally preserved wording remains.

### Task 4: Run full repository verification

**Files:**
- No source changes required

- [ ] **Step 1: Run Rust tests**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target-final
```

Expected: PASS.

- [ ] **Step 2: Run frontend tests**

Run:

```bash
cd frontend && npm test -- --run
```

Expected: PASS.

- [ ] **Step 3: Run frontend production build**

Run:

```bash
cd frontend && npm run build
```

Expected: PASS.

- [ ] **Step 4: Review git diff for scope**

Run:

```bash
git status --short
git diff -- README.md docs/runbooks/sidekick-runtime-smoke-checklist.md src/web/api.rs frontend/src/main.js frontend/src/i18n.js tests/web_server.rs tests/web_page.rs
```

Expected: only the planned files changed.
