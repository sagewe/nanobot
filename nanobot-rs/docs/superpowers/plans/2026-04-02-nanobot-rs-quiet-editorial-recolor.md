# Quiet Editorial Recolor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Recolor the `nanobot-rs` frontend to the approved Anthropic-inspired "Quiet Editorial" palette across login, app shell, light theme, dark theme, semantic accents, and syntax highlighting without changing behavior or layout.

**Architecture:** Treat `frontend/src/style.css` as the single source of truth for the new visual system. First lock the new palette into tests, then replace the shared theme tokens and the login-facing light-theme surfaces, then extend the same system through dark mode, semantic badges, avatars, traces, and code highlighting, and finally run full frontend verification.

**Tech Stack:** Vite, Vitest, vanilla JavaScript, HTML, CSS

---

## File Map

- Modify: `frontend/src/style.css`
  - Replace the current warm-orange token set with the approved Anthropic-inspired neutral palette.
  - Normalize hard-coded light, dark, semantic, avatar, and highlight colors back to shared tokens or approved semantic hues.
- Modify: `frontend/index.html`
  - Adjust the login-page wording to a quieter editorial tone if the existing "control plane" copy still clashes after recoloring.
- Modify: `frontend/test/render.test.js`
  - Add regression tests that lock the approved palette, the removal of off-brand purple accents, the syntax-highlight mapping, and the calmer login/app-shell treatments.

### Task 1: Lock and implement the quiet-editorial light palette

**Files:**
- Modify: `frontend/test/render.test.js`
- Modify: `frontend/src/style.css`
- Modify: `frontend/index.html`

- [ ] **Step 1: Write the failing regression test for the approved light-theme palette and quieter login tone**

```js
it("locks the quiet editorial light palette and calmer login copy", () => {
  const css = readCss();
  const lightThemeCss = readCssBlock(":root\\[data-theme=\"light\"\\]");
  const loginShellCss = readCssBlock("\\.login-shell");
  const loginCardCss = readCssBlock("\\.login-card");
  const html = readHtml();

  expect(lightThemeCss).toContain("--paper: #faf9f5;");
  expect(lightThemeCss).toContain("--ink: #141413;");
  expect(lightThemeCss).toContain("--accent: #d97757;");
  expect(lightThemeCss).toContain("--line: #e8e6dc;");
  expect(lightThemeCss).not.toContain("#C15F3C");

  expect(loginShellCss).toContain("radial-gradient");
  expect(loginShellCss).not.toContain("rgba(193, 95, 60, 0.16)");
  expect(loginCardCss).not.toContain("rgba(193, 95, 60, 0.18)");

  expect(html).toContain("Nanobot Workspace");
  expect(html).toContain("Enter Workspace");
  expect(html).not.toContain("Enter Control Plane");

  expect(css).not.toContain("#A14A2F");
});
```

- [ ] **Step 2: Run the targeted test to verify it fails on the current palette**

Run: `cd /Users/sage/nanobot/nanobot-rs/frontend && npm test -- test/render.test.js -t "locks the quiet editorial light palette and calmer login copy"`

Expected: FAIL because the stylesheet still uses `#C15F3C`, `#A14A2F`, and the warmer login gradients, and the login button copy still says `Enter Control Plane`.

- [ ] **Step 3: Implement the shared light-theme token pass and login-surface recolor**

```css
:root[data-theme="light"] {
  --paper: #faf9f5;
  --ink: #141413;
  --muted: #b0aea5;
  --dim: #5f5b54;
  --accent: #d97757;
  --accent-strong: #c96b4d;
  --panel: rgba(250, 249, 245, 0.92);
  --panel-soft: rgba(232, 230, 220, 0.52);
  --sidebar-bg: #ece9df;
  --line: #e8e6dc;
  --line-strong: rgba(20, 20, 19, 0.1);
  --input-bg: #fdfbf6;
  --info: #6a9bcc;
  --success: #788c5d;
  --warning: #a27b3b;
  --danger: #c56b5d;
  --code-bg: #f1eee5;
}

.login-shell {
  background:
    radial-gradient(circle at top left, rgba(217, 119, 87, 0.08), transparent 34%),
    radial-gradient(circle at bottom right, rgba(20, 20, 19, 0.05), transparent 30%),
    linear-gradient(160deg, #faf9f5 0%, #f1eee5 100%);
}

.login-card {
  border: 1px solid rgba(20, 20, 19, 0.08);
  background: rgba(250, 249, 245, 0.92);
}
```

Update the login copy in `frontend/index.html` to the quieter wording:

