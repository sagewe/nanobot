# Codex Provider Runtime Design

Status: draft
Date: 2026-03-22
Branch: `main`

## Summary

Add a first-class `codex` provider to `nanobot-rs` that uses the local Codex/ChatGPT login state from `~/.codex/auth.json` and speaks the Codex runtime protocol directly.

This provider is intentionally separate from the existing `openai` provider:

- it does not read `OPENAI_API_KEY`
- it does not fall back to `providers.openai.apiKey`
- it does not reuse the OpenAI-compatible `/chat/completions` transport
- it does not add browser login, callback handling, or auth UI to `nanobot-rs`

The first version is runtime-only:

- explicit `provider = "codex"` in model profiles
- local auth-file validation
- Codex protocol request execution
- clear fatal errors when the local Codex login state is missing or unusable

## Goals

- Let a session profile select `provider = "codex"` and run through the normal `AgentLoop`.
- Load Codex auth from `~/.codex/auth.json` or an explicitly configured alternate path.
- Use the same Codex backend family the local CLI uses rather than `api.openai.com/v1/chat/completions`.
- Preserve the current `LlmProvider` abstraction so CLI, Web, Telegram, WeCom, and Weixin continue to work unchanged above the provider layer.
- Keep profile-scoped request extras working for Codex-backed profiles.

## Non-Goals

- Browser-based OAuth login inside `nanobot-rs`
- Web or CLI auth-status dashboards
- Automatic fallback to API-key auth
- Automatic migration from `openai` profiles to `codex`
- Automatic refresh and rewrite of `~/.codex/auth.json`
- Treating `codex` as an `authMode` inside `providers.openai`

## User-Facing Behavior

### Configuration

`ProvidersConfig` gains a new top-level `codex` block:

```json
"providers": {
  "codex": {
    "authFile": "/Users/sage/.codex/auth.json",
    "apiBase": "https://chatgpt.com/backend-api"
  }
}
```

`agents.profiles` may then select it explicitly:

```json
"agents": {
  "defaults": {
    "defaultProfile": "codex:gpt-5.4",
    "workspace": "/Users/sage/.nanobot-rs/workspace",
    "maxToolIterations": 20
  },
  "profiles": {
    "codex:gpt-5.4": {
      "provider": "codex",
      "model": "gpt-5.4",
      "request": {
        "reasoning_effort": "high"
      }
    }
  }
}
```

### Runtime Behavior

When a session uses a `codex:*` profile:

- `nanobot-rs` reads the configured Codex auth file
- validates that the auth file represents a ChatGPT/Codex login
- constructs a Codex runtime client
- sends the turn through the Codex backend protocol
- normalizes the result back into `LlmResponse`

If the auth file is missing, malformed, or incompatible, the provider fails immediately with a clear configuration/auth error. It does not silently fall back to API-key auth.

## Local Protocol Evidence

This design is based on local runtime artifacts already present on the machine:

- `~/.codex/auth.json` exists and contains:
  - `auth_mode = "chatgpt"`
  - `tokens.access_token`
  - `tokens.refresh_token`
  - `tokens.id_token`
  - `tokens.account_id`
- the local `codex` binary is `codex-cli 0.115.0`
- local binary strings and logs show Codex runtime traffic uses:
  - `https://chatgpt.com/backend-api`
  - `responses_websocket`
  - `codex_api::sse::responses`
  - `/backend-api`
  - `/oauth/token`

This is sufficient to conclude that the Codex runtime path is distinct from the existing OpenAI-compatible provider path and should be implemented as a separate provider.

## Architecture

### Configuration Layer

Extend [config/mod.rs](/Users/sage/nanobot/nanobot-rs/src/config/mod.rs) with:

- `CodexProviderConfig`
- `ProvidersConfig.codex`
- provider validation that accepts `"codex"` in profiles

Suggested fields:

- `authFile: String`
- `apiBase: String`

Defaults:

- `authFile = "~/.codex/auth.json"`
- `apiBase = "https://chatgpt.com/backend-api"`

### Provider Registry

Extend [providers/registry.rs](/Users/sage/nanobot/nanobot-rs/src/providers/registry.rs) to:

- resolve `provider = "codex"`
- validate that a `codex` config block exists when referenced
- build a `CodexProvider`

Keep [providers/mod.rs](/Users/sage/nanobot/nanobot-rs/src/providers/mod.rs) as re-export/factory glue.

