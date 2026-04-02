# Sidekick README Design

Date: 2026-03-24

## Goal

Add a dedicated [README.md](<repo-root>/README.md) that helps a first-time user get the Rust runtime running quickly, without mixing Python-focused documentation from the repository root.

## Audience

Primary audience:
- Users who want to run `Sidekick` quickly

Secondary audience:
- Contributors who need one short section pointing at the main development commands

## Scope

The README should cover:
- What `Sidekick` is
- Current runtime status and supported surfaces
- Quick start with `onboard`, config editing, and `gateway`
- Minimal configuration model: `defaultProfile`, `profiles`, `providers`, `channels`, `tools`
- Embedded web and cross-channel session behavior
- Codex/provider caveats
- Current limitations
- Minimal development commands

The README should not:
- Replace or rewrite the root [README.md](<repo-root>/README.md)
- Promise feature parity with the Python runtime
- Duplicate full config defaults inline
- Include roadmap/spec/plan history

## Proposed Structure

1. `Sidekick`
Short description that this is the Rust runtime for the project.

2. `Current Status`
List current supported runtime paths:
- `agent`
- `gateway` with embedded web
- `telegram`
- `wecom`
- `weixin`
- `openai`, `custom`, `openrouter`, `ollama`, `codex`

3. `Quick Start`
Minimal runnable sequence:
- `cargo run --release -- onboard`
- edit `~/.config.json`
- `cargo run --release -- gateway`
- open `http://127.0.0.1:3456`

4. `Configuration`
Show one compact example with:
- `agents.defaults.defaultProfile`
- `agents.profiles`
- one provider block
- one optional channel block

5. `Channels and Web`
Explain:
- `gateway` starts embedded web
- embedded web can browse grouped sessions across channels
- non-`web` sessions are read-only and must be duplicated to `web` before continuing

6. `Provider Notes`
Explain:
- OpenAI-compatible providers are configured through profiles
- Codex uses `~/.codex/auth.json`
- Codex does not fall back to `OPENAI_API_KEY`

7. `Current Limitations`
State current practical limits, especially:
- runtime is still evolving
- most channel paths are text-first
- Weixin currently handles text messages only

8. `Development`
Keep this short:
- `cargo test`
- `cargo run --release -- gateway`
- link to [docs/runbooks/sidekick-runtime-smoke-checklist.md](<repo-root>/docs/runbooks/sidekick-runtime-smoke-checklist.md)

## Writing Guidance

- Prefer direct, operational language over marketing copy
- Be explicit about what works today
- Keep the file short enough to skim in one pass
- Link outward instead of embedding long reference material

## Acceptance Criteria

- A new user can find `onboard`, config location, and `gateway` startup in under one minute
- The README makes the embedded web default port (`3456`) visible
- The README makes cross-channel session behavior clear
- The README points users to the runbook for smoke testing and triage
- The README does not contradict current runtime behavior
