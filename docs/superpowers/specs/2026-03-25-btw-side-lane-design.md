# BTW Side-Lane Design

Date: 2026-03-25

## Goal

Add an explicit `/btw <question>` command that lets the user ask a side question while the main task for the same session keeps running, with the side reply using a stable session snapshot and never contaminating main-session history.

## Context

The current runtime now supports:

- session-scoped serialization for the main lane
- concurrent processing across different sessions
- session-level message debounce
- concurrent outbound delivery with per-destination FIFO

The next capability is a controlled side lane:

- same-session main work must not be interrupted
- the user must be able to ask a one-off question explicitly
- the side reply must use existing stable context
- the side reply must not alter the main session history or tool continuation state

This is not a general interruption system and is not natural-language-triggered. It is an explicit command.

## User-Facing Behavior

### Command Shape

The trigger is a single explicit command:

```text
/btw <question>
```

Examples:

```text
/btw summarize what we know so far
/btw what file are you editing right now
```

There is no modal `/btw` state. Each `/btw` command is a one-off side request.

### Activation Rule

- If the current session has an active main task, `/btw ...` starts a side-lane reply.
- If the current session does not have an active main task, the command does not silently fall back to a normal message.
- Instead, it returns a clear response such as:

```text
No active task is running in this session. Send a normal message instead.
```

### Channel Scope

`/btw` should behave consistently across all currently supported channels:

- CLI
- Web
- Telegram
- WeCom
- Weixin

The output format remains normal channel output. No special per-channel protocol is required for the first version.

## Core Semantics

### 1. Main Lane Is Not Interrupted

The currently running main task for a session keeps its existing execution path:

- same tool loop
- same session lock
- same history writes
- same progress behavior

`/btw` must never cancel, pause, or mutate that main lane.

### 2. BTW Uses a Stable Snapshot

The side lane reads the session’s latest stable persisted snapshot:

- saved session history from `SessionStore`
- current `active_profile`

It does not read:

- in-memory assistant/tool messages from the currently running main task
- pending debounced bursts
- unsaved intermediate progress

This is the core guarantee that keeps `/btw` from polluting or depending on unstable main-lane state.

### 3. BTW Does Not Write Main History

The following must not be appended to the main session history:

- the `/btw ...` user input
- the `/btw` assistant reply
- any side-lane tool-call traces

The side lane is intentionally ephemeral.

### 4. BTW May Use Tools

The first version should still allow tool usage inside the side lane.

Reasoning:

- a tool-less `/btw` lane would be too weak in practice
- it can remain isolated by using a fresh `ToolRegistry`
- side-lane tools should reply directly to the caller and avoid main-session persistence

## Architectural Model

## Main Lane vs BTW Lane

Each session now has two conceptual execution classes:

- `main`
- `btw`

### Main Lane

- existing message path
- subject to session serialization
- writes session history
- sets session-scoped progress/tool hints

### BTW Lane

- created only by `/btw ...`
- not subject to the main lane’s session lock
- reads a stable snapshot
- sends a direct reply
- does not write session history

This means the same session may have:

- one active `main` task
- zero or one active `btw` task

The design intentionally keeps these separate so future scheduling and cancellation rules remain clear.

## Runtime State Additions

The minimal runtime state needed is:

- `main_task_running: HashSet<String>` or equivalent keyed by `session_key`
- `main_task_generation: HashMap<String, u64>` or equivalent keyed by `session_key`
- `btw_tasks: HashMap<String, Vec<JoinHandle<()>>>` or equivalent keyed by `session_key`

The purpose is not to create a generalized scheduler yet. It is only to answer:

- is a main task active for this session?
- which exact main-task generation is active for this session?
- what side-lane tasks should `/stop` cancel?

### Atomic BTW Activation

`/btw` activation must bind to a specific main-task generation atomically.

The runtime should not:

1. check “is a main task running?”
2. then independently load a snapshot later

because the main lane can finish or restart in between.

Instead, the runtime should perform one session-scoped control read that yields:

- whether a main task is active
- the active main-task generation
- whether a `btw` task is already active

The side lane is then bound to that generation.

If the main generation changes before the side-lane task actually starts, the side lane should abort with a normal user-visible response instead of running against the wrong main task.

The “check active main task / check active btw task / reserve btw slot” sequence must be atomic for a given session. Two concurrent `/btw` requests for the same session must not both pass admission.

## Command Handling

### `/btw ...`

The ingress path should treat `/btw ...` similarly to other immediate control commands:

