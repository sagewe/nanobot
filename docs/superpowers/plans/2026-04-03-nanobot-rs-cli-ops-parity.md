# nanobot-rs CLI and Operations Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring Rust CLI and operator ergonomics up to the Python CLI baseline for setup, status visibility, channel/provider auth workflows, and agent `/restart` command parity before widening the feature surface.

**Architecture:** Keep `src/cli/mod.rs` as the command router only. Move read-only reporting into a small `src/cli/status.rs` helper, onboarding into `src/cli/onboard.rs`, and auth flows into `src/cli/auth.rs` so each surface stays focused and testable. Reuse the existing control-plane, provider, and Weixin runtime primitives instead of cloning Python’s richer UI behavior. The Rust plan is intentionally narrower than the Python CLI: onboarding stays line-oriented, `channels login` is only implemented where a real runtime flow exists, and `provider login` for Codex-class flows is a file-backed verification/handoff path rather than a browser OAuth clone.

**Tech Stack:** Rust, Clap, Tokio, existing `Config` / `ControlStore` / `WeixinLoginManager` / `CodexProvider`, integration tests in `tests/cli.rs`, `tests/cli_web.rs`, `tests/weixin.rs`, `tests/providers.rs`, and `tests/agent.rs`

---

### Task 1: Add a top-level `status` command for operator visibility

**Files:** Create `/Users/sage/nanobot/nanobot-rs/src/cli/status.rs`; modify `/Users/sage/nanobot/nanobot-rs/src/cli/mod.rs`; modify `/Users/sage/nanobot/nanobot-rs/tests/cli.rs`; modify `/Users/sage/nanobot/nanobot-rs/tests/cli_web.rs`

- [ ] **Step 1: Write the failing status tests**

Add integration tests that run `nanobot-rs status` against a temporary root and assert the output includes the current config path, workspace path, default profile, and control-plane bootstrap state. Add a help test that proves `status` appears in the CLI surface.

Use assertions shaped like:

```rust
assert!(stdout.contains("Config:"));
assert!(stdout.contains("Workspace:"));
assert!(stdout.contains("Default profile:"));
assert!(stdout.contains("Control plane:"));
```

- [ ] **Step 2: Run the targeted tests and confirm they fail**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-cli-status --test cli --test cli_web
```

Expected: compile or assertion failures because `status` does not exist yet, and the help output still lacks the new operator summary command.

- [ ] **Step 3: Implement the minimal status renderer and command wiring**

Add a small read-only status module that renders config file presence, workspace presence, default profile name, bootstrapped control-plane state when the root has been initialized, and user count plus per-user runtime state when the control store is available.

Keep this first pass generic. Do not add Weixin or Codex auth reporting yet; those belong to later auth tasks and should reuse the same renderer instead of duplicating string formatting.

- [ ] **Step 4: Re-run the status tests**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-cli-status --test cli --test cli_web
```

Expected: the new `status` command tests pass, and the help output includes the new top-level command.

- [ ] **Step 5: Commit**

```bash
git add /Users/sage/nanobot/nanobot-rs/src/cli/mod.rs \
        /Users/sage/nanobot/nanobot-rs/src/cli/status.rs \
        /Users/sage/nanobot/nanobot-rs/tests/cli.rs \
        /Users/sage/nanobot/nanobot-rs/tests/cli_web.rs
git commit -m "feat: add nanobot-rs status command"
```

### Task 2: Add a Rust onboarding wizard that stays narrower than the Python version

**Files:** Create `/Users/sage/nanobot/nanobot-rs/src/cli/onboard.rs`; modify `/Users/sage/nanobot/nanobot-rs/src/cli/mod.rs`; modify `/Users/sage/nanobot/nanobot-rs/tests/cli.rs`

- [ ] **Step 1: Write the failing wizard tests**

Add an integration test that pipes scripted answers into `nanobot-rs onboard --wizard` and verifies it creates the workspace, bootstraps the first admin, and persists the config. Add a second test that proves the existing non-wizard flow still behaves the same when `--wizard` is absent.

Keep the test fixture scripted so it can run in CI without a human TTY.

- [ ] **Step 2: Run the onboarding tests and confirm they fail**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-cli-onboard --test cli
```

Expected: compile or assertion failures because the wizard path is not implemented yet, and the current onboarding command still requires the old direct flags.

- [ ] **Step 3: Implement the minimal line-oriented wizard**

Add a small prompt flow that covers only the shared Rust setup primitives: workspace path, first admin username and password, optional display name, default profile selection, and brief provider/channel readiness prompts that seed the config file rather than a full deep editor.

Explicitly do not port Python’s full questionary/autocomplete experience, plugin injection, or deep Pydantic-style field editing in this batch. Those are useful follow-ups, but they are not required for CLI/operations parity.

Factor the pure config assembly and summary rendering so the wizard can be tested without depending on a real terminal.

- [ ] **Step 4: Re-run the onboarding tests**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-cli-onboard --test cli
```

Expected: the wizard test passes with scripted stdin, and the existing non-wizard onboarding path still passes.

- [ ] **Step 5: Commit**

