# Gateway Embedded Web Cross-Channel Sessions Design

## Summary

This change turns `gateway` into the primary runtime entrypoint for the Rust assistant by starting the Web console alongside the existing channel manager and agent loop.

The Web console stops being a Web-only chat surface and becomes a grouped session browser over the shared Rust session store. It can view sessions from `web`, `telegram`, `wecom`, `cli`, and `system`, but only `web` sessions remain writable inside the browser. If a user wants to continue a non-Web session from the browser, they must duplicate that session into a new `web` session and continue from the copy.

## Goals

- Start the Web console automatically when `gateway` starts.
- Let the Web console browse sessions from all Rust channels, grouped by channel.
- Keep non-Web channel sessions read-only in the browser.
- Support duplicating any non-Web session into a new writable `web` session.
- Duplicate full session history and preserve the active profile.
- Preserve the existing `web` command as a standalone entrypoint with the same grouped session browser behavior.

## Non-Goals

- No direct browser sending into `telegram`, `wecom`, `cli`, or `system` sessions.
- No bi-directional synchronization of browser-authored user messages back into external channels.
- No cross-session merge UI or session lineage graph beyond basic source metadata.
- No change to the existing external channel protocol semantics.
- No attempt to turn the Web console itself into a `Channel` implementation.

## Chosen Approach

### 1. `gateway` becomes the integrated runtime

`gateway` will launch three pieces together:

- `AgentLoop`
- `ChannelManager`
- embedded Web server

All three share the same Rust config, workspace, and session store root.

### 2. Web becomes a grouped session browser over the shared store

The Web backend no longer assumes all visible sessions live under `web:`. Instead, it enumerates all known session namespaces and returns grouped results for the browser to render.

Groups are channel-based, for example:

- `web`
- `telegram`
- `wecom`
- `cli`
- `system`

### 3. Browser writes remain limited to `web` sessions

This is a hard rule.

- `web` sessions are writable in the browser
- all non-`web` sessions are read-only in the browser

The browser can inspect any session, but it cannot continue a non-Web session directly.

### 4. Continuing an external session requires duplication

When the user views a non-Web session and wants to continue it from the browser, the browser calls a duplication API:

- copy full message history
- copy `active_profile`
- create a new `web:<uuid>` session
- store source metadata pointing back to the original session

After duplication, the browser switches to the new Web session and resumes normal browser-side sending there.

## Alternatives Considered

### A. Keep `web` separate from `gateway`

Rejected because it does not satisfy the requirement that `gateway` startup should also expose the Web console.

### B. Let the browser send directly into external channel sessions

Rejected because it creates confused session ownership and message-source semantics. A browser-originated user message would appear in a Telegram or WeCom session without having come from that external user, which is operationally misleading.

### C. Make the Web console a `Channel`

Rejected because the Web console is a control surface and browser server, not a natural fit for the existing inbound/outbound channel abstraction. Forcing it into `Channel` would make grouped browsing and duplication semantics harder, not easier.

## Detailed Design

### Runtime model

`gateway` changes from:

- start `AgentLoop`
- start `ChannelManager`

to:

- start `AgentLoop`
- start `ChannelManager`
- start `web::serve(...)`

The embedded Web server uses the same `AgentLoop` instance and same session store as the other channels.

The standalone `web` command remains available. It should expose the same grouped cross-channel browser behavior, but without also starting the external channel manager.

### CLI changes

`Gateway` gains Web bind options:

- `--web-host`, default `127.0.0.1`
- `--web-port`, default `3000`

There is no `--disable-web` in this slice. The requested behavior is that `gateway` starts the Web console by default.

The `web` command keeps its own existing `--host` and `--port`.

### Session model changes

The session store needs to support:

- listing sessions across all namespaces
- grouping them by channel namespace
- duplicating one session into a new `web` session

Session metadata gains:

- `source_session_key: Option<String>`

This records the origin when a new Web session is created by duplication.

Duplication rules:

- source session may be any namespace
- destination is always `web:<uuid>`
- copy full `messages`
- copy `active_profile`
- set fresh `created_at` and `updated_at`
- set `source_session_key` to the original session key
- reset `last_consolidated` to match the copied session state if the copied session already had consolidated boundaries; do not drop copied messages

### Web read/write rules

Each session returned by the Web API includes capability flags derived from its channel:

- `readOnly`
- `canSend`
- `canDuplicate`

Rules:

- `web`
  - `readOnly = false`
  - `canSend = true`
  - `canDuplicate = false`
- non-`web`
  - `readOnly = true`
  - `canSend = false`
  - `canDuplicate = true`

The Web backend must reject attempts to `POST /api/chat` against non-Web sessions even if the browser tries anyway.

### Web API changes

#### `GET /api/sessions`

Returns channel-grouped sessions:

```json
{
  "groups": [
    {
      "channel": "web",
      "label": "Web",
      "sessions": [
        {
          "sessionId": "abc",
          "channel": "web",
          "updatedAt": "2026-03-22T12:00:00Z",
          "activeProfile": "openai:gpt-4.1-mini",
          "preview": "Latest preview",
          "readOnly": false,
          "canSend": true,
          "canDuplicate": false
        }
      ]
    }
  ]
}
```

`preview` is still derived from the latest user or assistant text message and truncated for UI display.

#### `GET /api/sessions/{channel}/{id}`

Returns one session detail view:

- `sessionId`
- `channel`
- `updatedAt`
- `activeProfile`
- `messages`
- `readOnly`
- `canSend`
- `canDuplicate`
- `sourceSessionKey` when present

The route is channel-aware so the backend does not need to guess between `web:abc` and `telegram:abc`.

#### `POST /api/sessions`

Still creates a new writable `web` session.

This remains the browser’s “new chat” entrypoint.

#### `POST /api/sessions/duplicate`

Request:

```json
{
  "channel": "telegram",
  "sessionId": "12345"
}
```

Response:

- the newly created Web session detail, not just summary

Returning detail lets the browser switch immediately without a second fetch.

#### `POST /api/chat`

Request shape stays browser-focused, but it becomes channel-aware:

- `channel`
- `sessionId`
- `message`

Rules:

- if `channel != "web"`, reject with `400`
- if `channel == "web"`, proceed as today

Response still includes:

- `reply`
- `replyHtml`
- `sessionId`
- `activeProfile`

and now also includes:

- `channel`

This makes the browser-side state transition explicit.

### Web UI changes

The Web UI changes from a Web-only chat client into a grouped session browser.

#### Session rail

The left rail becomes channel-grouped:

- one section per channel
- sessions within each section sorted newest-first
- each row shows preview + active profile

#### Session detail view

When a session is selected:

- transcript loads from the backend detail endpoint
- current channel and active profile are shown
- if the session is writable (`web`), composer remains enabled
- if the session is read-only, composer is disabled

#### Read-only external sessions

For `telegram`, `wecom`, `cli`, and `system` sessions:

- transcript is readable
- composer is disabled
- a `Duplicate to Web` action is shown

#### Duplication flow

When the user clicks `Duplicate to Web`:

1. call `POST /api/sessions/duplicate`
2. receive the new Web session detail
3. refresh grouped session list
4. switch UI selection to the new Web session
5. enable composer and continue in that copied session

#### Boot behavior

On first load:

- fetch grouped sessions
- restore the stored selected session if it still exists with the same `channel + sessionId`
- otherwise select the most recent writable Web session if one exists
- if no Web sessions exist, create a new Web session

Persisted browser selection therefore becomes:

- `selectedChannel`
- `selectedSessionId`

not just a single Web session id.

### Error handling

- invalid `channel/id` pairs return clear `400`
- missing session returns `404`
- attempting to send to non-Web session returns `400` with a message explaining that duplication is required
- duplicate-from-missing session returns `404`
- browser should leave the current selection unchanged if duplicate or detail fetch fails

### Compatibility notes

- Existing `web:` sessions remain valid.
- Existing non-Web sessions become visible in the browser without migration.
- Existing browser-local stored Web-only selection may not include channel. On first boot after this change, the UI should treat the stored value as a legacy Web session ID and try `channel = "web"` first before falling back to normal selection logic.

## Testing Strategy

### Session store tests

- list sessions across multiple namespaces
- group names/namespaces are derived correctly
- duplicate copies full history
- duplicate preserves `active_profile`
- duplicate records `source_session_key`
- duplicate creates a new `web:` destination key

### Web server tests

- `GET /api/sessions` returns grouped results across channels
- `GET /api/sessions/{channel}/{id}` returns channel-aware detail
- `POST /api/chat` rejects non-Web sessions
- `POST /api/sessions/duplicate` creates a writable Web copy
- duplicate response includes copied messages, active profile, and source metadata

### Web page tests

- session rail renders grouped channel sections
- read-only non-Web sessions disable composer
- `Duplicate to Web` is visible only for non-Web sessions
- duplication switches selection to new Web session
- legacy stored Web-only session selection still restores correctly

### Gateway integration tests

- `gateway` starts the Web server alongside channels
- configured `--web-host` / `--web-port` bind correctly
- startup still works when Telegram and WeCom are enabled

## Risks And Mitigations

### Risk: confusing session identity between `channel:id` and raw session keys

Mitigation:

- make all Web detail/send APIs explicit about `channel` and `sessionId`
- keep raw internal keys server-side only

### Risk: browser accidentally mutates non-Web sessions

Mitigation:

- enforce the rule in backend validation, not only the UI
- return capability flags in list/detail payloads

### Risk: duplication creates hidden divergence from the source session

Mitigation:

- make duplication explicit in the UI
- persist `sourceSessionKey`
- never silently continue the original non-Web session in-place

## Implementation Notes

This is a single coherent slice and should be planned as:

1. session-store + API contract changes
2. gateway embedded Web startup
3. Web page grouped browser and duplication flow

The Rust runtime remains the authority for all behavior in this design.
