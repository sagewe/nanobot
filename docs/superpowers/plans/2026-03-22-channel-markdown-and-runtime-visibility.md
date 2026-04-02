# Channel Markdown And Runtime Visibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Hide internal runtime/tool-hint messages from external channels and render assistant CommonMark replies correctly in Web, Telegram, and WeCom.

**Architecture:** Add a shared `presentation` module with one filter layer and one Markdown rendering layer. External channels call the filter before sending, then render the same CommonMark source into channel-specific output: sanitized HTML for Web, Telegram Bot API HTML for Telegram, and WeCom markdown payloads for WeCom.

**Tech Stack:** Rust, `pulldown-cmark`, `ammonia`, existing `axum` web routes, existing Telegram/WeCom channel tests, `cargo test`

---

## File Map

**Create**
- `<repo-root>/.worktrees/channel-markdown-visibility/src/presentation/mod.rs`  
  Re-export presentation helpers.
- `<repo-root>/.worktrees/channel-markdown-visibility/src/presentation/filters.rs`  
  Central policy for whether an outbound message should be shown on a channel.
- `<repo-root>/.worktrees/channel-markdown-visibility/src/presentation/markdown.rs`  
  CommonMark renderers for Web HTML, Telegram HTML, and WeCom markdown.
- `<repo-root>/.worktrees/channel-markdown-visibility/tests/presentation.rs`  
  Focused unit tests for filtering and renderer behavior.

**Modify**
- `<repo-root>/.worktrees/channel-markdown-visibility/Cargo.toml`  
  Add Markdown/sanitization dependencies.
- `<repo-root>/.worktrees/channel-markdown-visibility/Cargo.lock`  
  Dependency lockfile update.
- `<repo-root>/.worktrees/channel-markdown-visibility/src/lib.rs`  
  Export `presentation`.
- `<repo-root>/.worktrees/channel-markdown-visibility/src/channels/mod.rs`  
  Apply visibility filter and Telegram HTML rendering/chunking.
- `<repo-root>/.worktrees/channel-markdown-visibility/src/channels/wecom.rs`  
  Apply visibility filter and send final replies as WeCom markdown messages.
- `<repo-root>/.worktrees/channel-markdown-visibility/src/web/api.rs`  
  Preserve `reply`, add rendered HTML field.
- `<repo-root>/.worktrees/channel-markdown-visibility/src/web/page.rs`  
  Render assistant HTML safely and keep user text plain.
- `<repo-root>/.worktrees/channel-markdown-visibility/tests/channels.rs`  
  Telegram filtering/rendering integration tests.
- `<repo-root>/.worktrees/channel-markdown-visibility/tests/wecom.rs`  
  WeCom filtering and markdown reply integration tests.
- `<repo-root>/.worktrees/channel-markdown-visibility/tests/web_server.rs`  
  API contract tests for `reply` + `replyHtml`.
- `<repo-root>/.worktrees/channel-markdown-visibility/tests/web_page.rs`  
  Page-shell expectations for assistant HTML rendering.

### Task 1: Add Presentation Filter Scaffolding

**Files:**
- Create: `<repo-root>/.worktrees/channel-markdown-visibility/src/presentation/mod.rs`
- Create: `<repo-root>/.worktrees/channel-markdown-visibility/src/presentation/filters.rs`
- Modify: `<repo-root>/.worktrees/channel-markdown-visibility/src/lib.rs`
- Create: `<repo-root>/.worktrees/channel-markdown-visibility/tests/presentation.rs`

- [x] **Step 1: Write the failing filter tests**

Add a focused test file with cases for:

```rust
#[test]
fn runtime_messages_are_hidden_from_external_channels() {
    assert!(!should_deliver_to_channel("telegram", &progress_metadata()));
    assert!(!should_deliver_to_channel("wecom", &tool_hint_metadata()));
    assert!(!should_deliver_to_channel("web", &progress_metadata()));
}

#[test]
fn cli_keeps_runtime_messages_visible() {
    assert!(should_deliver_to_channel("cli", &progress_metadata()));
}

#[test]
fn normal_messages_remain_visible_everywhere() {
    assert!(should_deliver_to_channel("telegram", &HashMap::new()));
}
```