```html
<div class="login-kicker">Nanobot Workspace</div>
<h1>Sign in to your workspace</h1>
<button id="login-button" type="submit">Enter Workspace</button>
```

- [ ] **Step 4: Run the targeted test to verify the light-theme slice passes**

Run: `cd /Users/sage/nanobot/nanobot-rs/frontend && npm test -- test/render.test.js -t "locks the quiet editorial light palette and calmer login copy"`

Expected: PASS

- [ ] **Step 5: Commit the light-theme slice**

```bash
cd /Users/sage/nanobot
git add nanobot-rs/frontend/src/style.css nanobot-rs/frontend/index.html nanobot-rs/frontend/test/render.test.js
git commit -m "feat: add quiet editorial light palette"
```

### Task 2: Lock and implement dark theme, assistant avatar, and syntax colors

**Files:**
- Modify: `frontend/test/render.test.js`
- Modify: `frontend/src/style.css`

- [ ] **Step 1: Write the failing regression test for the dark theme, avatar cleanup, and syntax palette**

```js
it("locks the quiet editorial dark palette and removes the purple assistant avatar", () => {
  const darkThemeCss = readCssBlock(":root\\[data-theme=\"dark\"\\]");
  const autoDarkThemeCss = readCssBlock(":root:not\\(\\[data-theme=\"light\"\\]\\):not\\(\\[data-theme=\"dark\"\\]\\)");
  const darkAvatarCss = readCssBlock(":root\\[data-theme=\"dark\"\\] \\.msg-group\\[data-role=\"assistant\"\\] \\.msg-avatar");
  const css = readCss();

  expect(darkThemeCss).toContain("--paper: #141413;");
  expect(darkThemeCss).toContain("--ink: #faf9f5;");
  expect(darkThemeCss).toContain("--accent: #d97757;");
  expect(autoDarkThemeCss).toContain("--accent: #d97757;");

  expect(darkAvatarCss).not.toContain("#af8aff");
  expect(darkAvatarCss).not.toContain("#8663ff");
  expect(darkAvatarCss).not.toContain("#5a47d8");

  expect(css).toContain("--hljs-keyword: #d97757;");
  expect(css).toContain("--hljs-attr: #6a9bcc;");
  expect(css).toContain("--hljs-string: #788c5d;");

  expect(css).not.toContain("#d4724a");
  expect(css).not.toContain("#b85c38");
});
```

- [ ] **Step 2: Run the targeted test to verify it fails on the current dark-theme styling**

Run: `cd /Users/sage/nanobot/nanobot-rs/frontend && npm test -- test/render.test.js -t "locks the quiet editorial dark palette and removes the purple assistant avatar"`

Expected: FAIL because dark mode still uses the older orange-heavy token set and the assistant avatar still contains the purple gradient.

- [ ] **Step 3: Implement the dark-token pass, assistant-avatar cleanup, and syntax highlight remap**

```css
:root[data-theme="dark"] {
  --paper: #141413;
  --ink: #faf9f5;
  --muted: #8f8b83;
  --dim: #c1bdb4;
  --accent: #d97757;
  --accent-strong: #e08a6f;
  --panel: rgba(20, 20, 19, 0.95);
  --panel-soft: rgba(39, 38, 35, 0.92);
  --sidebar-bg: #1d1c1a;
  --line: #34322d;
  --line-strong: rgba(250, 249, 245, 0.08);
  --input-bg: #23211f;
  --info: #6a9bcc;
  --success: #788c5d;
  --warning: #b19154;
  --danger: #cf7b70;
  --code-bg: #23211f;
}

:root[data-theme="dark"] .msg-group[data-role="assistant"] .msg-avatar {
  background: linear-gradient(180deg, #2a2825 0%, #141413 100%);
  border-color: rgba(250, 249, 245, 0.08);
  color: #faf9f5;
}

:root[data-theme="dark"] .msg-tool-output-content.hljs,
:root[data-theme="dark"] .msg-trace-code.hljs {
  --hljs-string:  #788c5d;
  --hljs-number:  #b19154;
  --hljs-keyword: #d97757;
  --hljs-attr:    #6a9bcc;
  --hljs-comment: #8f8b83;
  --hljs-literal: #b19154;
  --hljs-tag:     #6a9bcc;
  --hljs-name:    #788c5d;
  --hljs-punct:   #c1bdb4;
}
```

Mirror the same replacements in the system-dark `:root:not([data-theme="light"]):not([data-theme="dark"])` block so auto-dark and explicit dark stay aligned.

