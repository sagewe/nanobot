# Session Parallelism and Message Debounce Design

Date: 2026-03-25

## Goal

Replace the current global agent processing bottleneck with session-scoped concurrency, make outbound channel delivery concurrent, and add session-level burst message compression so short clusters of messages are merged into a single model turn.

## Context

Current behavior:

- Enabled channels start concurrently.
- Inbound channel traffic eventually reaches `AgentLoop`.
- `AgentLoop` currently uses one global `processing_lock`, so external channel traffic is effectively serialized across all sessions.
- `ChannelManager` currently consumes outbound messages from one loop and calls `channel.send()` serially.
- Web direct requests are more concurrent than bus-driven channel traffic because they do not go through the same global lock path.

The intended direction is:

- Different sessions should process in parallel.
- The same session should remain strictly serial.
- Outbound sending should no longer be globally serialized.
- Future `btw` support must remain possible:
  - a long-running task is not interrupted
  - a side response may later be added without contaminating the main session history
- Burst compression should happen per session, not per channel globally.

## Non-Goals

This change does not implement:

- `btw` side-lane replies
- message summarization via an LLM
- media compression or attachment coalescing
- channel-specific send ordering policies
- profile-specific debounce settings

## User-Visible Behavior

### 1. Session-Scoped Parallelism

- Messages from different `session_key`s can be processed at the same time.
- Messages from the same `session_key` remain strictly ordered.
- `/stop` still applies only to the current session.

### 2. Outbound Fan-Out

- A slow send on one channel must not block sends to other channels.
- The outbound queue remains a single intake point, but actual `send()` execution becomes concurrent.

### 3. Session-Level Message Debounce

- Debounce key is `session_key`, never just the channel name.
- Different chats within the same channel are never merged.
- Within a configured debounce window, multiple user messages for the same session are merged into one model turn.
- The merged turn is persisted as a single user message in session history.
- Messages outside the window remain separate turns.

## Compression Format

When multiple user messages are merged, the resulting user content should be formatted as:

```text
[Compressed user burst]
1. First message
2. Second message
3. Third message
```

This preserves message boundaries and makes the merge explicit to the model.

## Locking Model

### Current Problem

The current `processing_lock: Arc<Mutex<()>>` in `AgentLoop` serializes all bus-driven dispatches across all sessions.

### New Model

Replace the global lock with a session lock table:

- `session_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>`

Dispatch behavior:

1. Derive `session_key`
2. Get or create that session’s lock
3. Lock only that session
4. Build tools and process the message

Result:

- Same session stays serial
- Different sessions can progress independently

## Ingress Debounce Model

### Placement

Debounce happens before a message is processed by the main agent flow.

It should not be implemented as provider-side prompt manipulation.

### SessionIngressBuffer

Introduce a session-scoped ingress buffer component responsible for:

- collecting pending inbound messages per `session_key`
- arming a debounce timer
- emitting exactly one merged inbound message when the timer expires

### Command Bypass

Slash commands must not be merged into ordinary user bursts.

The ingress layer should explicitly bypass debounce for control messages such as:

- `/stop`
- `/new`
- `/help`
- `/models`
- `/model ...`

Behavior:

- a control command is dispatched immediately
- pending ordinary text for the same session must not absorb the command
- if needed, a pending burst should be flushed or left queued explicitly, but command semantics must remain immediate and unchanged

Command priority for the first version:

- `/stop`
  - bypasses debounce
  - preempts the current session task
  - clears any pending buffered burst for that same session
- `/new`, `/help`, `/models`, `/model ...`
  - bypass debounce
  - do not merge into ordinary text
  - do not preempt an already-running main task
  - execute after the current same-session task finishes

### Semantics

- If `messageDebounceMs == 0`, behavior remains immediate.
- If `messageDebounceMs > 0`:
  - first message for a session starts a timer
  - later messages within the window are appended to the same burst
  - when the window closes, one merged inbound message is dispatched

### Interaction with Running Tasks

The first version keeps the main session lane serial:

- if a session is already being processed, later inbound bursts wait behind that session’s lock
- they may still be merged among themselves in the ingress buffer

This preserves correctness and leaves room for a future `btw` side lane.

## Configuration

Add one new agent default setting:

- `agents.defaults.messageDebounceMs`

Behavior:

- default `0`
- `0` means disabled
- any positive integer enables session-level debounce in milliseconds

This is intentionally global at first:

- same semantics across Telegram, WeCom, Weixin, CLI, and bus-driven web/session traffic
- no per-channel or per-profile config in this slice

## Outbound Dispatch Model

### Current Problem

`ChannelManager` consumes outbound messages and executes `channel.send(msg).await` serially in one loop.

### New Model

Keep one outbound consumer loop, but replace global serialization with delivery-key workers.

Definitions:

- `delivery_key = "{channel}:{chat_id}"`

Behavior:

1. `consume_outbound()`
2. derive `delivery_key`
3. route the outbound message into that key’s worker queue
4. each worker sends messages for its own key in FIFO order
5. different delivery keys can progress concurrently

This preserves per-destination ordering while removing the global cross-channel bottleneck.

### Worker Lifecycle and Backpressure

The implementation should not detach unbounded anonymous send tasks.

Instead:

- maintain a tracked worker table keyed by `delivery_key`
- each worker owns a bounded queue
- `stop_all()` must stop/abort these workers together with the dispatch loop
- if a worker queue is full, intake must not block unrelated delivery keys; the implementation should fail that enqueue explicitly and log/drop the affected outbound message rather than head-of-line blocking the global intake loop
- idle workers should retire and remove themselves from the tracked table after their queue drains and they remain idle for a short timeout

This gives:

- bounded fan-out
- defined shutdown behavior
- preserved FIFO ordering for each destination
- concurrency across unrelated destinations

## Future Compatibility with `btw`

This design intentionally preserves a path for future `btw` support:

- main session lane remains serial
- ingress buffering stays per session
- future side-lane replies can consume a stable snapshot without writing into main session history

Nothing in this design should require future `btw` replies to reuse the main session lock.

## Files to Modify

- `/Users/sage/nanobot/nanobot-rs/src/config/mod.rs`
  - add `message_debounce_ms`
- `/Users/sage/nanobot/nanobot-rs/src/agent/mod.rs`
  - replace global processing lock with session lock table
  - add session ingress buffering/debounce
- `/Users/sage/nanobot/nanobot-rs/src/channels/mod.rs`
  - change outbound dispatch to concurrent fan-out
- `/Users/sage/nanobot/nanobot-rs/tests/agent.rs`
  - add session concurrency and debounce tests
- `/Users/sage/nanobot/nanobot-rs/tests/channels.rs`
  - add outbound concurrency tests

## Test Requirements

### Agent

1. Different sessions can process concurrently
2. Same session remains serial
3. Same-session burst within window is merged into one provider call
4. Same-session messages outside the window become separate provider calls
5. Different sessions are never merged together
6. Session history stores the merged burst as one user turn

### Channels

1. A slow outbound send on one delivery key does not block another delivery key
2. Messages for the same `channel:chat_id` remain FIFO
2. Existing Telegram/WeCom/Weixin send behavior does not regress

## Acceptance Criteria

- Removing the global agent bottleneck does not break same-session ordering
- External channels can make progress concurrently across different sessions
- Outbound sending no longer serializes all channels behind the slowest destination, while preserving FIFO per `channel:chat_id`
- Bursty short-message input within one session becomes a single model turn when debounce is enabled
- Full test suite remains green