- [x] **Step 2: Run the targeted tests to verify RED**

Run: `cargo test --target-dir /tmp/sidekick-target --test presentation`

Expected: FAIL because `presentation` module and `should_deliver_to_channel` do not exist yet.

- [x] **Step 3: Implement the minimal filter layer**

Add:

```rust
pub fn should_deliver_to_channel(channel: &str, metadata: &HashMap<String, Value>) -> bool {
    let is_runtime = metadata.get("_progress").and_then(Value::as_bool).unwrap_or(false)
        || metadata.get("_tool_hint").and_then(Value::as_bool).unwrap_or(false);

    match channel {
        "cli" => true,
        "telegram" | "wecom" | "web" => !is_runtime,
        _ => true,
    }
}
```

Export the module from `lib.rs`.

- [x] **Step 4: Run the targeted tests to verify GREEN**

Run: `cargo test --target-dir /tmp/sidekick-target --test presentation`

Expected: PASS

- [x] **Step 5: Commit**

```bash
git add <repo-root>/.worktrees/channel-markdown-visibility/src/presentation/mod.rs <repo-root>/.worktrees/channel-markdown-visibility/src/presentation/filters.rs <repo-root>/.worktrees/channel-markdown-visibility/src/lib.rs <repo-root>/.worktrees/channel-markdown-visibility/tests/presentation.rs
git commit -m "feat: add channel delivery filter"
```

### Task 2: Add Shared Markdown Rendering Helpers

**Files:**
- Modify: `<repo-root>/.worktrees/channel-markdown-visibility/Cargo.toml`
- Modify: `<repo-root>/.worktrees/channel-markdown-visibility/Cargo.lock`
- Create: `<repo-root>/.worktrees/channel-markdown-visibility/src/presentation/markdown.rs`
- Modify: `<repo-root>/.worktrees/channel-markdown-visibility/src/presentation/mod.rs`
- Modify: `<repo-root>/.worktrees/channel-markdown-visibility/tests/presentation.rs`

- [x] **Step 1: Write the failing renderer tests**

Extend `tests/presentation.rs` with targeted cases:

```rust
#[test]
fn web_renderer_returns_sanitized_html() {
    let html = render_web_html("**bold** <script>alert(1)</script>");
    assert!(html.contains("<strong>bold</strong>"));
    assert!(!html.contains("<script>"));
}

#[test]
fn telegram_renderer_returns_html_subset() {
    let html = render_telegram_html("**bold** `code` [link](https://example.com)");
    assert!(html.contains("<b>bold</b>"));
    assert!(html.contains("<code>code</code>"));
    assert!(html.contains("<a href=\"https://example.com\">link</a>"));
}

#[test]
fn wecom_renderer_returns_markdown_and_enforces_limit() {
    let rendered = render_wecom_markdown("# title");
    assert!(rendered.contains("# title"));
}
```

- [x] **Step 2: Run the targeted tests to verify RED**

Run: `cargo test --target-dir /tmp/sidekick-target --test presentation`

Expected: FAIL because renderer helpers and dependencies are missing.

- [x] **Step 3: Implement minimal renderers and dependencies**

Add dependencies in `Cargo.toml`:

```toml
pulldown-cmark = "0.13.2"
ammonia = "4.1.2"
```

Implement renderer helpers with:

- CommonMark parsing via `pulldown-cmark`
- Web HTML sanitization via `ammonia`
- Telegram HTML conversion for supported inline/block forms
- WeCom markdown passthrough/normalization with 20480-byte truncation fallback

Keep unsupported constructs readable rather than perfect.

- [x] **Step 4: Run the targeted tests to verify GREEN**

Run: `cargo test --target-dir /tmp/sidekick-target --test presentation`

Expected: PASS

- [x] **Step 5: Commit**

