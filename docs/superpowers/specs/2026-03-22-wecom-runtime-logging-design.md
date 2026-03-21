# WeCom Runtime Logging Design

## Summary

Add operator-facing runtime logs to the existing WeCom smart-bot channel so a human running `nanobot-rs gateway` can tell whether the bot is connecting, subscribed, receiving callbacks, sending replies, and reconnecting.

This is a logging-only slice. It must not change the WeCom protocol behavior, channel lifecycle, message routing, heartbeat cadence, reconnect strategy, or reply semantics.

## Scope

### In Scope

- Add positive lifecycle logs for WeCom connection state
- Add positive logs for inbound text callbacks and outbound replies
- Add reconnect logs that explain why a reconnect is happening
- Add debug-level logs for noisy-but-useful diagnostic signals
- Add tests that verify the expected log lines appear

### Out of Scope

- Any change to WeCom protocol payloads
- Any new config fields for log verbosity
- Any change to heartbeat interval or reconnect timing
- Logging full payloads, message bodies, bot secrets, or any sensitive content

## Chosen Approach

### Recommended: Layered `info` + `debug` Logging

Use `info` for state transitions an operator actively cares about, and `debug` for higher-frequency protocol-adjacent details.

This keeps default logs useful while preserving a deeper troubleshooting mode via `RUST_LOG=debug`.

### Rejected Alternatives

#### Log everything at `info`

Too noisy for normal operation. `pong` and allowlist drops would quickly drown out the useful signals.

#### Add no positive logs and rely on errors only

This is the current problem. Operators can see failures but cannot tell whether a healthy connection exists.

## Logging Contract

### `info` Logs

These should appear during healthy operation:

- `wecom connecting to <wsBase>`
- `wecom websocket connected`
- `wecom subscribe acknowledged`
- `wecom text callback sender=<userid> chat=<chatid>`
- `wecom reply sent chat=<chatid>`
- `wecom reconnecting in <delay> after: <error>`
- `wecom channel stopped`

### `debug` Logs

These should appear only when debug logging is enabled:

- `wecom pong received`
- `dropping wecom message from blocked sender <userid>`
- `wecom reply context updated chat=<chatid> req_id=<req_id>`

## Constraints

- Do not log message content
- Do not log `secret`
- Do not log full payload JSON
- Do not add logs for every `ping`
- Do not change public interfaces or config structure

## File Changes

- Modify `nanobot-rs/src/channels/wecom.rs`
- Modify or add tests under `nanobot-rs/tests/wecom.rs`

No other files should need behavioral changes.

## Testing Strategy

Add one focused log-capture test around the existing mock WeCom server flow. Verify that a normal connection lifecycle emits the expected `info` logs:

- connecting
- websocket connected
- subscribe acknowledged
- text callback
- reply sent

Add one reconnect-oriented assertion that a forced disconnect emits:

- reconnecting in ...

The test should not assert exact UUIDs, full error text, or unstable timing numbers beyond the presence of the reconnect message.

## Acceptance Criteria

- Running `RUST_LOG=info cargo run --release -- gateway` shows positive WeCom lifecycle logs during a healthy session
- Running with `RUST_LOG=debug` additionally shows `pong received`, allowlist drops, and reply-context updates
- No sensitive values or message bodies are logged
- Existing WeCom behavior and tests remain unchanged except for the addition of logging assertions
