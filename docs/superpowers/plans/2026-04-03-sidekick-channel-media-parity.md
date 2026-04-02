# Sidekick Channel and Media Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the existing Rust channels closer to the Python behavior already in this repo by fixing the message-tool media contract first, then improving Telegram reply/media context, WeCom non-text inbound handling, Feishu real media handling, and a scoped Weixin expansion without adding any new channels.

**Architecture:** Keep this batch inside the current Rust tool/channel files instead of introducing a new shared abstraction layer. The first change is the message tool contract: preserve outbound media and inbound metadata through `ToolContext` so channels can see the same reply context the agent already knows. After that, tighten each channel in place with the smallest viable parity step: Telegram learns to preserve thread/reply context and inbound media, WeCom accepts non-text callbacks, Feishu stops emitting placeholders for media and uploads attachments, and Weixin expands only where the current protocol shape is already visible.

**Tech Stack:** Rust, Tokio, reqwest, serde_json, tracing, existing integration tests, Python parity references

---

## File Map

**Read before changing**
- `<repo-root>/src/bus/mod.rs` - `InboundMessage` / `OutboundMessage` shape, especially the fact that `media` does not exist yet and must be added deliberately.
- `<repo-root>/src/config/mod.rs` - current channel config defaults and already-existing knobs like `replyToMessage`.
- `<repo-root>/channels/telegram.py` - parity target for Telegram reply/thread/media handling.
- `<repo-root>/channels/wecom.py` - parity target for WeCom non-text inbound handling.
- `<repo-root>/channels/feishu.py` - parity target for Feishu inbound media download and outbound media upload.
- `<repo-root>/agent/tools/message.py` - parity target for message tool `media` support.
- `<repo-root>/channels/base.py` - parity target for how channel context is passed around in Python.

**Modify**
- `<repo-root>/src/bus/mod.rs`
- `<repo-root>/src/agent/mod.rs`
- `<repo-root>/src/tools/mod.rs`
- `<repo-root>/src/channels/mod.rs`
- `<repo-root>/src/channels/wecom.rs`
- `<repo-root>/src/channels/feishu.rs`
- `<repo-root>/src/channels/weixin.rs`
- `<repo-root>/tests/agent.rs`
- `<repo-root>/tests/tools.rs`
- `<repo-root>/tests/channels.rs`
- `<repo-root>/tests/wecom.rs`
- `<repo-root>/tests/feishu.rs`
- `<repo-root>/tests/weixin.rs`

### Task 1: Preserve Media And Reply Context Through The Message Tool

**Files:**
- Modify: `<repo-root>/src/bus/mod.rs`
- Modify: `<repo-root>/src/agent/mod.rs`
- Modify: `<repo-root>/src/tools/mod.rs`
- Modify: `<repo-root>/tests/agent.rs`
- Modify: `<repo-root>/tests/tools.rs`

- [ ] **Step 1: Write the failing tests**

Add two focused regressions:

```rust
#[tokio::test]
async fn message_tool_schema_includes_media() {
    // assert the tool schema exposes a media array
}

#[tokio::test]
async fn message_tool_forwards_media_and_context_metadata() {
    // assert media and inbound metadata survive into OutboundMessage
}
```

The `tests/agent.rs` case should drive the full agent path so the test proves `AgentLoop -> ToolContext -> MessageTool -> OutboundMessage` preserves `media` and context metadata such as `message_id` and any Telegram-style thread metadata stored on the inbound message.

- [ ] **Step 2: Run the targeted tests to verify RED**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test tools message_tool_schema_includes_media
cargo test --target-dir /tmp/sidekick-target --test agent message_tool_forwards_media_and_context_metadata
```

Expected: FAIL because the bus envelope does not carry `media` yet, `MessageTool` does not expose `media` as a first-class schema field, and `ToolContext` does not preserve inbound metadata end-to-end.

- [ ] **Step 3: Implement the minimal tool-context pass-through**

In `src/bus/mod.rs`:
- Add `media: Vec<String>` to both `InboundMessage` and `OutboundMessage`.
- Use `#[serde(default)]` so existing fixtures and persisted payloads still deserialize cleanly.
- Do not redesign the bus; this is a narrow shape extension only.

