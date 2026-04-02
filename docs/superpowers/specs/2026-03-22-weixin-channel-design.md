# Weixin Channel Design

Status: draft
Date: 2026-03-22
Branch: `codex/weixin-channel`

## Summary

Add a first-party Weixin channel to `Sidekick` using the backend contract documented in [WEIXIN_BACKEND_SPEC.md](<repo-root>/docs/WEIXIN_BACKEND_SPEC.md).

This first version is intentionally narrow:

- single account per gateway
- QR login initiated from the embedded web console only
- text-only direct-message receive/reply
- persistent login state, long-poll cursor, and reply context tokens
- no media upload/download
- no typing indicators
- no group-chat support

## Goals

- Let `gateway` log into a Weixin bot account through a browser-driven QR flow.
- Persist account state so the gateway can resume polling after restart.
- Long-poll inbound Weixin messages and route them into the existing agent loop.
- Reply to inbound direct text messages through `sendmessage`.
- Make Weixin sessions visible in the embedded web console alongside `web`, `telegram`, and `wecom`.

## Non-Goals

- Multi-account support
- CLI login flow
- Group message support
- Media send/receive
- Typing indicator integration
- Rich Weixin-specific formatting beyond plain text
- Background auto-refresh of expired login without explicit user re-login

## User-Facing Behavior

### Gateway

Running:

```bash
cargo run --release -- gateway
```

will continue to start channels plus the embedded web console. If Weixin is enabled, the gateway will:

- load any persisted Weixin account state from the workspace
- start polling immediately if a valid `bot_token` exists
- otherwise remain idle until a web-driven login is completed

### Embedded Web

The embedded web console gains a Weixin account section with:

- current login status
- QR-code login start action
- QR image/status display
- logout / clear-session action

The existing session browser will show `weixin` sessions as read-only cross-channel sessions, with the same duplicate-to-web behavior used for other non-web channels.

### Weixin Messaging

The Weixin channel will:

- accept inbound direct text messages only
- ignore group messages and non-text items, with logs
- send plain-text replies only
- require a cached `context_token` for replies

## Architecture

### Configuration

Add `channels.weixin` to [config/mod.rs](<repo-root>/.worktrees/weixin-channel/src/config/mod.rs):

```json
"weixin": {
  "enabled": false,
  "apiBase": "https://ilinkai.weixin.qq.com",
  "cdnBase": "https://novac2c.cdn.weixin.qq.com/c2c"
}
```

This config contains only static connector settings. Runtime account state is not stored in `config.json`.

### Workspace State

Persist Weixin runtime state under the workspace, for example:

```text
<workspace>/channels/weixin/account.json
<workspace>/channels/weixin/context_tokens.json
```

`account.json` stores:

- `bot_token`
- `ilink_bot_id`
- `baseurl`
- `ilink_user_id`
- `get_updates_buf`
- `status`
- `updated_at`

`context_tokens.json` stores the latest `context_token` keyed by peer user id.

### Channel Structure

Add [channels/weixin.rs](<repo-root>/.worktrees/weixin-channel/src/channels/weixin.rs) with:

- `WeixinChannel`
- `WeixinClient`
- `WeixinAccountStore`
- `WeixinLoginManager`
- request/response parsing helpers

Keep [channels/mod.rs](<repo-root>/.worktrees/weixin-channel/src/channels/mod.rs) as registry glue only:

- register `WeixinChannel` in `ChannelManager`
- re-export narrow test helpers where useful

### Web Structure

Extend the embedded web server to expose Weixin management APIs:

- `GET /api/weixin/account`
- `POST /api/weixin/login/start`
- `GET /api/weixin/login/status`
- `POST /api/weixin/logout`

The existing `GET /api/sessions` and session detail endpoints remain the source of truth for browsing Weixin conversation sessions.

## Detailed Flow

### Login Flow

1. User opens embedded web console.
2. User clicks `Login to Weixin`.
3. Backend calls `GET /ilink/bot/get_bot_qrcode?bot_type=3`.
4. Backend returns login session data to the browser:
   - `qrcode`
   - `qrcode_img_content`
5. Browser renders the QR image.
6. Browser polls backend login status endpoint.
7. Backend polls `GET /ilink/bot/get_qrcode_status?qrcode=<qrcode>`.
8. When status becomes `confirmed`, backend persists:
   - `bot_token`
   - `ilink_bot_id`
   - returned `baseurl` if present, otherwise configured `apiBase`
   - optional `ilink_user_id`
   - empty `get_updates_buf` if absent
9. Poll loop becomes eligible to start automatically.

### Polling Flow

