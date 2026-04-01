# Feishu Rust Core Channel Design

Status: draft
Date: 2026-04-02
Branch: `main`

## Summary

Bring `nanobot-rs` Feishu support up to the Python channel's core messaging path without attempting full media parity.

This slice keeps the Rust runtime architecture intact and adds the minimum Feishu-specific behavior needed for a usable first-party channel:

- WebSocket long connection for inbound events
- `allowFrom` filtering on inbound senders
- group chat policy with `mention` and `open`
- inbound `text` and `post` parsing
- outbound `text`, `post`, and `interactive` selection
- reply-to-message support through Feishu reply API
- message deduplication and reaction emoji support

The existing Rust outbound-only app-credential sender becomes a full channel implementation that can both receive and send messages through the current `ChannelManager` and `MessageBus`.

## Goals

- Start a Feishu long-connection session from `gateway` without adding another runtime.
- Route supported inbound Feishu messages into the existing agent loop as `InboundMessage`.
- Preserve the current cross-channel session model by using stable `channel/chat_id` mappings.
- Send outbound Feishu replies in the least lossy format available among `text`, `post`, and `interactive`.
- Support quoting the triggering user message when `replyToMessage` is enabled.
- Keep the scope narrow enough to implement and test in one focused plan.

## Non-Goals

- Media upload or download
- Audio transcription
- Rich post-image extraction
- Share-card and interactive-card inbound parsing beyond safe placeholders
- Feishu webhook mode
- Distributed coordination across multiple gateways using the same Feishu app
- Full Python parity for every message type or SDK callback

## Chosen Approach

### Recommended: Native Rust Feishu Channel Using Existing HTTP and WebSocket Stack

Extend the current Rust `FeishuChannel` to own:

- app credential authentication and token caching
- long-connection lifecycle
- event parsing and filtering
- reply context handling
- outbound format selection

This approach reuses current project conventions:

- `reqwest` for HTTP APIs
- `tokio-tungstenite` for long-lived WebSocket sessions
- `MessageBus` for inbound and outbound routing
- local helper structs instead of a large SDK dependency

Why this is the right fit:

- The codebase already has a working Rust channel pattern for long-lived connectors in `wecom`.
- The current Feishu implementation already owns authentication and outbound send logic.
- The requested scope is narrower than what a full SDK abstraction would buy us.
- Avoiding a third-party SDK keeps protocol ownership local and testable.

### Rejected Alternative: Adopt A Community Rust Feishu SDK As The Main Integration Surface

Community crates exist, but they are not clearly the stable default for this codebase. Adopting one now would add version and abstraction risk at the exact layer where the project already has working primitives. A crate can still be reconsidered later if the protocol surface grows enough to justify it.

## Configuration

Extend `channels.feishu` in `nanobot-rs/src/config/mod.rs` with:

```json
"feishu": {
  "enabled": false,
  "appId": "",
  "appSecret": "",
  "apiBase": "https://open.feishu.cn/open-apis",
  "wsBase": "wss://open.feishu.cn/open-apis/ws",
  "encryptKey": "",
  "verificationToken": "",
  "allowFrom": [],
  "reactEmoji": "THUMBSUP",
  "groupPolicy": "mention",
  "replyToMessage": false
}
```

Semantics:

- `enabled`: register and start the channel
- `appId` / `appSecret`: tenant token credentials
- `apiBase`: HTTP API base, overrideable in tests
- `wsBase`: WebSocket long-connection base, overrideable in tests
- `encryptKey` / `verificationToken`: accepted for parity with the Python config surface; this slice does not depend on webhook verification semantics but should not reject these fields
- `allowFrom`: explicit sender allowlist; empty denies all, `["*"]` allows all
- `reactEmoji`: emoji type added to accepted inbound messages
- `groupPolicy`: `mention` or `open`
- `replyToMessage`: whether the first outbound response should use the reply API

## User-Facing Behavior

### Gateway

Running:

```bash
cargo run --release -- gateway
```

with Feishu enabled will:

- authenticate using `appId` and `appSecret`
- establish a long connection to Feishu
- reconnect when the connection drops
- publish supported inbound Feishu events into the existing agent loop

### Inbound Messaging

The channel accepts:

- direct messages from allowlisted senders
- group messages when `groupPolicy = "open"`
- group messages when `groupPolicy = "mention"` and the bot is mentioned

The channel ignores:

- events sent by bots
- duplicate message ids
- senders blocked by `allowFrom`
- unsupported events that do not map cleanly to the Rust MVP

### Outbound Messaging

For normal replies:

- short plain text uses Feishu `text`
- medium text with links but no complex markdown uses Feishu `post`
- headings, code blocks, tables, lists, or long content use Feishu `interactive`

When `replyToMessage = true`, the first outbound response for a turn attempts Feishu's reply API using the inbound `message_id`. If that fails, the channel falls back to ordinary message creation.

Progress and tool-hint filtering continues to use the existing presentation-layer visibility rules already applied to external channels.

## Architecture

### Files And Responsibilities

- Modify `nanobot-rs/src/config/mod.rs`
  - extend `FeishuConfig`
  - add defaults and serde coverage for the new fields
- Modify `nanobot-rs/src/channels/feishu.rs`
  - move from outbound-only sender to full long-connection channel
  - keep Feishu-specific helpers private to this module
- Modify `nanobot-rs/src/channels/mod.rs`
  - keep registration and re-export glue only
- Modify `nanobot-rs/tests/channels.rs`
  - cover long-connection behavior, filtering, reply routing, and outbound format selection
