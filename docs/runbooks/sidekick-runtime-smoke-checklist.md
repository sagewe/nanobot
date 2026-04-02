# Sidekick Runtime Smoke Checklist

Use this after a config change, provider switch, or fresh checkout to confirm the current Rust runtime is actually alive. It covers the real surfaces that exist today: `gateway` with embedded web, Telegram, WeCom, Weixin, and the Codex provider.

## Baseline

- Config lives in `~/.sidekick/config.toml` by default.
- Workspace lives in `~/.sidekick/workspace` by default.
- Embedded web listens on `http://127.0.0.1:3456` unless overridden.
- `cargo run --release -- web` still exists as a standalone UI; the smoke path below uses `gateway` because that is the production path for embedded web plus channels.
- The current smoke surface is mostly text-only. Media is not part of this checklist.

## 1. Gateway + Embedded Web

Start the full runtime:

```bash
cd <repo-root>
cargo run --release -- gateway
```

Open `http://127.0.0.1:3456`.

Auth notes:

- The web UI uses the `sidekick_session` cookie only.
- A brand-new empty `web` session now stays visually empty until the first message is sent. That is expected and not a rendering bug.

What success looks like:

- `channels started: [...]`
- `web session <id> started`
- `web session <id> completed`
- `wecom connecting to ...`, `wecom websocket connected`, `wecom subscribe acknowledged` if WeCom is enabled
- `weixin waiting for login` if Weixin is enabled but not logged in yet
- `weixin polling started bot=<bot_id> base=<api_base>` after Weixin login succeeds

Smoke steps:

1. Open the embedded web UI.
2. Create a new `web` session if needed.
3. Send `Reply with exactly OK.`
4. Expect the assistant reply to be exactly `OK`.
5. If you opened a read-only session from another channel, use `Duplicate to Web` before sending.

Failure triage:

- No web page on port `3456`: check whether another process already owns the port.
- No `channels started` line: config was not loaded or the channel is disabled.
- Web request fails: look for `web session <id> failed` in the logs.
- Sign-in succeeds but the UI returns to the login screen: the browser session was not established; delete stale site cookies, then sign in again.
- Read-only session refuses input: duplicate the session into `web` first.

## 2. Telegram

Minimum config:

- `channels.telegram.enabled = true`
- `channels.telegram.token` set to a valid bot token
- `channels.telegram.allowFrom` contains the sender ID, username, or `*`

Start the same `gateway` command as above.

What success looks like:

- Telegram is listed in `channels started: [...]`
- A text message from an allowlisted user reaches the agent
- The assistant reply arrives back in Telegram

Current behavior:

- Telegram is text-only for this runtime smoke.
- `_progress` and `_tool_hint` messages are filtered before send.
- Telegram is intentionally quiet on success; the main signal is the reply appearing in chat.

Failure triage:

- No inbound message: `allowFrom` does not match the sender or the sender is not allowlisted.
- No outbound reply: token is wrong or Telegram returned an HTTP/API error.
- Progress text appears in chat: the message was not marked as runtime metadata.

## 3. WeCom

Minimum config:

- `channels.wecom.enabled = true`
- `channels.wecom.botId` set
- `channels.wecom.secret` set
- `channels.wecom.wsBase` left at the default unless you are behind a custom endpoint

Start `gateway`.

What success looks like:

- `wecom connecting to wss://openws.work.weixin.qq.com`
- `wecom websocket connected`
- `wecom subscribe acknowledged`
- `wecom text callback sender=<id> chat=<id>`
- `wecom reply sent chat=<id>`

Smoke steps:

1. Send a plain text message to the WeCom bot.
2. Confirm the agent receives it.
3. Confirm the reply comes back in the same chat.

Failure triage:

- No connect logs: `botId`, `secret`, or `wsBase` is wrong, or WeCom is disabled.
- Connects but never receives callbacks: sender is blocked by `allowFrom`, or the platform is not delivering the message as text.
- Reconnects repeatedly: heartbeat or network path is unstable.

## 4. Weixin

Weixin is started from the same `gateway` process, but login happens through the embedded web panel.

Minimum config:

- `channels.weixin.enabled = true`
- `channels.weixin.apiBase = https://ilinkai.weixin.qq.com` unless you are using a different backend
- `channels.weixin.cdnBase = https://novac2c.cdn.weixin.qq.com/c2c` unless overridden

Smoke steps:

1. Start `gateway`.
2. Open the embedded web UI.
3. Use the Weixin panel to start login.
4. Scan the QR code and confirm the login on the phone.
5. Send a plain text message to the Weixin bot.

What success looks like:

- `weixin waiting for login` before the account is present
- `weixin polling started bot=<bot_id> base=<api_base>` after login
- `weixin text callback sender=<user_id> chat=<chat_id>` when a message arrives
- `weixin reply sent chat=<chat_id>` after the assistant answers
- `weixin account expired; waiting for relogin` if the stored session expires

Current behavior:

- Weixin currently handles text messages in and out.
- Non-text messages are ignored.
- If the account is missing or expired, the runtime keeps waiting for a fresh login rather than crashing.

Failure triage:

- Only `weixin waiting for login` appears: login was never completed or the account was not persisted.
- `weixin account expired; waiting for relogin`: redo the QR login.
- Message arrives but no reply: the inbound was not text, or the session has no valid `context_token`.

## 5. Codex Provider

Codex uses the local ChatGPT login state in `~/.codex/auth.json`.

Required conditions:

- `auth_mode` must be `chatgpt`
- `tokens.access_token`, `tokens.refresh_token`, `tokens.id_token`, and `tokens.account_id` should exist
- `providers.codex.apiBase` should point at the Codex backend, currently `https://chatgpt.com/backend-api/codex`
- There is no fallback to `OPENAI_API_KEY`

Fast smoke:

```bash
cd <repo-root>
cargo run --release -- agent \
  --config /tmp/sidekick-codex-smoke.json \
  --session cli:smoke \
  --message "Reply with exactly OK."
```

What success looks like:

- The command prints `OK`
- `/model codex:gpt-5.4` works in interactive `agent` mode
- A tool call followed by a tool result returns a final assistant reply

Failure triage:

- `codex provider error 400 ...`: the request shape or profile extras are wrong.
- `codex provider error 404 ...`: the Codex base URL is wrong.
- `auth_mode must be 'chatgpt'`: the local auth file is not a valid ChatGPT/Codex login state.
- Missing token/account errors: the local Codex login file is incomplete or unreadable.

## 6. Quick Triage Map

- `channels started` missing: config or enable flags are wrong.
- Web works but channel replies do not: check the channel-specific logs above.
- Telegram silent: confirm sender is allowlisted.
- WeCom reconnects: check WebSocket reachability and the bot credentials.
- Weixin waits forever: complete the QR login in the web panel.
- Codex fails before replying: inspect `~/.codex/auth.json` and the `providers.codex` block first.
