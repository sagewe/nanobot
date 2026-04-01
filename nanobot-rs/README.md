# nanobot-rs

`nanobot-rs` is the Rust runtime for the nanobot project. This guide covers the current Rust implementation only. The repository root [README.md](/Users/sage/nanobot/README.md) still documents the broader Python-first project and project background.

## Current Status

The Rust runtime currently supports these primary paths:

- `agent`
- `gateway` with embedded web
- `feishu`
- `telegram`
- `wecom`
- `weixin`
- `openai`
- `custom`
- `openrouter`
- `ollama`
- `codex`

This runtime is already usable, but it is still evolving and should be treated as a focused Rust implementation rather than a claim of full feature parity with the Python runtime.

## Quick Start

From the repository root:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo run --release -- onboard
```

This creates the default config at `~/.nanobot-rs/config.json` and the default workspace at `~/.nanobot-rs/workspace`.

Edit `~/.nanobot-rs/config.json`, then start the main runtime:

```bash
cargo run --release -- gateway
```

Open the embedded web UI at `http://127.0.0.1:3456`.

If you only want a direct CLI session, you can also run:

```bash
cargo run --release -- agent
```

## Configuration

The current config model is profile-based:

- `agents.defaults.defaultProfile` selects the default profile for new sessions
- `agents.profiles` defines the available `provider:model` choices
- `providers` contains provider-specific connection settings
- `channels` enables Telegram, Feishu, WeCom, or Weixin
- `tools` contains tool-specific settings such as web search/fetch

Minimal example:

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

If `config.toml` already exists, the runtime will also load that format, but `onboard` uses JSON by default.

## Channels and Web

`gateway` is the main long-running entrypoint. It starts:

- the agent loop
- enabled channels
- the embedded web UI

The embedded web UI can browse grouped sessions across channels. `web` sessions are writable. Sessions from other channels are read-only in the browser and must be duplicated into a writable `web` session before you continue chatting there.

The standalone web command still exists:

```bash
cargo run --release -- web
```

But the main operational path is `gateway`, because it combines embedded web with the active channel runtime.

Feishu in the Rust runtime currently supports long-connection inbound messaging, sender allowlists, group `mention`/`open` policy handling, reply-to-message routing, reaction emoji acknowledgements, and outbound `text` / `post` / `interactive` delivery with app credentials.

## Provider Notes

OpenAI-compatible providers are selected through profiles and currently include:

- `openai`
- `custom`
- `openrouter`
- `ollama`

`codex` is a separate provider. It uses the local ChatGPT/Codex login state in `~/.codex/auth.json` and does not fall back to `OPENAI_API_KEY`.

For Codex, the current backend base is:

```text
https://chatgpt.com/backend-api/codex
```

You can inspect available profiles in-session with:

```text
/models
```

And switch the current session with:

```text
/model provider:model
```

## Current Limitations

- The Rust runtime is still evolving.
- Most channel paths are text-first.
- Weixin currently handles text messages only.
- Non-`web` sessions are read-only in the browser until duplicated into a writable `web` session.
- This README describes the Rust runtime as it works today; it does not try to mirror every capability documented in the root project README.

## Development

Useful commands:

```bash
cd /Users/sage/nanobot/nanobot-rs
cargo test
cargo run --release -- gateway
```

For smoke testing, expected logs, and channel/provider triage, use the runbook:

- [nanobot-rs runtime smoke checklist](/Users/sage/nanobot/docs/runbooks/nanobot-rs-runtime-smoke-checklist.md)