```bash
git add <repo-root>/.worktrees/channel-markdown-visibility/Cargo.toml <repo-root>/.worktrees/channel-markdown-visibility/Cargo.lock <repo-root>/.worktrees/channel-markdown-visibility/src/presentation/mod.rs <repo-root>/.worktrees/channel-markdown-visibility/src/presentation/markdown.rs <repo-root>/.worktrees/channel-markdown-visibility/tests/presentation.rs
git commit -m "feat: add shared markdown renderers"
```

### Task 3: Wire Web Assistant HTML Rendering

**Files:**
- Modify: `<repo-root>/.worktrees/channel-markdown-visibility/src/web/api.rs`
- Modify: `<repo-root>/.worktrees/channel-markdown-visibility/src/web/page.rs`
- Modify: `<repo-root>/.worktrees/channel-markdown-visibility/tests/web_server.rs`
- Modify: `<repo-root>/.worktrees/channel-markdown-visibility/tests/web_page.rs`

- [x] **Step 1: Write the failing Web API and page tests**

Add/adjust tests to assert:

```rust
assert_eq!(response["reply"], "hello from agent");
assert!(response["replyHtml"].as_str().unwrap().contains("<strong>"));
```

And in page-shell tests, assert assistant messages are inserted through an HTML path such as:

```rust
assert!(html.contains("node.innerHTML = content"));
assert!(html.contains("payload.replyHtml"));
```

- [x] **Step 2: Run the targeted Web tests to verify RED**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test web_server
cargo test --target-dir /tmp/sidekick-target --test web_page
```

Expected: FAIL because the API does not yet return `replyHtml` and the page still uses plain-text assistant insertion.

- [x] **Step 3: Implement minimal Web rendering**

In `web/api.rs`:

- Extend `ChatResponse` with `replyHtml`
- Keep the existing `reply` field intact
- Render `replyHtml` using `render_web_html(&reply)`

In `web/page.rs`:

- Keep user messages on `textContent`
- Add an assistant-specific append path that uses safe HTML from `replyHtml`
- Preserve current composer, reset, and shortcut behavior

- [x] **Step 4: Run the targeted Web tests to verify GREEN**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test web_server
cargo test --target-dir /tmp/sidekick-target --test web_page
```

Expected: PASS

- [x] **Step 5: Commit**

```bash
git add <repo-root>/.worktrees/channel-markdown-visibility/src/web/api.rs <repo-root>/.worktrees/channel-markdown-visibility/src/web/page.rs <repo-root>/.worktrees/channel-markdown-visibility/tests/web_server.rs <repo-root>/.worktrees/channel-markdown-visibility/tests/web_page.rs
git commit -m "feat: render markdown replies in web ui"
```

### Task 4: Filter And Render Telegram Output

**Files:**
- Modify: `<repo-root>/.worktrees/channel-markdown-visibility/src/channels/mod.rs`
- Modify: `<repo-root>/.worktrees/channel-markdown-visibility/tests/channels.rs`
- Modify: `<repo-root>/.worktrees/channel-markdown-visibility/tests/presentation.rs`

- [x] **Step 1: Write the failing Telegram tests**

Extend `tests/channels.rs` with cases for:

```rust
#[tokio::test]
async fn telegram_channel_drops_runtime_messages() {
    // send OutboundMessage with _progress=true
    // assert mock server captured zero sends
}

#[tokio::test]
async fn telegram_channel_sends_rendered_html() {
    // send "**bold**"
    // assert parse_mode == "HTML"
    // assert text contains "<b>bold</b>"
}
```

Also add a focused renderer/chunking test in `tests/presentation.rs` for long Telegram HTML content that must split on safe boundaries.