- bypass debounce
- dispatch immediately
- never merge into ordinary user bursts

Unlike `/help` or `/models`, `/btw ...` must not run inline on the main lane. It should create a dedicated side-lane task.

### `/stop`

For the first version:

- `/stop` still applies to the current session
- it cancels the active main task for that session
- it also cancels any active `btw` tasks for that session
- it clears pending debounced bursts for that session

This keeps stop semantics easy to explain:

- one session
- one stop command
- all active work for that session is cancelled

No dedicated `/btw-stop` command is needed in this slice.

### BTW Concurrency Limit

The first version should allow at most one active `btw` task per session.

If another `/btw ...` arrives while a `btw` task is already active for that same session, return a clear response such as:

```text
A BTW reply is already running for this session. Wait for it to finish or stop the session.
```

This keeps the first version bounded and removes ambiguity around late side replies.

## Processing Flow

For a session with an active main task:

1. User sends `/btw what are you doing?`
2. Runtime atomically reads the session control state, captures the active main-task generation, and reserves the session’s single `btw` slot
3. Runtime loads the current saved session snapshot
4. Runtime builds a one-off provider request from that snapshot plus the `/btw` question
5. Runtime creates a fresh `ToolRegistry` for the side lane
6. Runtime runs a provider/tool loop without writing main-session history
7. Runtime sends the final reply back through the originating channel
8. Runtime releases the session’s `btw` slot

If no main task is active:

1. User sends `/btw ...`
2. Runtime returns the explicit “no active task” response
3. No side-lane task is created

## Message Construction

The `/btw` provider input should be explicit that this is a side question, without pretending it is part of main history.

Recommended shape for the synthetic user input:

```text
[BTW side question]
<question>
```

This makes the side-lane intent clear to the model and helps future debugging.

The main system prompt should remain unchanged. The side lane uses the same system prompt and same current profile as the session snapshot.

## Interaction with Session Debounce

`/btw ...` must bypass the session debounce buffer completely.

Rationale:

- it is an explicit control command
- delaying it behind debounce defeats the purpose of a side question
- it must never merge into ordinary user bursts

Ordinary user messages still follow existing debounce rules.

## Error Handling

### No Active Main Task

Return a normal user-visible response, not an internal error.

### Snapshot or Profile Read Failure

If the runtime cannot load the session snapshot or cannot resolve the active profile for the side lane:

- return a normal user-visible error reply for the `/btw` request
- do not start the side lane
- do not affect the main task

This includes cases such as:

- `SessionStore` read failure
- missing session snapshot
- invalid or missing `active_profile`

### BTW Provider Failure

Return a normal user-visible error reply for the side lane only.

Do not:

- cancel the main lane
- mutate the main session

### BTW Tool Failure

Treat tool failure exactly like other provider/tool-loop failures in the current runtime: it is contained to the requesting lane and returned as part of that side reply flow.

## Non-Goals

This slice does not implement:

- natural-language auto-detection of side questions
- `/btw` UI affordances in Web
- dedicated `/btw` history or transcript branch visualization
- reading in-memory unsaved state from the active main task
- lane prioritization or fairness controls
- message compression inside the btw lane
- independent `btw` cancellation commands

## Files to Modify

- `/Users/sage/nanobot/nanobot-rs/src/agent/mod.rs`
  - recognize `/btw ...`
  - track main-lane activity by session
  - track btw tasks by session
  - add a `process_btw(...)` path
- `/Users/sage/nanobot/nanobot-rs/tests/agent.rs`
  - add btw-lane coverage

No channel-specific protocol changes are required for the first version because channels only need to deliver the side reply as a normal outbound message.

## Test Requirements

### Core BTW Behavior

1. `/btw ...` while a main task is active returns a side reply
2. `/btw ...` does not block behind the main session lock
3. `/btw ...` does not append user or assistant turns to main session history
4. `/btw ...` sees the current persisted `active_profile`
5. `/btw ...` cannot see unsaved in-flight main-lane messages

### Command Semantics

1. `/btw ...` with no active main task returns the explicit “no active task” reply
2. `/btw ...` bypasses debounce and does not merge into a user burst
3. `/stop` cancels both main and btw tasks for the session

### Isolation

1. A second `/btw ...` for the same session is rejected while one is already active
2. A btw failure does not affect the main task
3. A main task completion does not retroactively write btw messages into session history
4. A btw task bound to an outdated main-task generation is rejected instead of running against the wrong main flow
