# Web Session Auto-Continue Design

Date: 2026-04-03

## Summary

The embedded web UI currently treats every non-`web` session as strictly read-only. Users can inspect `telegram`, `wecom`, `weixin`, `cli`, and `system` sessions, but they must click `Duplicate to Web` before they can continue the conversation from the browser.

This change keeps that ownership model intact while removing the manual duplication step. When a user sends a message from a non-`web` session in the browser, the backend will automatically duplicate the selected session into a new writable `web` session, deliver the message there, and return the new `web` session identity to the browser. The original session remains read-only and unchanged.

## Goals

- Preserve the existing rule that only `web` sessions are browser-writable.
- Remove the need to manually duplicate a non-`web` session before sending the first browser-authored message.
- Keep session lineage explicit by preserving `sourceSessionKey` on the auto-created `web` session.
- Make the browser feel like it can "continue" an external session without mutating the original channel history.
- Update README and smoke documentation so the new browser behavior is accurate and discoverable.

## Non-Goals

- Do not send browser-authored messages back into `telegram`, `wecom`, `weixin`, `cli`, or `system` sessions directly.
- Do not add cross-session merge, history rewrite, or bidirectional synchronization.
- Do not remove the explicit duplication API or the `Duplicate to Web` button.
- Do not redesign session grouping, source metadata, or channel semantics.

## Chosen Approach

### 1. Keep non-`web` sessions logically read-only

The browser session model does not change at the storage layer:

- original non-`web` sessions stay read-only
- `web` sessions remain the only writable browser sessions
- duplicated `web` sessions keep `sourceSessionKey`

The change is only in how the browser transitions from inspection to continuation.

### 2. Auto-duplicate inside `POST /api/chat`

`POST /api/chat` will stop rejecting non-`web` sessions. Instead:

1. validate `channel`, `sessionId`, and message as today
2. if `channel == "web"`, behave exactly as today
3. if `channel != "web"`, duplicate `{channel}:{sessionId}` into a new `web:<uuid>` session
4. send the new user message to that new `web` session
5. return the assistant reply plus the new `web` session identity

This keeps the transition atomic from the browser's perspective. The browser does not need to call `POST /api/sessions/duplicate` first just to send a message.

### 3. Let the composer stay usable for duplicable sessions

The browser should no longer block submission solely because a session is marked `readOnly`.

Instead, the UI behavior becomes:

- writable `web` session:
  - composer enabled
  - send behaves as today
- duplicable non-`web` session:
  - composer enabled
  - first browser send transparently continues in a new `web` session
- truly unsendable session:
  - composer disabled

The explicit `Duplicate to Web` button remains available for users who want to fork the session before typing.

### 4. Surface the transition clearly enough, but quietly

The UI should not present this as an error path. A small informational status after send is sufficient, for example that the conversation continued in Web.

This avoids surprising users who thought they were still editing the original channel session, while still keeping the flow lightweight.

## API and Data Contract Changes

## `POST /api/chat`

The response shape stays the same:

- `reply`
- `replyHtml`
- `persisted`
- `channel`
- `sessionId`
- `activeProfile`

But for non-`web` requests, the returned `channel` and `sessionId` will point at the new `web` session rather than the originally requested external session.

No new endpoint is required.

## Session duplication

Reuse the existing session-store duplication path:

- copy full message history
- copy `active_profile`
- preserve `last_consolidated`
- set `source_session_key`
- create a fresh `web:<uuid>` session

No new session metadata is needed.

## Frontend Behavior

The current frontend uses `readOnly` to disable the textarea and to reject submit before it reaches the API. That becomes too strict once auto-duplication exists.

The updated browser rules should be:

- if `canSend`, allow normal send
- else if `canDuplicate`, allow submit and rely on API auto-duplication
- else disable composer

After a successful non-`web` send:

- refresh the session list
- select the returned `web` session
- keep the draft empty because the send already succeeded

The session detail fetched after send will then show the writable `web` copy with `sourceSessionKey` pointing back to the original external session.

## Documentation Changes

### README

Update the web behavior description to say:

- the browser can inspect grouped sessions across channels
- non-`web` sessions remain read-only as stored sessions
- sending from a non-`web` session in the browser automatically continues the conversation in a new `web` copy
- manual duplication is still available

Also add the already-implemented operator capabilities that the current README does not describe well enough:

- `sidekick status`
- `sidekick onboard --wizard`
- `channels status`
- `provider login codex`
- durable `memory/MEMORY.md` and `memory/HISTORY.md`
- `Skills` management in the embedded web UI

### Smoke checklist

Update the runbook so browser smoke steps match the new continuation behavior:

- selecting a non-`web` session no longer requires a manual duplicate before sending
- the first browser-authored reply should switch into a `web` session
- the original external session remains unchanged

## Testing Strategy

### Backend

Add regression coverage that proves:

- `POST /api/chat` no longer rejects duplicable non-`web` sessions
- the first send returns a `web` session id with copied history plus the new turn
- the original external session remains unchanged
- existing `POST /api/sessions/duplicate` behavior still works

### Frontend shell

Update shell assertions to prove:

- read-only-but-duplicable sessions do not disable the composer
- the page still exposes the explicit `Duplicate to Web` control
- the browser flow switches to the returned session after auto-continue

### Full verification

Run the repository-level verification the README already documents:

- `cargo test`
- `cd frontend && npm test -- --run`
- `cd frontend && npm run build`

## Acceptance Criteria

- A browser user can open a `telegram`, `wecom`, `weixin`, `cli`, or `system` session and send a message without first clicking `Duplicate to Web`.
- That send always continues in a new `web` session rather than writing into the original channel session.
- The original non-`web` session history is unchanged.
- Existing explicit duplication still works.
- README and smoke docs match the implemented behavior and current Sidekick capabilities.
