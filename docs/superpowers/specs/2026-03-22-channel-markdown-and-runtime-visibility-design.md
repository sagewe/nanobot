# Channel Markdown And Runtime Visibility Design

## Summary

Implement two user-facing behavior changes for `nanobot-rs`:

1. Hide internal runtime/progress/tool-hint messages from external channels (`telegram`, `wecom`, `web`).
2. Render assistant Markdown replies across `telegram`, `wecom`, and `web` from one CommonMark source.

The recommended design is a shared presentation layer that sits between agent output and channel delivery. It decides whether a message is externally visible, then renders the same CommonMark content into channel-specific output:

- Web: sanitized HTML
- Telegram: Bot API HTML
- WeCom: long-connection `markdown` message payload

CLI keeps its current plain-text progress and tool-hint output for operator visibility.

## Goals

- Remove odd external strings such as `message("telegram")` and `message("wecom")` from user-facing channels.
- Keep internal execution visibility in local operator surfaces, especially CLI and terminal logs.
- Let the assistant write one Markdown reply format and have channels render the best supported representation.
- Reuse one rendering pipeline instead of maintaining three unrelated channel-specific formatters.
- Preserve current session, provider, and tool-loop behavior.

## Non-Goals

- No streaming Markdown rendering in the web page.
- No image or attachment Markdown support.
- No raw HTML passthrough from model output.
- No custom per-channel prompt engineering to force different output formats.
- No changes to provider behavior or message history storage format.

## Current Problems

### External runtime leakage

The agent currently emits progress and tool hints through `BusProgressReporter` using outbound metadata such as `_progress` and `_tool_hint`. Those messages are dispatched through the same outbound path as normal assistant replies. As a result, external channels can surface internal execution details that were meant for operator observability, including strings derived from tool hints.

### Inconsistent Markdown behavior

Current output is plain text everywhere:

- Web app inserts assistant replies with `textContent`
- Telegram sends plain `text`
- WeCom currently sends stream text messages

This means model replies that contain Markdown are not rendered, and users see formatting syntax directly.

## Chosen Approach

Introduce a shared presentation module with two responsibilities:

1. Visibility filtering
2. Channel-specific Markdown rendering

This module will be called by outbound channel send paths and the web chat response path. Agent logic continues to produce ordinary text plus metadata. Presentation logic decides what users actually see.

## Alternatives Considered

### 1. Patch each channel directly

Hide `_progress` and `_tool_hint` separately in Telegram, WeCom, and Web, then add a separate Markdown formatter in each place.

Pros:

- Smaller immediate diff

Cons:

- Duplicates logic
- Increases drift between channels
- Makes future channel additions expensive

### 2. Shared presentation layer

Add one module for visibility rules and one module for Markdown rendering, then call it from each output path.

Pros:

- Single policy for user-visible vs internal messages
- Single CommonMark source of truth
- Easier test coverage and future extension

Cons:

- Requires a small amount of new structure

### 3. Internal rich-text AST

Parse Markdown into an internal AST and serialize per channel.

Pros:

- Most structurally correct long term

Cons:

- Too large for this slice
- Unnecessary before more channels or rich content types exist

## Recommended Architecture

### New module

Create:

- `/Users/sage/nanobot/nanobot-rs/src/presentation/mod.rs`
- `/Users/sage/nanobot/nanobot-rs/src/presentation/filters.rs`
- `/Users/sage/nanobot/nanobot-rs/src/presentation/markdown.rs`

### Responsibilities

#### `filters.rs`

Decide whether an `OutboundMessage` should be shown to end users on a given channel.

Rules:

- `cli`: keep current behavior, including runtime progress and tool hints
- `telegram`: hide messages with `_progress=true` or `_tool_hint=true`
- `wecom`: hide messages with `_progress=true` or `_tool_hint=true`
- `web`: hide runtime/progress/tool-hint messages from browser users

This should use existing outbound metadata instead of changing agent semantics.

Configuration semantics for this slice:

- External user channels (`telegram`, `wecom`, `web`) always suppress runtime-only messages, regardless of `sendProgress` and `sendToolHints`
- Existing operator-oriented surfaces such as CLI and terminal logs keep their current visibility behavior
- The config keys remain in place for backward compatibility and can be revisited later if per-channel operator previews are needed

#### `markdown.rs`

Provide three renderer entry points:

- `render_web_html(markdown: &str) -> String`
- `render_telegram_html(markdown: &str) -> String`
- `render_wecom_markdown(markdown: &str) -> String`

Common input format:

- Assistant final replies and `message` tool content are treated as CommonMark source text.

### Rendering targets

#### Web

Pipeline:

1. Parse CommonMark with `pulldown-cmark`
2. Generate HTML
3. Sanitize generated HTML with `ammonia`
4. Return sanitized HTML to the frontend

The browser page should render assistant content with safe HTML insertion. User-authored messages should remain plain text.

#### Telegram

Pipeline:

1. Parse CommonMark
2. Convert supported nodes to Telegram Bot API HTML subset
3. Escape all plain text segments for Telegram HTML safety
4. Send using `parse_mode: "HTML"`

Why HTML instead of MarkdownV2:

- Telegram HTML is easier to generate correctly
- MarkdownV2 has fragile escaping rules and is more likely to regress on code blocks, punctuation, and mixed content

#### WeCom

Pipeline:

1. Parse CommonMark
2. Convert supported nodes to WeCom markdown syntax
3. Send as a WeCom `markdown` message payload instead of the current `stream` text payload