- [x] **Step 2: Run the targeted Telegram tests to verify RED**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test channels telegram_channel_drops_runtime_messages
cargo test --target-dir /tmp/sidekick-target --test channels telegram_channel_sends_rendered_html
cargo test --target-dir /tmp/sidekick-target --test presentation telegram_html_chunks_preserve_tags
```

Expected: FAIL because filtering, render conversion, and safe chunking are not wired yet.

- [x] **Step 3: Implement minimal Telegram filtering and rendering**

In `src/channels/mod.rs`:

- Return early from `TelegramChannel::send` if `should_deliver_to_channel("telegram", &msg.metadata)` is `false`
- Render with `render_telegram_html(&msg.content)`
- Send `parse_mode: "HTML"`
- Replace raw string chunking with a helper that splits rendered HTML on complete block/tag boundaries

- [x] **Step 4: Run the targeted Telegram tests to verify GREEN**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test channels telegram_channel_drops_runtime_messages
cargo test --target-dir /tmp/sidekick-target --test channels telegram_channel_sends_rendered_html
cargo test --target-dir /tmp/sidekick-target --test presentation telegram_html_chunks_preserve_tags
```

Expected: PASS

- [x] **Step 5: Commit**

```bash
git add <repo-root>/.worktrees/channel-markdown-visibility/src/channels/mod.rs <repo-root>/.worktrees/channel-markdown-visibility/tests/channels.rs <repo-root>/.worktrees/channel-markdown-visibility/tests/presentation.rs
git commit -m "feat: render telegram markdown replies"
```

### Task 5: Filter And Render WeCom Output

**Files:**
- Modify: `<repo-root>/.worktrees/channel-markdown-visibility/src/channels/wecom.rs`
- Modify: `<repo-root>/.worktrees/channel-markdown-visibility/tests/wecom.rs`
- Modify: `<repo-root>/.worktrees/channel-markdown-visibility/tests/presentation.rs`

- [x] **Step 1: Write the failing WeCom tests**

Extend `tests/wecom.rs` with cases for:

```rust
#[tokio::test]
async fn wecom_channel_drops_runtime_messages() {
    // send OutboundMessage with _tool_hint=true
    // assert no aibot_respond_msg payload is emitted
}

#[tokio::test]
async fn wecom_channel_sends_markdown_replies() {
    // establish reply context
    // send "**bold**"
    // assert payload.body.msgtype == "markdown"
    // assert payload.body.markdown.content contains rendered content
}
```

Add a renderer test that verifies 20480-byte truncation/fallback remains valid UTF-8.

- [x] **Step 2: Run the targeted WeCom tests to verify RED**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test wecom wecom_channel_drops_runtime_messages
cargo test --target-dir /tmp/sidekick-target --test wecom wecom_channel_sends_markdown_replies
cargo test --target-dir /tmp/sidekick-target --test presentation wecom_markdown_respects_size_limit
```

Expected: FAIL because WeCom still uses text stream replies and does not filter runtime messages.

- [x] **Step 3: Implement minimal WeCom markdown sending**

In `src/channels/wecom.rs`:

- Add a builder like:

```rust
pub fn build_wecom_markdown_reply_request(req_id: &str, content: &str) -> Value {
    json!({
        "cmd": "aibot_respond_msg",
        "headers": { "req_id": req_id },
        "body": {
            "msgtype": "markdown",
            "markdown": { "content": content }
        }
    })
}
```

- Filter hidden runtime messages before sending
- Render visible content with `render_wecom_markdown`
- Keep reply context lookup unchanged

- [x] **Step 4: Run the targeted WeCom tests to verify GREEN**

Run:

```bash
cargo test --target-dir /tmp/sidekick-target --test wecom wecom_channel_drops_runtime_messages
cargo test --target-dir /tmp/sidekick-target --test wecom wecom_channel_sends_markdown_replies
cargo test --target-dir /tmp/sidekick-target --test presentation wecom_markdown_respects_size_limit
```

Expected: PASS

- [x] **Step 5: Run full verification**

Run:

```bash
cargo fmt
cargo test --target-dir /tmp/sidekick-target
```

Expected: PASS for the full Rust suite

- [x] **Step 6: Commit**

```bash
git add <repo-root>/.worktrees/channel-markdown-visibility/src/channels/wecom.rs <repo-root>/.worktrees/channel-markdown-visibility/tests/wecom.rs <repo-root>/.worktrees/channel-markdown-visibility/tests/presentation.rs
git commit -m "feat: render wecom markdown replies"
```