```bash
git add /Users/sage/nanobot/nanobot-rs/src/cli/mod.rs \
        /Users/sage/nanobot/nanobot-rs/src/cli/onboard.rs \
        /Users/sage/nanobot/nanobot-rs/tests/cli.rs
git commit -m "feat: add nanobot-rs onboarding wizard"
```

### Task 3: Add `channels status` plus Weixin login parity

**Files:** Create `/Users/sage/nanobot/nanobot-rs/src/cli/auth.rs`; modify `/Users/sage/nanobot/nanobot-rs/src/cli/mod.rs`; modify `/Users/sage/nanobot/nanobot-rs/src/cli/status.rs`; modify `/Users/sage/nanobot/nanobot-rs/src/channels/weixin.rs`; modify `/Users/sage/nanobot/nanobot-rs/tests/cli.rs`; modify `/Users/sage/nanobot/nanobot-rs/tests/weixin.rs`

- [ ] **Step 1: Write the failing channel auth tests**

Add tests that prove `channels status` prints the built-in Rust channel set and marks Weixin as enabled/disabled plus logged in or expired when account state exists, and `channels login weixin` starts the QR flow, polls status, and persists a confirmed account into the Weixin account store. Use the existing Weixin mock-server patterns so the test does not hit the real backend.

- [ ] **Step 2: Run the targeted tests and confirm they fail**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-weixin-auth --test cli --test weixin
```

Expected: compile or assertion failures because the `channels` subcommand and Weixin CLI flow do not exist yet.

- [ ] **Step 3: Implement the minimal Weixin command path**

Add `channels status` and `channels login weixin` wiring in the CLI, backed by the existing `WeixinLoginManager` and `WeixinAccountStore`.

Keep the login flow text-first: start the QR session, show the QR payload or data URL in the terminal, poll until confirmed or expired, and persist the confirmed account state.

If a richer terminal QR renderer is not available, do not block on it. The goal here is operational usability, not a perfect browser-style login UI.

Extend the shared status renderer from Task 1 so Weixin login state is visible in the operator summary once the account store contains data.

- [ ] **Step 4: Re-run the Weixin tests**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-weixin-auth --test cli --test weixin
```

Expected: `channels status` reflects the current Weixin state, and the mocked QR login flow persists a confirmed account.

- [ ] **Step 5: Commit**

```bash
git add /Users/sage/nanobot/nanobot-rs/src/cli/auth.rs \
        /Users/sage/nanobot/nanobot-rs/src/cli/mod.rs \
        /Users/sage/nanobot/nanobot-rs/src/cli/status.rs \
        /Users/sage/nanobot/nanobot-rs/src/channels/weixin.rs \
        /Users/sage/nanobot/nanobot-rs/tests/cli.rs \
        /Users/sage/nanobot/nanobot-rs/tests/weixin.rs
git commit -m "feat: add nanobot-rs weixin channel auth workflow"
```

### Task 4: Add Codex provider login/status parity without expanding provider scope

**Files:** modify `/Users/sage/nanobot/nanobot-rs/src/cli/auth.rs`; modify `/Users/sage/nanobot/nanobot-rs/src/cli/mod.rs`; modify `/Users/sage/nanobot/nanobot-rs/src/cli/status.rs`; modify `/Users/sage/nanobot/nanobot-rs/src/providers/codex.rs`; modify `/Users/sage/nanobot/nanobot-rs/tests/cli.rs`; modify `/Users/sage/nanobot/nanobot-rs/tests/providers.rs`

- [ ] **Step 1: Write the failing Codex auth tests**

Add tests that prove `provider login codex` validates the configured auth file, prints the account id, and reports success when the file is valid; that the command fails with a clear error when the auth file is missing or malformed; and that the top-level status summary includes a Codex readiness line once the helper exists. Keep this flow file-backed. Do not introduce a browser OAuth login flow or a new generic provider-auth surface in Rust.

- [ ] **Step 2: Run the targeted tests and confirm they fail**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-codex-auth --test cli --test providers
```

Expected: compile or assertion failures because the Codex auth summary/login command is not wired up yet.

- [ ] **Step 3: Implement the Codex auth summary and login helper**

Extend `CodexProvider` with a small public summary helper that can report the resolved auth-file path, validated account id, and whether the auth file parses successfully. Wire `provider login codex` to that helper so the command becomes a practical verification/handoff step for operators. Reuse the same summary in the top-level status output so status and login stay consistent. If Rust still cannot do something that Python’s CLI can, say so explicitly in the command output instead of pretending the path is ready.

- [ ] **Step 4: Re-run the Codex tests**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-codex-auth --test cli --test providers
```

Expected: `provider login codex` reports valid auth state for a good fixture, missing auth files fail with a clear and actionable message, and the status summary shows the same Codex readiness data.

- [ ] **Step 5: Commit**

