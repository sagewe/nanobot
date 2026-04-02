# Web Shortcut And WeCom Channel Design

## Summary

Add two focused capabilities to `Sidekick`:

1. A faster browser composer flow where `Ctrl+Enter` sends the current message. On macOS, `Cmd+Enter` will do the same.
2. A text-only WeCom smart bot channel, wired into `gateway` alongside Telegram, using the official long-connection model described in [智能机器人长连接](https://developer.work.weixin.qq.com/document/path/101463).

This design keeps the current Rust-first architecture intact. `web` remains a thin UI over the existing agent loop. `wecom` becomes another `Channel` implementation that maps inbound bot events into `InboundMessage` and maps agent replies back into WeCom text responses.

## Scope

### In Scope

- `Ctrl+Enter` and `Cmd+Enter` submit in the browser composer
- Normal `Enter` continues to insert a newline
- New `channels.wecom` config block
- New `WecomBotChannel` started by `gateway`
- Text-only inbound and outbound WeCom messaging
- Reuse of existing `/help`, `/new`, and `/stop` command behavior through the current agent loop
- Heartbeat, reconnect, and reply-context tracking needed for a stable single-bot connection

### Out of Scope

- Markdown, images, files, cards, buttons, or media in WeCom
- Webhook-based WeCom integrations
- Corporate app credentials such as `corp_id` / `agent_id`; this slice uses smart bot `botId` / `secret`
- Streaming UI output
- Presence, welcome messages, active push notifications, or broadcast features
- Online integration tests against the real WeCom service

## Chosen Approach

### Recommended: Native Rust WeCom Long-Connection Channel

Implement WeCom as a native Rust channel that owns its WebSocket lifecycle and plugs into the existing `ChannelManager`.

Why this is the right fit:

- It matches the official smart bot long-connection product instead of the older webhook or internal-app flows.
- It preserves the current architecture: channels publish inbound messages to the bus and send outbound agent replies.
- It avoids introducing a Python or Node runtime into `Sidekick`.

### Rejected Alternatives

#### Wrap the official Node or Python SDK

This would likely reduce protocol work, but it would make deployment and debugging worse and cut across the current “single Rust binary” direction.

#### Use webhook push instead of long connection

This does not match the chosen WeCom product, needs a public callback endpoint, and would create a different operational model from the one requested.

## Configuration

Add a new `channels.wecom` block to `Config`:

```json
"wecom": {
  "enabled": false,
  "botId": "",
  "secret": "",
  "wsBase": "wss://openws.work.weixin.qq.com",
  "allowFrom": []
}
```

Semantics:

- `enabled`: whether `ChannelManager` should register and start the WeCom channel
- `botId`: smart bot identifier from WeCom
- `secret`: smart bot secret from WeCom
- `wsBase`: default WebSocket base URL, overrideable for tests or mocks
- `allowFrom`: optional allowlist of WeCom `userid` values; empty means no extra filter

Deliberately omitted from config for this slice:

- heartbeat interval
- reconnect backoff tuning
- logging verbosity

Those stay internal to keep the operator surface small for the MVP.

## Web UX

The browser composer in `src/web/page.rs` should support:

- `Ctrl+Enter` to submit on all platforms
- `Meta+Enter` to submit on macOS
- plain `Enter` to keep inserting a newline

Implementation constraint:

- Keyboard submission must reuse the exact same path as clicking `Send`
- Existing behavior for trimming, disabling controls during work, clearing input on send, and restoring failed drafts must remain unchanged

## Architecture

### Files And Modules

- Keep `Channel` and `ChannelManager` in `src/channels/mod.rs`
- Add `src/channels/wecom.rs` for:
  - `WecomConfig`
  - `WecomBotChannel`
  - connection lifecycle
  - heartbeat and reconnect handling
  - inbound event parsing
  - outbound text sending
- Extend `src/config/mod.rs` with `WecomConfig` and `channels.wecom`
- Update `src/web/page.rs` for the keyboard shortcut only

This design avoids broad refactoring. If Telegram and WeCom share small utilities, extract private helpers only. Do not redesign the public channel trait for this work.

### Bus Integration

`WecomBotChannel` will follow the same high-level shape as `TelegramChannel`:

1. Maintain a single running connection while enabled
2. Parse inbound bot events
3. Convert supported text messages into `InboundMessage`
4. Publish them to the existing `MessageBus`
5. Use `send(OutboundMessage)` to turn agent replies into WeCom outbound text calls

Public bus types stay unchanged. No new message abstraction is introduced for this slice.

## Data Flow

### Browser Shortcut Flow

1. User types in the existing `<textarea>`
2. `Ctrl+Enter` or `Cmd+Enter` triggers the same submit logic as the `Send` button
3. The form posts to the existing `/api/chat`
4. Existing UI updates and reply rendering stay unchanged

### WeCom Inbound Flow

1. `gateway` starts `WecomBotChannel`
2. The channel opens a WebSocket connection using `botId` and `secret`
3. The channel receives WeCom events
4. For supported text-message events, it extracts:
   - sender `userid`
   - conversation identifier
   - message text
5. It maps them into:
   - `channel = "wecom"`
   - `sender_id = userid`
   - `chat_id = conversation_id`, or `userid` when no conversation id exists
   - `content = message text`
6. The message is published to the bus and handled by the existing agent pipeline

### WeCom Outbound Flow

Because outbound replies need request-specific routing, `WecomBotChannel` will maintain an in-memory reply-context cache keyed by `chat_id`.

For each accepted inbound message, the channel records the reply target required by WeCom. When `send(OutboundMessage)` is called:

1. Find the cached reply context for `msg.chat_id`
2. Build a WeCom text reply using that target
3. Send the reply
4. Keep the cache entry fresh for the next turn in the same conversation

This keeps `OutboundMessage` unchanged and limits WeCom-specific knowledge to the channel.

## Failure Handling

### Startup Errors

- If `enabled=true` but `botId` or `secret` is missing, channel startup should fail with a clear configuration error
- Other channels and the rest of `gateway` should continue to behave normally

### Runtime Errors

- Broken connection or heartbeat timeout: reconnect with capped exponential backoff and light jitter
- Authentication failure or another active connection already holding the bot: log a clear error and retry slowly instead of crashing the process
- Invalid payload or unsupported event type: log and skip only that event
- Missing reply context on outbound send: return a channel-scoped error instead of misrouting the reply

### Single-Connection Constraint

The official WeCom model allows only one active connection per bot. This slice will not add distributed coordination. Operationally, the expectation is one `gateway` instance per `botId`. If a second instance competes for the same bot, logs should make that obvious.

## Testing Strategy

### Web Shortcut Tests

- `Ctrl+Enter` sends the message
- `Cmd+Enter` sends the message
- plain `Enter` adds a newline and does not submit
- send/restore behavior from the current composer stays intact

### Config And Wiring Tests

- default config includes `channels.wecom`
- `ChannelManager` registers `wecom` only when enabled
- startup fails clearly when `enabled=true` and credentials are missing

### WeCom Channel Behavior Tests

- valid text events publish the correct `InboundMessage`
- `allowFrom` filtering works
- `/help`, `/new`, and `/stop` continue to work through the normal agent pipeline
- outbound text replies use the correct cached reply target for the conversation

### Connection Robustness Tests

- disconnect triggers reconnect
- heartbeat timeout triggers reconnect
- malformed payloads do not kill the main loop
- non-text events are ignored cleanly
- concurrent outbound sends do not corrupt reply routing across chats

### Test Method

Use protocol mocks and local WebSocket test servers. Do not depend on the live WeCom service in CI.

## Implementation Notes

- Prefer text-only semantics throughout the first WeCom slice
- Preserve the existing `gateway` command shape
- Keep the feature independent from Python Sidekick config compatibility
- Keep the implementation small enough to plan and ship as one focused slice

## Acceptance Criteria

- `Sidekick web` accepts `Ctrl+Enter` and `Cmd+Enter` as send shortcuts without breaking newline input
- `Sidekick gateway` can start with `channels.wecom.enabled=true`
- a WeCom text message can enter the existing agent loop and receive a text reply
- reconnect and heartbeat failures do not crash the process
- default generated config includes the new WeCom block
