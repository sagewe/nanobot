# Codex Live Transport Design

## Summary

Replace the current mock-oriented Codex provider transport with a minimal live transport that works against the real ChatGPT/Codex backend using the local `~/.codex/auth.json` login state. Keep `codex` as a distinct provider. Do not add browser login, token refresh, or UI streaming in this slice.

## Problem

The current `codex` provider is not usable against the real backend:

- it sends `POST {apiBase}/responses`
- the default `apiBase` is `https://chatgpt.com/backend-api`
- real runtime calls therefore hit `https://chatgpt.com/backend-api/responses`
- real requests return `404 Not Found`

Evidence from the locally installed Codex client shows that the real transport is not the current plain HTTP path:

- the binary contains `https://chatgpt.com/backend-api/codex`
- the binary contains `responses_websocket`
- the binary contains `codex-api/src/sse/responses.rs`
- local logs show `codex_api::endpoint::responses_websocket`
- local logs show `codex_api::sse::responses`

This means the current provider is failing because the protocol assumption is wrong, not because of user configuration or auth-file parsing.

## Goals

- Use real `~/.codex/auth.json` credentials with `auth_mode = "chatgpt"`
- Support normal text responses from the real backend
- Support tool-call responses from the real backend
- Preserve the existing agent/tool loop contract:
  - provider returns `LlmResponse`
  - agent executes tools
  - provider handles the next round
- Keep Codex separate from `openai`

## Non-Goals

- No browser login flow
- No token refresh or rewriting `~/.codex/auth.json`
- No token-by-token UI streaming
- No attempt to fully clone the official Codex CLI protocol surface
- No realtime UI updates in Web/Telegram/WeCom

## Recommended Approach

Implement a minimal live Codex event transport behind the existing `LlmProvider` interface. Internally, the provider may use SSE or websocket semantics, but externally it must still return one aggregated `LlmResponse` per call.

### Why this approach

- It fixes the real 404 instead of layering more guesses on top of `/responses`
- It preserves the current agent/session/tool architecture
- It supports tool-call turns without forcing a provider-internal tool executor
- It keeps the complexity localized to `providers/codex.rs`

## Transport Decision

First implementation target: **minimal event-stream transport**, not plain request/response.

Use the real Codex backend path rooted at:

- `https://chatgpt.com/backend-api/codex`

The provider should implement whichever of SSE or websocket is easier to stabilize first, but the runtime contract for this slice is:

- connect to the real backend
- submit one model request
- collect backend events until completion
- aggregate those events into a final `LlmResponse`

The provider must not expose streaming details to callers in this slice.

## Architecture

### `CodexProvider`

Keep `CodexProvider` as the public runtime entry for the `codex` provider.

Responsibilities:

- load and validate the auth file
- build the live transport client
- submit one request
- aggregate the event stream into one `LlmResponse`

### `CodexTransport`

Add a private transport layer inside `providers/codex.rs` or a small sibling module.

Responsibilities:

- build request headers from auth
- establish the live connection
- submit the request
- yield parsed backend events until the turn completes

### `CodexAggregator`

Add a private event aggregator.

Responsibilities:

- consume transport events
- accumulate assistant text
- accumulate function-call payloads
- derive final `finish_reason`
- preserve relevant extra fields

This boundary keeps protocol complexity out of the agent layer.

## Request Semantics

The provider continues to accept the same high-level inputs:

- `messages`
- `tools`
- `ProviderRequestDescriptor`

The provider maps them into the backend request shape.

Requirements:

- `provider = "codex"` remains explicit
- `OPENAI_API_KEY` is never consulted
- `access_token` from `~/.codex/auth.json` is used for auth
- `ChatGPT-Account-Id` from `account_id` is included
- request extras from the active profile continue to flow into the transport request

## Event Semantics

First slice only needs to support the minimum event set required for non-streaming completion:

- a final assistant message item
- a final function-call item
- completion/end-of-turn signal

Based on local evidence, likely relevant events include:

- `response.output_item.done`
- `response.function_call_arguments.delta`
- `response.function_call_arguments.done`
- `response.completed`

The implementation should prefer completed output-item events when available. Argument delta assembly should be used only when a complete function-call payload is not already present in the final item.

## Aggregation Rules

### Assistant text

- collect only assistant-authored output text
- join text fragments in order
- return `LlmResponse.content = Some(text)` if any text is present

### Tool calls

- detect final function-call items
- produce `ToolCall { id, name, arguments }`
- if arguments arrive incrementally, assemble them before finalizing

### Finish reason

- if one or more tool calls are present, use `tool_calls`
- otherwise use `stop`

### Errors

- auth/config errors remain fatal
- malformed protocol payloads remain fatal
- retry only transient transport failures

## Integration Rules

The agent loop remains unchanged:

- Codex provider returns tool calls
- existing agent tool execution runs normally
- follow-up request goes back through the Codex provider

Do not implement provider-internal tool execution.

## Configuration

Keep the current public config shape:

```json
"providers": {
  "codex": {
    "authFile": "~/.codex/auth.json",
    "apiBase": "https://chatgpt.com/backend-api"
  }
}
```

Do not add new required user-facing config for this slice unless real transport evidence proves it is necessary.

## Testing Strategy

### 1. Event aggregation tests

Add focused tests covering:

- assistant text aggregation
- function-call aggregation
- incremental function-call argument assembly
- malformed event sequences failing clearly

### 2. Live-transport contract tests

Use a mock SSE or websocket server to verify:

- correct request path under `/backend-api/codex`
- bearer auth from `access_token`
- `ChatGPT-Account-Id` header
- request model and tool payloads
- response aggregation into `LlmResponse`

### 3. Agent integration tests

Add integration coverage proving:

- `/model codex:gpt-5.4` selects the Codex profile
- a Codex turn can return a function call
- the agent executes the function
- the next Codex turn consumes the tool result and returns final text

## Acceptance Criteria

- Real backend no longer 404s due to `/backend-api/responses`
- `codex` profile can return plain assistant text
- `codex` profile can return tool calls
- agent tool loop works end-to-end with the Codex provider
- `OPENAI_API_KEY` fallback is still absent
- no streaming UI work is introduced

## Risks

- The backend may require websocket rather than SSE for full compatibility
- The backend may expect request fields beyond the current inferred shape
- Some event variants may differ from the ones observed locally

To control risk, the implementation must start with a transport contract test and only support the minimum event set necessary for text and tool-call completion.