Message shape:

- `msgtype: "markdown"`
- `markdown.content: <rendered content>`

Operational limit from the official long-connection document:

- `markdown.content` must be UTF-8 and must not exceed 20480 bytes

This aligns with the official WeCom intelligent bot long-connection documentation, which explicitly documents `markdown消息` under the long-connection message type section.

Source:

- [WeCom intelligent bot long connection](https://developer.work.weixin.qq.com/document/path/101463)
- [Telegram Bot API formatting options](https://core.telegram.org/bots/api#formatting-options)

## Supported Markdown Semantics

The source format is CommonMark, but rendering is capability-based. Supported constructs:

- Headings
- Paragraphs
- Bold
- Italic
- Links
- Inline code
- Fenced code blocks
- Bulleted and numbered lists
- Blockquotes

Degradation rules:

- Tables: Web may render them if the parser output supports them; Telegram and WeCom should degrade to readable plain text blocks
- Unsupported nested structures: flatten to text
- Raw HTML input: never trusted, never passed through directly

## Message Visibility Semantics

### What remains user-visible

- Normal assistant final replies
- `message` tool output meant for the user
- Existing direct-reply semantics for web requests

### What becomes internal-only

- Agent progress messages sent through `_progress`
- Tool-hint messages sent through `_tool_hint`
- Any strings that only describe internal execution state

### CLI exception

CLI remains the operator-oriented interface and keeps the current progress/tool-hint visibility.

## Data Flow

### Telegram and WeCom

1. Agent emits `OutboundMessage`
2. Channel send path asks presentation filter whether the message is externally visible
3. If hidden, drop it silently
4. If visible, render Markdown for that channel
5. Send channel-formatted payload

### Web

1. Web chat service receives final reply string from direct processing
2. Before returning JSON, render the assistant reply to sanitized HTML
3. API returns both the original text reply and rendered HTML
4. Frontend appends assistant messages with safe HTML
5. User messages remain plain text

## Channel-Specific Changes

### Telegram

Modify `/Users/sage/nanobot/nanobot-rs/src/channels/mod.rs`:

- Filter out runtime-only outbound messages before `sendMessage`
- Render visible content using Telegram HTML
- Include `parse_mode: "HTML"`
- Preserve current chunking logic, but ensure chunks are split after rendering in a way that does not break tags

### WeCom

Modify `/Users/sage/nanobot/nanobot-rs/src/channels/wecom.rs`:

- Filter out runtime-only outbound messages
- Add a markdown reply builder alongside the existing stream builder
- Send final user-facing replies as `markdown`
- Preserve reply context behavior and long-connection semantics
- Enforce the documented 20480-byte `markdown.content` limit with truncation/fallback behavior

### Web

Modify:

- `/Users/sage/nanobot/nanobot-rs/src/web/api.rs`
- `/Users/sage/nanobot/nanobot-rs/src/web/page.rs`

Changes:

- API preserves the existing text reply field and adds a rendered HTML field for assistant messages
- Frontend inserts assistant HTML into the transcript
- Frontend continues to render user messages with `textContent`

## Error Handling

- Rendering failure should degrade to escaped plain text, not fail the whole send path
- Sanitization should always run for web output
- Telegram unsupported structures should degrade to readable HTML-safe text
- WeCom unsupported structures should degrade to readable markdown/plain text
- Hidden runtime messages should be silently dropped, not logged as send errors

## Testing Strategy

### Visibility filtering

- Telegram outbound runtime messages are dropped
- WeCom outbound runtime messages are dropped
- Web user-facing payload excludes runtime/tool-hint strings
- CLI behavior remains unchanged

### Telegram rendering

- CommonMark converts to Telegram HTML for bold, italic, links, inline code, code blocks, and lists
- Special characters are escaped correctly
- Chunking does not split inside HTML tags

### WeCom rendering

- CommonMark converts to WeCom markdown
- WeCom send path uses markdown message type for final replies
- Reply contexts continue to route replies correctly

### Web rendering

- Assistant Markdown renders as HTML
- User messages stay plain text
- Unsafe HTML is sanitized out

### Regression coverage

- Existing progress logging tests still pass
- Existing channel tests still pass
- Existing WeCom runtime and reply tests still pass after message-type changes

## Implementation Order

1. Add presentation filter module and hide runtime messages for external channels
2. Add tests proving `telegram`, `wecom`, and `web` no longer surface runtime/tool-hint messages
3. Add shared Markdown rendering helpers
4. Wire web sanitized HTML rendering
5. Wire Telegram HTML rendering
6. Switch WeCom final replies to markdown message payloads
7. Run the full Rust test suite

## Risks And Mitigations

### Telegram chunking can break rendered output

Mitigation:

- Split rendered output on safe boundaries
- Add targeted tests for long formatted replies

### WeCom markdown may not match Telegram/Web semantics exactly

Mitigation:

- Treat CommonMark as the authoring source
- Explicitly degrade unsupported constructs instead of trying to emulate unsupported behavior

### Web HTML rendering can introduce XSS if done naively

Mitigation:

- Generate HTML from Markdown only
- Always sanitize with `ammonia`
- Keep user message rendering as plain text

## Success Criteria

- Telegram, WeCom, and Web no longer show internal runtime messages such as `message("telegram")` or `message("wecom")`
- Assistant Markdown is rendered in all three channels
- CLI still shows execution progress for operator use
- Full `cargo test` suite remains green