In `src/agent/mod.rs`:
- Extend every `tools.set_context(...)` call to pass a cloned copy of the inbound `msg.metadata`.
- Keep the current session and reply behavior unchanged; this is only a metadata carrier change.

In `src/tools/mod.rs`:
- Add a `metadata: HashMap<String, Value>` field to `ToolContext`.
- Update `MessageTool::schema()` so `media` is a first-class argument instead of a loose extra.
- Update `MessageTool::execute()` to:
  - forward `media` into `OutboundMessage.media`
  - merge `ToolContext.metadata` into the outbound metadata
  - keep `message_id` in metadata so existing channel reply logic still works

Do not add new tool types or another transport abstraction.

- [ ] **Step 4: Re-run the targeted tests**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test tools message_tool_schema_includes_media
cargo test --target-dir /tmp/sidekick-target --test agent message_tool_forwards_media_and_context_metadata
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add <repo-root>/src/bus/mod.rs <repo-root>/src/agent/mod.rs <repo-root>/src/tools/mod.rs <repo-root>/tests/agent.rs <repo-root>/tests/tools.rs
git commit -m "feat: preserve message tool media context"
```

### Task 2: Add Telegram Reply, Thread, And Media Parity

**Files:**
- Modify: `<repo-root>/src/channels/mod.rs`
- Modify: `<repo-root>/tests/channels.rs`

- [ ] **Step 1: Write the failing Telegram tests**

Add regression coverage for:

```rust
#[tokio::test]
async fn telegram_channel_publishes_media_and_reply_context() {
    // inbound photo/document/reply_to_message should populate content, media, metadata, and session key
}

#[tokio::test]
async fn telegram_channel_sends_attachments_and_thread_reply_metadata() {
    // outbound msg.media + metadata["message_id"] + metadata["message_thread_id"]
}
```

The test fixture in `tests/channels.rs` will need extra Telegram Bot API routes for `getFile`, `sendPhoto`, `sendVoice`, `sendAudio`, and `sendDocument`, because the current harness only covers `getUpdates` and `sendMessage`.

- [ ] **Step 2: Run the targeted tests to verify RED**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test channels telegram_channel_publishes_media_and_reply_context
cargo test --target-dir /tmp/sidekick-target --test channels telegram_channel_sends_attachments_and_thread_reply_metadata
```

Expected: FAIL because the Rust Telegram channel currently only reads `text` on inbound messages and does not preserve the richer reply/thread context from the payload.

- [ ] **Step 3: Implement the minimal Telegram parity**

In `src/channels/mod.rs`:
- Expand inbound parsing to accept the same message shapes the Python channel already handles: text, photo, voice, audio, and documents.
- Download inbound media to the Telegram media directory and place the local file paths in `InboundMessage.media`.
- Capture `message_id`, `reply_to_message_id`, `message_thread_id`, `media_group_id`, `username`, and forum flags in metadata.
- Set `session_key_override` for threaded forum chats so replies keep the topic-scoped session.
- Extend `send()` to dispatch `sendPhoto`, `sendVoice`, `sendAudio`, or `sendDocument` when `msg.media` is present, and preserve `reply_to_message_id` plus `message_thread_id` on both media and text sends.
- Keep plain-text sends on the current `sendMessage` path when no attachments are present.

Do not add a new Telegram abstraction or switch to a different SDK.

- [ ] **Step 4: Re-run the targeted tests**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test channels telegram_channel_publishes_media_and_reply_context
cargo test --target-dir /tmp/sidekick-target --test channels telegram_channel_sends_attachments_and_thread_reply_metadata
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add <repo-root>/src/channels/mod.rs <repo-root>/tests/channels.rs
git commit -m "feat: add telegram media and reply parity"
```

### Task 3: Accept Non-Text WeCom Inbound Callbacks

**Files:**
- Modify: `<repo-root>/src/channels/wecom.rs`
- Modify: `<repo-root>/tests/wecom.rs`

- [ ] **Step 1: Write the failing WeCom tests**

Add tests that prove the Rust channel stops dropping all non-text callbacks:

```rust
#[test]
fn parse_wecom_callback_accepts_non_text_message_types() {
    // image/file/voice/mixed callbacks still parse req_id, sender, chat, and msg type
}