```bash
git add /Users/sage/nanobot/nanobot-rs/src/cli/auth.rs \
        /Users/sage/nanobot/nanobot-rs/src/cli/mod.rs \
        /Users/sage/nanobot/nanobot-rs/src/cli/status.rs \
        /Users/sage/nanobot/nanobot-rs/src/providers/codex.rs \
        /Users/sage/nanobot/nanobot-rs/tests/cli.rs \
        /Users/sage/nanobot/nanobot-rs/tests/providers.rs
git commit -m "feat: add nanobot-rs codex provider auth workflow"
```

### Task 5: Add agent `/restart` command parity

**Files:** modify `/Users/sage/nanobot/nanobot-rs/src/agent/mod.rs`; modify `/Users/sage/nanobot/nanobot-rs/tests/agent.rs`

- [ ] **Step 1: Write the failing restart test**

Add a test that sends `/restart` through the agent path and asserts the user gets a `Restarting...` response, the command is treated as an ephemeral operator command rather than a normal model turn, and a restart hook is invoked once. Use a fake restart hook so the test never actually relaunches the test process.

- [ ] **Step 2: Run the targeted test and confirm it fails**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-agent-restart --test agent restart_command_publishes_restart_notice_and_invokes_restart_hook -- --exact
```

Expected: failure because `/restart` is not yet handled in the Rust agent loop.

- [ ] **Step 3: Implement the restart hook and command dispatch**

Add a small restart abstraction to `AgentLoop` so tests can substitute a no-op while production code keeps the current process-relaunch behavior.

Make `/restart` match the Python user experience as closely as Rust allows: surface the restart notice immediately, avoid persisting it as a normal conversational turn, relaunch after a short delay through the default hook, and include `/restart` in help text and any ephemeral-command handling.

- [ ] **Step 4: Re-run the agent tests**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-agent-restart --test agent
```

Expected: the new restart test passes, and the existing agent suite stays green.

- [ ] **Step 5: Commit**

```bash
git add /Users/sage/nanobot/nanobot-rs/src/agent/mod.rs \
        /Users/sage/nanobot/nanobot-rs/tests/agent.rs
git commit -m "feat: add nanobot-rs restart command"
```

### Task 6: Run the CLI and operations verification sweep

**Files:** Verify `/Users/sage/nanobot/nanobot-rs/src/cli/mod.rs`; verify `/Users/sage/nanobot/nanobot-rs/src/cli/status.rs`; verify `/Users/sage/nanobot/nanobot-rs/src/cli/onboard.rs`; verify `/Users/sage/nanobot/nanobot-rs/src/cli/auth.rs`; verify `/Users/sage/nanobot/nanobot-rs/src/providers/codex.rs`; verify `/Users/sage/nanobot/nanobot-rs/src/agent/mod.rs`; verify `/Users/sage/nanobot/nanobot-rs/tests/cli.rs`; verify `/Users/sage/nanobot/nanobot-rs/tests/cli_web.rs`; verify `/Users/sage/nanobot/nanobot-rs/tests/weixin.rs`; verify `/Users/sage/nanobot/nanobot-rs/tests/providers.rs`; verify `/Users/sage/nanobot/nanobot-rs/tests/agent.rs`

- [ ] **Step 1: Run the touched CLI and auth suites together**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test --target-dir /tmp/nanobot-rs-target-cli-ops --test cli --test cli_web --test weixin --test providers --test agent
```

Expected:
- the new `status`, wizard, Weixin auth, Codex auth, and `/restart` cases pass together
- there are no regressions in the existing multi-user CLI surface

- [ ] **Step 2: Run formatting and the full Rust suite**

Run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo fmt --all --check
cargo test --target-dir /tmp/nanobot-rs-target-cli-ops
```

Expected:
- formatting check passes
- the full Rust suite passes without new regressions in control, web, channels, or providers

- [ ] **Step 3: Optional manual smoke check**

If a manual pass is useful, run:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo run -- onboard --wizard
cargo run -- status
```

Check:
- the wizard path completes without needing undocumented flags
- `status` reports config/workspace/default profile and control-plane state
- Weixin and Codex readiness lines are visible when configured

- [ ] **Step 4: Finalize only if verification found follow-up fixes**

If verification exposed follow-up changes, stage and commit them in the same area that introduced the regression:

```bash
git add /Users/sage/nanobot/nanobot-rs/src/cli/mod.rs \
        /Users/sage/nanobot/nanobot-rs/src/cli/status.rs \
        /Users/sage/nanobot/nanobot-rs/src/cli/onboard.rs \
        /Users/sage/nanobot/nanobot-rs/src/cli/auth.rs \
        /Users/sage/nanobot/nanobot-rs/src/providers/codex.rs \
        /Users/sage/nanobot/nanobot-rs/src/agent/mod.rs \
        /Users/sage/nanobot/nanobot-rs/tests/cli.rs \
        /Users/sage/nanobot/nanobot-rs/tests/cli_web.rs \
        /Users/sage/nanobot/nanobot-rs/tests/weixin.rs \
        /Users/sage/nanobot/nanobot-rs/tests/providers.rs \
        /Users/sage/nanobot/nanobot-rs/tests/agent.rs
git commit -m "feat: complete nanobot-rs cli ops parity"
```

Otherwise, stop after the verification pass and do not create an extra commit.
