# Sidekick README Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a dedicated [README.md](<repo-root>/README.md) that gives first-time users a fast, accurate guide to the current Rust runtime.

**Architecture:** Keep the work isolated to documentation. Update the repository-root README, grounded in the current runtime behavior and linked to the deeper runbook for smoke testing and triage. Do not change runtime code as part of this task.

**Tech Stack:** Markdown, existing Rust CLI/runtime behavior, existing runbook documentation

---

### Task 1: Confirm current runtime facts before writing

**Files:**
- Read: `<repo-root>/docs/superpowers/specs/2026-03-24-sidekick-readme-design.md`
- Read: `<repo-root>/docs/runbooks/sidekick-runtime-smoke-checklist.md`
- Read: `<repo-root>/src/cli/mod.rs`
- Read: `<repo-root>/src/config/mod.rs`
- Create: `<repo-root>/README.md`

- [x] **Step 1: Write the failing verification checklist in the plan notes**

Record the facts the README must match exactly:

```text
- onboard command exists
- gateway starts embedded web
- default embedded web port is 3456
- config root is ~/.sidekick
- profiles use agents.defaults.defaultProfile + agents.profiles
- providers include openai, custom, openrouter, ollama, codex
- channels include telegram, wecom, weixin
- Weixin currently handles text messages only
```

- [x] **Step 2: Run quick fact checks before drafting**

Run:

```bash
cd <repo-root>
cargo run --release -- help
rg -n "DEFAULT_WEB_PORT|ONBOARD_TEMPLATE_SUMMARY" src/cli/mod.rs
rg -n "defaultProfile|openai|custom|openrouter|ollama|codex|telegram|wecom|weixin" src/config/mod.rs
```

Expected:
- `help` lists `onboard`, `agent`, `gateway`, `web`
- `DEFAULT_WEB_PORT` is `3456`
- config code clearly uses `defaultProfile`, includes the current provider set, and includes the current channel set

- [x] **Step 3: Draft a short README outline before writing prose**

Use this exact section order:

```markdown
# Sidekick
## Current Status
## Quick Start
## Configuration
## Channels and Web
## Provider Notes
## Current Limitations
## Development
```

- [x] **Step 4: Commit prep checkpoint**

No commit yet. This task is complete when the runtime facts are verified and the outline matches the approved spec.

### Task 2: Write the README with only current, verified behavior

**Files:**
- Create: `<repo-root>/README.md`
- Reference: `<repo-root>/docs/runbooks/sidekick-runtime-smoke-checklist.md`
- Reference: `<repo-root>/README.md`

- [x] **Step 1: Write the initial README draft**

Write the document with the following minimum content:

```markdown
# Sidekick

Rust runtime for the Sidekick project. This README covers the current Rust implementation only; the repository root README still documents the broader Python-first project.

## Current Status
- `agent`
- `gateway` with embedded web
- `telegram`
- `wecom`
- `weixin`
- `openai`, `custom`, `openrouter`, `ollama`, `codex`

## Quick Start
```bash
cargo run --release -- onboard
```

Edit `~/.config.json`, then:

```bash
cargo run --release -- gateway
```

Open `http://127.0.0.1:3456`.
```

Then expand the rest of the sections with concise, operational language.

- [x] **Step 2: Add one minimal configuration example**

Use one compact example only. It should show:
- `agents.defaults.defaultProfile`
- `agents.profiles`
- one provider block
- one optional channel block
- one `tools` subsection

Use this shape:

```json
{
  "agents": {
    "defaults": {
      "defaultProfile": "openai:gpt-4.1-mini"
    },
    "profiles": {
      "openai:gpt-4.1-mini": {
        "provider": "openai",
        "model": "gpt-4.1-mini",
        "request": {}
      }
    }
  },
  "providers": {
    "openai": {
      "apiKey": "sk-...",
      "apiBase": "https://api.openai.com/v1",
      "extraHeaders": {}
    }
  },
  "channels": {
    "telegram": {
      "enabled": false,
      "token": "",
      "allowFrom": [],
      "apiBase": "https://api.telegram.org"
    }
  },
  "tools": {
    "web": {
      "search": {
        "provider": "duckduckgo"
      }
    }
  }
}
```

Do not paste the full default config.

- [x] **Step 3: Make cross-channel session behavior explicit**

Include a short paragraph stating:

```text
gateway starts the embedded web UI, the web UI can browse grouped sessions across channels, and non-web sessions are read-only until duplicated into a writable web session.
```

- [x] **Step 4: Make Codex behavior explicit**

Include a short paragraph stating:

```text
Codex uses ~/.codex/auth.json and does not fall back to OPENAI_API_KEY.
```

- [x] **Step 5: Make current limitations explicit**

Include at least these points:

```text
- the Rust runtime is still evolving
- most channel paths are text-first
- Weixin currently handles text messages only
```

- [x] **Step 6: Commit**

```bash
git add <repo-root>/README.md
git commit -m "docs: add Sidekick readme"
```

### Task 3: Verify the README against the current runtime

**Files:**
- Verify: `<repo-root>/README.md`
- Verify: `<repo-root>/docs/runbooks/sidekick-runtime-smoke-checklist.md`

- [x] **Step 1: Run content checks against the finished README**

Run:

```bash
cd <repo-root>
rg -n "cargo run --release -- onboard|~/.config.json|cargo run --release -- gateway|127.0.0.1:3456|defaultProfile|Duplicate to Web|~/.codex/auth.json|Weixin currently handles text messages only" README.md
```

Expected:
- every required phrase appears exactly once or in an equivalent visible form

- [x] **Step 2: Re-check command references against the binary**

Run:

```bash
cd <repo-root>
cargo run --release -- help
```

Expected:
- the commands referenced in the README still exist

- [x] **Step 3: Verify the quick-start section is discoverable at the top of the file**

Run:

```bash
cd <repo-root>
sed -n '1,80p' README.md
```

Expected:
- the first screenful includes what `Sidekick` is
- `Quick Start` appears near the top
- `cargo run --release -- onboard`
- `~/.config.json`
- `cargo run --release -- gateway`
- `http://127.0.0.1:3456`

- [x] **Step 4: Reconcile README claims against current runtime facts**

Manually compare the README against:

```text
- <repo-root>/docs/runbooks/sidekick-runtime-smoke-checklist.md
- <repo-root>/src/cli/mod.rs
- <repo-root>/src/config/mod.rs
```

Expected:
- no provider/channel/runtime claim in the README contradicts the current code or runbook
- the README explicitly keeps Weixin text-only
- the README explicitly links readers to <repo-root>/docs/runbooks/sidekick-runtime-smoke-checklist.md for smoke testing and triage
- the `Current Status` section matches the runtime surfaces described by the smoke checklist instead of claiming broader support

- [x] **Step 5: Re-run the existing smoke-focused test baseline**

Run:

```bash
cd <repo-root>
cargo test --target-dir /tmp/sidekick-target-readme
```

Expected:
- full test suite passes

- [x] **Step 6: Final commit if verification required README edits**

If verification caused follow-up edits:

```bash
git add <repo-root>/README.md
git commit -m "docs: tighten Sidekick quickstart wording"
```

Otherwise, do not create an extra commit.