### New Provider Module

Add [providers/codex.rs](/Users/sage/nanobot/nanobot-rs/src/providers/codex.rs) with:

- auth-file parsing and validation
- Codex runtime request client
- protocol-to-`LlmResponse` normalization
- error mapping into the existing retry/fatal boundary

## Auth Source and Validation

The auth file is treated as the only authentication source for this provider.

Validation rules:

- the file must exist and deserialize as JSON
- `auth_mode` must equal `"chatgpt"`
- `tokens.access_token` must exist and be non-empty
- `tokens.refresh_token` must exist and be non-empty
- `tokens.id_token` must exist and be non-empty
- `tokens.account_id` must exist and be non-empty

Explicit exclusions:

- do not read `OPENAI_API_KEY`
- do not consult `providers.openai.apiKey`
- do not auto-detect Codex from an OpenAI profile

First-version auth lifecycle:

- read local auth
- use it as-is
- if the backend rejects it, surface an auth error
- require the operator to refresh login externally via Codex

## Transport and Protocol

The `codex` provider should target the same backend family the local Codex CLI uses:

- base URL rooted at `https://chatgpt.com/backend-api`
- responses-style session transport rather than `/v1/chat/completions`

Implementation constraint:

- the rest of `nanobot-rs` still expects a non-streaming `LlmProvider::chat(...) -> LlmResponse`
- therefore the provider may internally use Codex SSE or websocket transport, but it must collapse the interaction into one final `LlmResponse`

Internal transport details are provider-private. The important contract is:

- request built from session profile + message history + tool schema
- response normalized into:
  - final assistant text
  - tool calls
  - finish reason
  - extra assistant fields when relevant

## Request Construction

For a Codex-backed request, the provider must combine:

- current profile `model`
- current profile `request` JSON extras
- session messages mapped into the Codex protocol's expected content shape
- tool definitions from the current `ToolRegistry`

Rules:

- profile `request` extras are merged into the provider request
- runtime-owned fields override conflicting extras when necessary
- unknown request extras are preserved when safe to forward

This preserves the existing profile-based model parameter system for Codex without treating Codex as OpenAI-compatible.

## Response Normalization

The provider must map Codex protocol responses back into `LlmResponse`.

Required outcomes:

- assistant text content is captured
- function/tool calls are captured in the existing internal format
- assistant-side extra protocol fields can be preserved when relevant for replay

This keeps `AgentLoop`, session persistence, and the current tool-call loop unchanged above the provider.

## Error Semantics

Errors should be classified into four buckets:

- configuration errors
  - auth file missing
  - JSON malformed
  - missing required fields
  - unsupported `auth_mode`
- authentication errors
  - expired or rejected local auth
  - backend denies the bearer/session
- protocol errors
  - unexpected response shape
  - unsupported Codex event sequence
- transient runtime errors
  - transport failure
  - backend timeout
  - 429 / overload / temporary server failure

Retry behavior:

- retry transient runtime errors only
- do not retry configuration or authentication errors
- do not hide fatal auth/config failures behind fallback behavior

## Tests

Add coverage for:

### Configuration

- `providers.codex` deserializes with defaults
- `provider = "codex"` is accepted in profiles
- missing `providers.codex` block is rejected when referenced

### Auth File Handling

- missing auth file fails clearly
- malformed auth JSON fails clearly
- `auth_mode != "chatgpt"` fails clearly
- missing required token fields fail clearly
- `OPENAI_API_KEY` is not consulted

### Provider Runtime

- request construction includes the selected model and request extras
- transport/auth headers are derived from the auth file, not API-key config
- transient transport failures retry
- auth failures do not retry
- protocol response normalizes into `LlmResponse`

### Integration

- a `codex:*` profile can be selected through `/model`
- `AgentLoop` invokes the `codex` provider when the session profile selects it
- the rest of the session/profile flow is unchanged

## Open Questions for Implementation

These are implementation questions to resolve from local Codex behavior before writing code, not user-facing product questions:

- exact Codex request path under `https://chatgpt.com/backend-api`
- whether the minimal first version should use SSE, websocket, or a provider-private fallback order
- exact auth header composition beyond bearer token and account identity
- exact response event sequence required to reconstruct tool calls

The implementation should resolve these from the local Codex client behavior first, not from third-party reverse engineering.