- [ ] **Step 4: Run the targeted test to verify the dark-theme slice passes**

Run: `cd /Users/sage/nanobot/nanobot-rs/frontend && npm test -- test/render.test.js -t "locks the quiet editorial dark palette and removes the purple assistant avatar"`

Expected: PASS

- [ ] **Step 5: Commit the dark-theme slice**

```bash
cd /Users/sage/nanobot
git add nanobot-rs/frontend/src/style.css nanobot-rs/frontend/test/render.test.js
git commit -m "feat: add quiet editorial dark palette"
```

### Task 3: Remove remaining off-brand semantic accents and verify the full frontend

**Files:**
- Modify: `frontend/test/render.test.js`
- Modify: `frontend/src/style.css`

- [ ] **Step 1: Write the failing regression test for semantic badges and remaining legacy colors**

```js
it("uses restrained semantic accents and removes legacy off-brand palette leftovers", () => {
  const sessionBadgeCss = readCssBlock("\\.session-channel-badge");
  const weixinBadgeCss = readCssBlock("\\.session-channel-badge\\[data-channel=\"weixin\"\\]");
  const telegramBadgeCss = readCssBlock("\\.session-channel-badge\\[data-channel=\"telegram\"\\]");
  const darkWeixinBadgeCss = readCssBlock(":root\\[data-theme=\"dark\"\\] \\.session-channel-badge\\[data-channel=\"weixin\"\\]");
  const css = readCss();

  expect(sessionBadgeCss).toContain("background:");
  expect(sessionBadgeCss).toContain("color: var(--accent);");
  expect(weixinBadgeCss).toContain("color: var(--success);");
  expect(telegramBadgeCss).toContain("color: var(--info);");
  expect(darkWeixinBadgeCss).toContain("color: var(--success);");

  expect(css).not.toContain("#af8aff");
  expect(css).not.toContain("#8663ff");
  expect(css).not.toContain("#5a47d8");
  expect(css).not.toContain("#56d364");
  expect(css).not.toContain("#64b5f6");
});
```

- [ ] **Step 2: Run the targeted test to verify it fails before the semantic cleanup**

Run: `cd /Users/sage/nanobot/nanobot-rs/frontend && npm test -- test/render.test.js -t "uses restrained semantic accents and removes legacy off-brand palette leftovers"`

Expected: FAIL because session badges and status-related colors still rely on leftover bright greens, blues, and historical dark-theme values.

- [ ] **Step 3: Implement the remaining semantic cleanup and token substitution sweep**

```css
.session-channel-badge {
  background: rgba(217, 119, 87, 0.1);
  color: var(--accent);
}

.session-channel-badge[data-channel="weixin"] {
  background: color-mix(in srgb, var(--success) 14%, transparent);
  color: var(--success);
}

.session-channel-badge[data-channel="telegram"] {
  background: color-mix(in srgb, var(--info) 14%, transparent);
  color: var(--info);
}
```

If `color-mix()` would be inconsistent with the project's browser target, replace it with equivalent fixed `rgba(...)` values derived from the approved palette. Sweep the rest of `frontend/src/style.css` for stale palette literals and convert them to the new tokens or restrained semantic colors.

- [ ] **Step 4: Run the full frontend test suite**

Run: `cd /Users/sage/nanobot/nanobot-rs/frontend && npm test`

Expected: PASS with `vitest` reporting all frontend tests green.

- [ ] **Step 5: Run the production build**

Run: `cd /Users/sage/nanobot/nanobot-rs/frontend && npm run build`

Expected: PASS with Vite emitting a production bundle and no CSS/HTML import errors.

- [ ] **Step 6: Commit the semantic cleanup and verification pass**

```bash
cd /Users/sage/nanobot
git add nanobot-rs/frontend/src/style.css nanobot-rs/frontend/test/render.test.js nanobot-rs/frontend/index.html
git commit -m "feat: complete quiet editorial frontend recolor"
```

## Notes for Execution

- Keep the change set limited to the approved recolor scope. Do not restructure markup or move logic between files.
- Reuse existing selectors and layout scaffolding whenever possible; this is a palette and tone pass, not a component rewrite.
- If a targeted CSS assertion is too brittle, prefer reading the specific selector block with `readCssBlock(...)` instead of matching the entire stylesheet.
- Manual visual inspection should happen after `npm run build` if a local preview is available, with special attention to:
  - login page
  - session rail
  - selected/hover states
  - assistant avatar
  - trace/output surfaces
  - skills/settings panels
  - dark mode parity