When account state includes a `bot_token`, `WeixinChannel` runs a long-poll loop against:

- `POST {effective_baseurl}/ilink/bot/getupdates`

Behavior:

- send `get_updates_buf` from persisted account state
- append `base_info.channel_version`
- use required auth headers:
  - `AuthorizationType: ilink_bot_token`
  - `Authorization: Bearer <bot_token>`
  - `X-WECHAT-UIN: <base64(random u32 decimal string)>`
- treat client-side timeouts as normal empty polls
- persist updated `get_updates_buf` after successful responses
- adapt client timeout using `longpolling_timeout_ms` if server provides it

If server returns `errcode = -14`:

- mark the account as expired
- stop normal polling
- surface expired state in the web console
- require explicit web re-login

### Inbound Routing

For each inbound message:

- require `message_type == 1`
- ignore messages with a non-empty `group_id`
- find first supported text item from `item_list`
- cache `context_token` by `from_user_id`
- publish inbound message:
  - `channel = "weixin"`
  - `sender_id = from_user_id`
  - `chat_id = from_user_id`
  - `content = text`
  - metadata includes message ids/timestamps when useful

This makes the session key effectively:

```text
weixin:<from_user_id>
```

### Outbound Reply Flow

When the agent emits an outbound message for `channel == "weixin"`:

1. Filter through the existing presentation visibility rules.
2. Look up the cached `context_token` for `chat_id`.
3. If no token exists, fail the send with a clear error and log it.
4. Send:

```json
{
  "msg": {
    "from_user_id": "",
    "to_user_id": "<chat_id>",
    "client_id": "<generated unique id>",
    "message_type": 2,
    "message_state": 2,
    "item_list": [
      {
        "type": 1,
        "text_item": {
          "text": "<plain text reply>"
        }
      }
    ],
    "context_token": "<cached context token>"
  },
  "base_info": {
    "channel_version": "<Sidekick channel version>"
  }
}
```

Markdown rendering is explicitly out of scope for Weixin v1. Outbound content should be flattened to plain text.

## Web API Contract

### `GET /api/weixin/account`

Returns persisted account summary and runtime status:

- `enabled`
- `loggedIn`
- `expired`
- `botId`
- `userId`
- `baseUrl`
- `updatedAt`

### `POST /api/weixin/login/start`

Starts a QR login and returns:

- `qrcode`
- `qrcodeImgContent`

Server keeps the QR token in ephemeral login state for follow-up status polling.

### `GET /api/weixin/login/status`

Returns current login progress:

- `wait`
- `scaned`
- `confirmed`
- `expired`
- `failed`

When confirmed, persisted account state should already be updated.

### `POST /api/weixin/logout`

Clears persisted account credentials, cursor, and cached context tokens. The running poller should stop or become idle immediately.

## Logging

Add balanced runtime logs similar to WeCom:

- `weixin polling started`
- `weixin qr login started`
- `weixin qr status=<status>`
- `weixin login confirmed account=<ilink_bot_id>`
- `weixin message received sender=<from_user_id>`
- `weixin reply sent chat=<chat_id>`
- `weixin session expired`
- `weixin polling retry after timeout/error`

High-frequency or noisy details stay at `debug`, not `info`.

## Failure Semantics

- Missing config when enabled: startup error
- Missing persisted account state: channel stays idle, not failed
- Missing `context_token` on outbound reply: message send fails, session keeps running
- Poll timeout: normal retry
- `errcode = -14`: account marked expired, user must re-login
- Unsupported inbound items: ignored with log, no crash

## Testing Strategy

### Config and State

- default config includes `channels.weixin`
- workspace account store round-trips persisted state
- context token cache round-trips by peer id

### Login Flow

- start login returns QR payload
- status polling handles `wait`, `scaned`, `confirmed`, `expired`
- confirmed login persists `bot_token`, `baseurl`, `ilink_bot_id`

### Polling and Routing

- `getupdates` request includes required headers and `base_info.channel_version`
- non-empty `get_updates_buf` persists after success
- direct text message becomes `InboundMessage`
- group messages are ignored
- non-text items are ignored
- `errcode = -14` marks account expired

### Sending

- outbound text send includes cached `context_token`
- missing `context_token` returns an error
- presentation-filtered internal runtime messages are not delivered

### Embedded Web

- account/status endpoints return expected shape
- session list contains `weixin` group when messages exist
- Weixin sessions are read-only in the web console
- duplicate-to-web works for Weixin sessions

## Open Questions Deferred

- Media upload/download
- Typing indicators
- Group-chat semantics
- Multi-account routing
- CLI login