- Modify `nanobot-rs/tests/providers.rs`
  - assert default config serialization for new Feishu fields
- Modify `nanobot-rs/README.md`
  - update Feishu capability and config docs to match actual Rust behavior

No bus or public channel trait redesign is needed for this slice.

### Internal Structure In `feishu.rs`

Split the module into focused units:

- config-backed channel state
- token cache
- WebSocket session runner
- inbound event parsing helpers
- allowlist and mention policy helpers
- outbound message rendering helpers
- reply and reaction HTTP helpers
- bounded dedup cache

The module may remain one file for this slice if it stays readable. If it grows too large during implementation, split parsing or rendering helpers into a sibling private module under `src/channels/feishu/`.

## Data Flow

### Startup Flow

1. `ChannelManager` creates `FeishuChannelHandle`.
2. `start()` validates required config.
3. The channel authenticates or waits until it needs a token.
4. The channel opens a Feishu long connection.
5. The long-connection loop receives event frames and dispatches supported message events.

### Inbound Flow

1. Receive a Feishu message event frame.
2. Parse the envelope and extract:
   - `message_id`
   - `chat_id`
   - `chat_type`
   - `message_type`
   - sender `open_id`
   - optional mentions
   - parent/root message ids when present
3. Drop the event if:
   - sender is a bot
   - the sender is not allowlisted
   - the event is a duplicate
   - the message is a group message that fails the configured policy
4. Add the configured reaction emoji on a best-effort basis.
5. Convert supported content into agent input:
   - `text` -> plain text
   - `post` -> extracted text
6. Build `InboundMessage` with:
   - `channel = "feishu"`
   - `sender_id = sender open_id`
   - `chat_id = chat_id` for group messages
   - `chat_id = sender open_id` for direct messages
   - metadata containing `message_id`, `chat_type`, `msg_type`, `parent_id`, `root_id`
7. Publish the message to `MessageBus`.

### Outbound Flow

1. Existing agent loop emits `OutboundMessage`.
2. `send()` checks presentation filtering.
3. The channel determines the target id type:
   - `chat_id` for group sessions and direct sessions keyed by explicit chat ids
   - `open_id` for direct replies keyed by sender open id
4. The channel chooses `text`, `post`, or `interactive` based on content complexity.
5. If `replyToMessage` and `metadata.message_id` are present, try the reply API first.
6. If reply fails or does not apply, send via `im/v1/messages`.

## Parsing Rules

### Allowlist

Feishu follows the security behavior already documented elsewhere in the repo:

- empty `allowFrom` denies all
- `["*"]` allows all
- otherwise sender `open_id` must match an entry exactly

### Group Policy

- `open`: accept all supported group messages
- `mention`: accept only when the message explicitly mentions the bot or uses `@_all`

### Supported Inbound Message Types

Required in this slice:

- `text`
- `post`

Fallback behavior:

- other message types become a compact placeholder such as `[image]`, `[file]`, or `[interactive]`
- placeholders are only published when they still carry useful conversational meaning; otherwise the event may be ignored

This preserves a useful agent transcript without pulling media support into the plan.

## Failure Handling

### Startup Failures

- missing `appId` or `appSecret` should fail channel startup clearly
- malformed `wsBase` or `apiBase` should produce connector-scoped errors

### Runtime Failures

- token fetch failure should log and retry without crashing the entire gateway
- broken WebSocket connection should reconnect with bounded delay
- malformed event frames should be logged and skipped
- reaction or reply API failures should not block message ingestion
- send failures should return channel-scoped errors to the caller

### Single-Connection Assumption

This design assumes one active gateway process per Feishu app credential set. If multiple instances compete for the same long connection, logs should make the collision clear, but distributed locking is explicitly out of scope.

## Testing Strategy

### Config Tests

- default serialization includes new Feishu fields and expected defaults
- legacy configs without the new fields still deserialize successfully

### Channel Wiring Tests

- `ChannelManager` registers Feishu when enabled
- startup fails clearly when enabled without credentials

### Inbound Behavior Tests

- valid text direct message publishes the correct `InboundMessage`
- `allowFrom` blocks non-matching senders
- empty `allowFrom` denies all
- `["*"]` allows all
- `mention` policy blocks non-mentioned group messages
- `mention` policy accepts bot mentions and `@_all`
- duplicate `message_id` is ignored
- bot-originated messages are ignored
- `post` content is converted into plain text

### Outbound Behavior Tests

- short content sends Feishu `text`
- link-only medium content sends Feishu `post`
- code block, heading, or table content sends Feishu `interactive`
- `replyToMessage` uses reply API on the first send
- failed reply falls back to normal send
- runtime/progress messages remain filtered

### Reliability Tests

- long-connection disconnect triggers reconnect
- malformed frames do not kill the outer channel loop
- concurrent outbound sends do not corrupt token state

### Test Method

Use local HTTP and WebSocket test servers. Do not depend on live Feishu infrastructure in CI.

## Documentation Impact

Update `nanobot-rs/README.md` so it no longer claims Feishu is outbound-only once this slice lands.

Do not update the root repository README to imply full Python parity. The root README can continue to describe the broader Python implementation separately.

## Open Questions Resolved For This Slice

- Rust will not adopt a Feishu SDK as the primary integration layer in this slice.
- Media support is intentionally deferred.
- `encryptKey` and `verificationToken` remain config fields for compatibility even though the Rust MVP does not use webhook verification flow.
