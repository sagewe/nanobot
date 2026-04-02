# Sidekick

`Sidekick` is now the primary runtime for this repository. The repo root contains the active Rust codebase plus the embedded web frontend; the old Python runtime has been removed.

## Current Status

The current runtime supports these main paths:

- `agent`
- `gateway` with embedded web
- `users` control-plane commands
- `feishu`
- `telegram`
- `wecom`
- `weixin`
- `openai`
- `custom`
- `openrouter`
- `ollama`
- `codex`

Sidekick is usable today, but it is still evolving and should not be treated as a claim of complete parity with the legacy implementation.

## Quick Start

From the repository root:

```bash
cargo run --release -- onboard \
  --admin-username alice \
  --admin-password change-me
```

This bootstraps a multi-user control plane under `~/.sidekick`:

- `control/` holds system state, users, audit logs, and web sessions
- `users/<user_id>/config.toml` is the per-user runtime config
- `users/<user_id>/workspace/` is the per-user workspace

If a legacy single-user install already exists under `~/.nanobot-rs/`, `onboard` will migrate that config and workspace into the first admin automatically.

After bootstrapping, edit the generated user config, then start the main runtime:

```bash
cargo run --release -- gateway
```

Open the embedded web UI at `http://127.0.0.1:3456`.

For a direct CLI request, pass the bootstrapped username explicitly:

```bash
cargo run --release -- agent --user alice --message "hello"
```

For an interactive CLI session:

```bash
cargo run --release -- agent --user alice --session cli:dev
```

## Configuration

The canonical config format is TOML. The runtime still reads legacy JSON configs when a sibling `config.toml` is missing.

The current config model is profile-based:

- `agents.defaults.defaultProfile` selects the default profile for new sessions
- `agents.profiles` defines available `provider:model` choices
- `providers` contains provider-specific connection settings
- `channels` enables Telegram, Feishu, WeCom, or Weixin
- `tools` contains tool-specific settings such as web search and fetch

Minimal example:

```toml
[agents.defaults]
defaultProfile = "openai:gpt-4.1-mini"

[agents.profiles."openai:gpt-4.1-mini"]
provider = "openai"
model = "gpt-4.1-mini"

[providers.openai]
apiKey = "sk-..."
apiBase = "https://api.openai.com/v1"

[channels.telegram]
enabled = false
token = ""
allowFrom = []
apiBase = "https://api.telegram.org"

[tools.web.search]
provider = "duckduckgo"
```

## Channels and Web

`gateway` is the main long-running entrypoint. It starts:

- enabled user runtimes
- enabled channels
- the embedded web UI

The standalone web server still exists:

```bash
cargo run --release -- web
```

But the main operational path is `gateway`, because it combines embedded web with the active channel runtime.

The browser UI can inspect grouped sessions across channels. `web` sessions are writable. Sessions from other channels are read-only in the browser until duplicated into a writable `web` session.

Feishu currently supports long-connection inbound messaging, sender allowlists, group `mention` and `open` policy handling, reply-to-message routing, reaction emoji acknowledgements, and outbound `text`, `post`, and `interactive` delivery.

## Provider Notes

OpenAI-compatible providers are selected through profiles and currently include:

- `openai`
- `custom`
- `openrouter`
- `ollama`

`codex` is a separate provider. It uses the local ChatGPT/Codex login state in `~/.codex/auth.json` and does not fall back to `OPENAI_API_KEY`.

Current Codex backend base:

```text
https://chatgpt.com/backend-api/codex
```

In session, use:

```text
/models
```

to inspect profiles, and:

```text
/model provider:model
```

to switch the current session.

## Current Limitations

- The runtime is still evolving.
- Most channel paths are text-first.
- Weixin currently handles text messages only.
- Non-`web` sessions are read-only in the browser until duplicated into a writable `web` session.

## Development

Useful commands from the repository root:

```bash
cargo test
cd frontend && npm test -- --run
cd frontend && npm run build
cargo run --release -- gateway
```

For smoke testing, expected logs, and channel/provider triage, use the runbook:

- [Sidekick runtime smoke checklist](/Users/sage/nanobot/docs/runbooks/sidekick-runtime-smoke-checklist.md)