#[tokio::test]
async fn wecom_channel_publishes_non_text_inbound_messages() {
    // non-text callback should reach the bus instead of being ignored
}
```

Keep the assertions focused on current behavior: the callback should be accepted, the reply context should be updated, and the bus should receive a usable placeholder/content summary rather than nothing.

- [ ] **Step 2: Run the targeted tests to verify RED**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test wecom parse_wecom_callback_accepts_non_text_message_types
cargo test --target-dir /tmp/sidekick-target --test wecom wecom_channel_publishes_non_text_inbound_messages
```

Expected: FAIL because the current parser only admits `msgtype == "text"`.

- [ ] **Step 3: Broaden the callback parser in place**

In `src/channels/wecom.rs`:
- Replace the text-only parse gate with a parser that accepts the existing `aibot_msg_callback` envelope for all message types.
- Preserve the current `req_id`, sender, and chat extraction so the reply-context cache still updates.
- Add a small local match for image/file/voice/mixed turns so the channel emits a readable placeholder or summary instead of silently dropping them.
- Keep the reply/send flow unchanged; this batch is about inbound acceptance, not a new WeCom media upload pipeline.

If the payload shape for a specific non-text type is ambiguous, prefer a clear placeholder over a hard failure.

- [ ] **Step 4: Re-run the targeted tests**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test wecom parse_wecom_callback_accepts_non_text_message_types
cargo test --target-dir /tmp/sidekick-target --test wecom wecom_channel_publishes_non_text_inbound_messages
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add <repo-root>/src/channels/wecom.rs <repo-root>/tests/wecom.rs
git commit -m "feat: accept wecom non-text inbound callbacks"
```

### Task 4: Split Feishu Into Inbound Media Handling

**Files:**
- Modify: `<repo-root>/src/channels/feishu.rs`
- Modify: `<repo-root>/tests/feishu.rs`

- [ ] **Step 1: Write the failing Feishu inbound tests**

Add targeted coverage for the current placeholder gap:

```rust
#[tokio::test]
async fn feishu_channel_downloads_image_audio_file_and_post_media() {
    // image/audio/file/post payloads should produce real media paths instead of placeholders
}

#[tokio::test]
async fn feishu_channel_keeps_reply_context_for_media_messages() {
    // accepted media messages should still carry message_id / parent_id / root_id metadata
}
```

The fixture will need extra Feishu API routes for the message resource download path, because the current test server only records create/reply/reaction calls.

- [ ] **Step 2: Run the targeted tests to verify RED**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test feishu feishu_channel_downloads_image_audio_file_and_post_media
cargo test --target-dir /tmp/sidekick-target --test feishu feishu_channel_keeps_reply_context_for_media_messages
```

Expected: FAIL because the Rust Feishu channel still collapses non-text message types to placeholders.

- [ ] **Step 3: Implement Feishu inbound media downloads**

In `src/channels/feishu.rs`:
- Add a download helper modeled on the existing Python behavior for `image_key` and `file_key`.
- Save media under the Feishu media directory and return the local path along with a readable summary string.
- Expand inbound normalization so `image`, `audio`, `file`, `media`, and `post` turns keep their real text/media content instead of placeholder-only output.
- Preserve the current dedup, allowlist, reaction, and reply-context behavior.

Do not introduce a new module unless the file becomes impossible to keep readable.

- [ ] **Step 4: Re-run the targeted tests**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test feishu feishu_channel_downloads_image_audio_file_and_post_media
cargo test --target-dir /tmp/sidekick-target --test feishu feishu_channel_keeps_reply_context_for_media_messages
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add <repo-root>/src/channels/feishu.rs <repo-root>/tests/feishu.rs
git commit -m "feat: download feishu inbound media"
```

### Task 5: Add Feishu Outbound Media Uploads For Message Tool Attachments

**Files:**
- Modify: `<repo-root>/src/channels/feishu.rs`
- Modify: `<repo-root>/tests/feishu.rs`

- [ ] **Step 1: Write the failing outbound media tests**

Add tests that prove Feishu can actually deliver `OutboundMessage.media`:

```rust
#[tokio::test]
async fn feishu_channel_uploads_media_before_sending_text() {
    // local media file should be uploaded and then sent in the same turn
}

#[tokio::test]
async fn message_tool_media_reaches_feishu_outbound_send() {
    // direct message-tool media should survive all the way to the Feishu channel
}
```

This task should reuse the reply/create logic already present in `send()`, not replace it.

- [ ] **Step 2: Run the targeted tests to verify RED**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test feishu feishu_channel_uploads_media_before_sending_text
cargo test --target-dir /tmp/sidekick-target --test feishu message_tool_media_reaches_feishu_outbound_send
```

Expected: FAIL because the current Feishu `send()` implementation ignores `msg.media`.

- [ ] **Step 3: Implement the minimal outbound upload flow**

In `src/channels/feishu.rs`:
- Add upload helpers for image/file media modeled on the Python channel.
- Send `msg.media` first, then send the text content using the current `text` / `post` / `interactive` detection.
- Preserve the `reply_to_message` fallback logic for the first payload only.
- Keep `_progress` and `_tool_hint` skip rules intact.

Do not change the existing Feishu message-format heuristics in this batch.

- [ ] **Step 4: Re-run the targeted tests**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test feishu feishu_channel_uploads_media_before_sending_text
cargo test --target-dir /tmp/sidekick-target --test feishu message_tool_media_reaches_feishu_outbound_send
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add <repo-root>/src/channels/feishu.rs <repo-root>/tests/feishu.rs
git commit -m "feat: upload feishu outbound media"
```

### Task 6: Expand Weixin Beyond Text-Only Where The Current Protocol Already Supports It

**Files:**
- Modify: `<repo-root>/src/channels/weixin.rs`
- Modify: `<repo-root>/tests/weixin.rs`

- [ ] **Step 1: Write the failing Weixin tests**

Add a narrow regression that proves the channel no longer drops every non-text inbound turn:

```rust
#[tokio::test]
async fn weixin_channel_accepts_non_text_getupdates_items() {
    // non-text envelopes should still produce usable inbound messages or summaries
}
```

The test should stay conservative: it only needs to prove the Rust channel accepts more than plain text, not that it implements a new upload protocol.

- [ ] **Step 2: Run the targeted tests to verify RED**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test weixin weixin_channel_accepts_non_text_getupdates_items
```

Expected: FAIL because the current parser only admits `message_type == 1` text turns.

- [ ] **Step 3: Relax the parser without inventing a new protocol**

In `src/channels/weixin.rs`:
- Broaden `parse_weixin_message` so non-text updates are not discarded as soon as the message type is not `1`.
- Preserve `context_token`, `from_user_id`, `get_updates_buf`, and `longpolling_timeout_ms` handling exactly as-is.
- Keep outbound `sendmessage` text-only in this batch unless the current codebase already proves a media shape; if a real media upload schema is needed, split that into a separate follow-up task after protocol confirmation.

This is the place to be conservative. Do not turn this into a new Weixin architecture effort.

- [ ] **Step 4: Re-run the targeted tests**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test weixin weixin_channel_accepts_non_text_getupdates_items
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add <repo-root>/src/channels/weixin.rs <repo-root>/tests/weixin.rs
git commit -m "feat: broaden weixin inbound message parsing"
```

### Task 7: Run The Cross-Batch Verification Sweep

**Files:**
- No code changes; this is the final verification pass.

- [ ] **Step 1: Run formatting**

Run:

```bash
cargo fmt
```

Expected: no diff after formatting.

- [ ] **Step 2: Run the touched Rust test files**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test tools
cargo test --target-dir /tmp/sidekick-target --test agent
cargo test --target-dir /tmp/sidekick-target --test channels
cargo test --target-dir /tmp/sidekick-target --test wecom
cargo test --target-dir /tmp/sidekick-target --test feishu
cargo test --target-dir /tmp/sidekick-target --test weixin
```

Expected: PASS.

- [ ] **Step 3: Run the full Rust suite once**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target
```

Expected: PASS for the full `Sidekick` suite, or a concrete failure that points to a batch-local regression rather than a pre-existing unrelated issue.

- [ ] **Step 4: Finalize the batch**

No new commit unless one of the earlier task commits needs a follow-up fix. If a task fails here, fix it in the same task that introduced the regression, then rerun the specific test file and the full suite.
