# Tab Style Unification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unify `Sidekick`'s tab-like UI so the sidebar tabs, pane headers, grouped section headers, and header actions share one restrained navigation language without changing behavior.

**Architecture:** Keep the work inside `frontend/src/style.css` and `frontend/index.html`, and use `frontend/test/render.test.js` to lock the shared navigation tokens, semantic header classes, and theme regressions before each CSS/HTML slice lands. Start by introducing shared navigation tokens and section primitives, then apply semantic classes and shared surfaces through `Jobs`, `MCP`, `Skills`, `Settings`, and `Users`, and finish with collapsed-sidebar/dark-theme regression locking plus full frontend verification.

**Tech Stack:** Vite, Vitest, vanilla HTML, vanilla CSS, vanilla JavaScript

---

## File Map

- Read: `frontend/src/main.js`
  - Confirm the current tab switching behavior is ID- and pane-based so the visual changes do not accidentally require JavaScript changes.
- Modify: `frontend/src/style.css`
  - Add shared navigation/section tokens.
  - Add shared `section-*` primitives.
  - Rewire pane-specific headers, grouped surfaces, and header actions to the same visual rules.
  - Replace hard-coded dark active-tab overrides with token-driven values.
- Modify: `frontend/index.html`
  - Add semantic classes such as `section-header`, `section-title-group`, `section-eyebrow`, `section-action`, and `section-surface` to the existing pane markup.
  - Keep the structure and IDs stable so behavior stays unchanged.
- Modify: `frontend/test/render.test.js`
  - Add CSS/HTML regression tests that lock the shared token names, semantic markup, unified header/button rules, and dark/collapsed sidebar behavior.

### Task 1: Lock shared navigation tokens and section primitives

**Files:**
- Modify: `frontend/test/render.test.js`
- Modify: `frontend/src/style.css`

- [ ] **Step 1: Write the failing regression test for shared navigation tokens and reusable section primitives**

Add a focused test near the existing CSS/HTML snapshot-style assertions:

```js
it("defines shared navigation tokens and section primitives", () => {
  const rootCss = readCssBlock(":root");
  const tabBtnCss = readCssBlock("\\.tab-btn");
  const sectionHeaderCss = readCssBlock("\\.section-header");
  const sectionTitleGroupCss = readCssBlock("\\.section-title-group");
  const sectionActionCss = readCssBlock("\\.section-action");

  expect(rootCss).toContain("--nav-item-radius:");
  expect(rootCss).toContain("--nav-item-pad-y:");
  expect(rootCss).toContain("--nav-item-pad-x:");
  expect(rootCss).toContain("--nav-hover-bg:");
  expect(rootCss).toContain("--nav-active-bg:");
  expect(rootCss).toContain("--section-surface-bg:");

  expect(tabBtnCss).toContain("border-radius: var(--nav-item-radius);");
  expect(tabBtnCss).toContain("padding: var(--nav-item-pad-y) var(--nav-item-pad-x);");
  expect(sectionHeaderCss).toContain("display: flex;");
  expect(sectionHeaderCss).toContain("justify-content: space-between;");
  expect(sectionTitleGroupCss).toContain("display: grid;");
  expect(sectionActionCss).toContain("border-radius: var(--nav-item-radius);");
});
```

- [ ] **Step 2: Run the targeted test to verify it fails**

Run:

```bash
cd <repo-root>/frontend && npm test -- test/render.test.js -t "defines shared navigation tokens and section primitives"
```

Expected: FAIL because `frontend/src/style.css` does not yet define the shared `--nav-*` / `--section-*` tokens or the reusable `.section-*` selectors.

- [ ] **Step 3: Implement the shared token layer and base section primitives**

In `frontend/src/style.css`, add a small shared token set under `:root` and the new reusable section selectors:

```css
:root {
  --nav-item-radius: 0.72rem;
  --nav-item-pad-y: 0.56rem;
  --nav-item-pad-x: 0.7rem;
  --nav-hover-bg: rgba(217, 119, 87, 0.08);
  --nav-active-bg: #fdfcfb;
  --nav-active-ink: var(--accent);
  --nav-quiet-bg: transparent;
  --section-surface-bg: rgba(255, 255, 255, 0.02);
  --section-surface-border: var(--line);
}

.section-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 0.75rem;
}

.section-title-group {
  display: grid;
  gap: 0.15rem;
}

.section-action {
  border: 1px solid var(--line);
  border-radius: var(--nav-item-radius);
  background: var(--nav-quiet-bg);
  color: var(--dim);
}
```

Update `.tab-btn` to consume the new shared tokens instead of fixed radius/padding values.

- [ ] **Step 4: Run the targeted test to verify the base token slice passes**

Run:

```bash
cd <repo-root>/frontend && npm test -- test/render.test.js -t "defines shared navigation tokens and section primitives"
```

Expected: PASS.

- [ ] **Step 5: Commit the shared-token slice**

```bash
cd <repo-root>
git add frontend/src/style.css frontend/test/render.test.js
git commit -m "feat: add shared tab styling tokens"
```

### Task 2: Add semantic section classes to pane markup and grouped surfaces

**Files:**
- Modify: `frontend/test/render.test.js`
- Modify: `frontend/index.html`

- [ ] **Step 1: Write the failing regression test for semantic section markup**

Add a markup-focused test:

```js
it("marks tab-adjacent panes with shared section semantics", () => {
  const html = readHtml();

  expect(html).toContain('class="jobs-header section-header"');
  expect(html).toContain('id="jobs-refresh-btn" type="button" class="section-action"');
  expect(html).toContain('class="control-panel control-panel--skills section-surface"');
  expect(html).toContain('class="skills-group section-surface"');
  expect(html).toContain('class="skills-detail section-surface"');
  expect(html).toContain('class="users-pane-header section-title-group"');
  expect(html).toContain('class="session-kicker section-eyebrow"');
});
```

- [ ] **Step 2: Run the targeted test to verify it fails**

Run:

```bash
cd <repo-root>/frontend && npm test -- test/render.test.js -t "marks tab-adjacent panes with shared section semantics"
```

Expected: FAIL because `frontend/index.html` does not yet include the shared `section-*` classes.

- [ ] **Step 3: Apply the semantic classes to the existing pane markup**

In `frontend/index.html`, annotate the existing elements without changing IDs or pane structure:

```html
<div class="jobs-header section-header">
  <div class="section-title-group">
    <h2 class="jobs-title" data-i18n="jobs_title">Scheduled Jobs</h2>
  </div>
  <button id="jobs-refresh-btn" type="button" class="section-action" ...>
```

Apply the same idea to:

- `Jobs` and `MCP` pane headers
- `Skills` panel header, group headers, and detail header
- `Settings` workspace header, section headers, channel-group headers, and advanced header
- `Users` pane header and the create/list panel headers
- `section-surface` on shared grouped surfaces such as `control-panel`, `skills-group`, `skills-detail`, `settings-section`, and `settings-channel-group`

Do not rename or remove any existing pane-specific classes; only add shared semantic classes.
Do not invent new labels or `data-i18n` keys just to satisfy the shared header structure. When a pane only has a title today, keep the existing copy and wrap it with the shared semantic classes.

- [ ] **Step 4: Run the targeted test to verify the markup slice passes**

Run:

```bash
cd <repo-root>/frontend && npm test -- test/render.test.js -t "marks tab-adjacent panes with shared section semantics"
```

Expected: PASS.

- [ ] **Step 5: Commit the semantic-markup slice**

```bash
cd <repo-root>
git add frontend/index.html frontend/test/render.test.js
git commit -m "feat: add shared section markup classes"
```

### Task 3: Unify pane header actions, grouped surfaces, and dark/collapsed tab states

**Files:**
- Modify: `frontend/test/render.test.js`
- Modify: `frontend/src/style.css`

- [ ] **Step 1: Write the failing regression test for header-action reuse and theme-safe active states**

Add a CSS regression test that locks the remaining visual unification:

```js
it("reuses shared header actions and token-driven active tab states", () => {
  const css = readCss();
  const jobsHeaderButtonCss = readCssBlock("\\.jobs-header button");
  const controlPanelHeaderButtonCss = readCssBlock("\\.control-panel-header button");
  const jobsTitleCss = readCssBlock("\\.jobs-title");
  const skillsGroupCss = readCssBlock("\\.skills-group");
  const settingsSectionCss = readCssBlock("\\.settings-section");
  const activeTabCss = readCssBlock("\\.tab-btn\\[data-active=\"true\"\\]");
  const collapsedActiveCss = readCssBlock("\\.session-rail\\[data-collapsed=\"true\"\\] \\.tab-btn\\[data-active=\"true\"\\]");

  expect(jobsHeaderButtonCss).toContain("border-radius: var(--nav-item-radius);");
  expect(controlPanelHeaderButtonCss).toContain("border-radius: var(--nav-item-radius);");
  expect(jobsTitleCss).toContain('font-family: "Poppins", Arial, sans-serif;');
  expect(skillsGroupCss).toContain("background: var(--section-surface-bg);");
  expect(settingsSectionCss).toContain("background: var(--section-surface-bg);");
  expect(css).toContain("--nav-active-bg-strong:");
  expect(activeTabCss).toContain("background: var(--nav-active-bg-strong);");
  expect(collapsedActiveCss).toContain("box-shadow:");
});
```

- [ ] **Step 2: Run the targeted test to verify it fails**

Run:

```bash
cd <repo-root>/frontend && npm test -- test/render.test.js -t "reuses shared header actions and token-driven active tab states"
```

Expected: FAIL because pane header buttons and grouped surfaces still use separate local values, and the dark active-tab styling still relies on the hard-coded `#252320` override.

- [ ] **Step 3: Rewire pane-specific selectors to the shared section system**

In `frontend/src/style.css`:

- point `.jobs-header button` and `.control-panel-header button` to the same radius, border, hover, and background rules as `.section-action`
- point `.skills-group`, `.skills-detail`, `.settings-section`, and `.settings-channel-group` to `var(--section-surface-bg)` and `var(--section-surface-border)`
- align section titles such as `.jobs-title`, `.settings-section-title`, and other major pane headings to the approved heading family where practical
- add a theme-aware `--nav-active-bg-strong` token to the light and dark token sets
- move `.tab-btn[data-active="true"]` to the shared token path so light, dark, and auto-dark all inherit the same active-state logic
- add a collapsed-sidebar active-state boost such as an inset ring or box shadow so the icon-only state remains legible

Representative CSS shape:

```css
:root[data-theme="light"] {
  --nav-active-bg-strong: #fdfcfb;
}

.jobs-header button,
.control-panel-header button,
.section-action {
  border-radius: var(--nav-item-radius);
  background: var(--nav-quiet-bg);
}

.skills-group,
.skills-detail,
.settings-section,
.settings-channel-group,
.section-surface {
  background: var(--section-surface-bg);
  border: 1px solid var(--section-surface-border);
}

.jobs-title,
.settings-section-title,
.sidebar-title {
  font-family: "Poppins", Arial, sans-serif;
}

.tab-btn[data-active="true"] {
  color: var(--nav-active-ink);
  background: var(--nav-active-bg-strong);
}

.session-rail[data-collapsed="true"] .tab-btn[data-active="true"] {
  box-shadow: inset 0 0 0 1px rgba(217, 119, 87, 0.24);
}
```

- [ ] **Step 4: Run the targeted test to verify the theme/state slice passes**

Run:

```bash
cd <repo-root>/frontend && npm test -- test/render.test.js -t "reuses shared header actions and token-driven active tab states"
```

Expected: PASS.

- [ ] **Step 5: Commit the visual-unification slice**

```bash
cd <repo-root>
git add frontend/src/style.css frontend/test/render.test.js
git commit -m "feat: unify tab-adjacent header styling"
```

### Task 4: Run full frontend verification and fix any small regressions

**Files:**
- Modify: `frontend/src/style.css`
- Modify: `frontend/index.html`
- Modify: `frontend/test/render.test.js`

- [ ] **Step 1: Run the full frontend test suite**

Run:

```bash
cd <repo-root>/frontend && npm test
```

Expected: PASS.

- [ ] **Step 2: Run the production build**

Run:

```bash
cd <repo-root>/frontend && npm run build
```

Expected: PASS with a normal Vite production build output and no CSS/HTML syntax errors.

- [ ] **Step 3: Manually smoke-check the unified panes**

Inspect the app in the browser and verify:

- sidebar expanded
- sidebar collapsed
- `Jobs`
- `MCP`
- `Skills`
- `Settings`
- admin-only `Users`
- light theme
- dark theme

Focus on whether pane headers, grouped section surfaces, and header actions all read as one family without any behavior change.

- [ ] **Step 4: Apply the smallest verification fixes if the smoke check exposes spacing or state regressions**

Only if needed, make minimal corrections in the already-touched files. Do not introduce new component abstractions or JavaScript behavior changes during cleanup.

- [ ] **Step 5: Commit the verification pass**

If Step 4 changed files, commit the verification fix. If Step 4 produced no code changes, explicitly skip this step.

```bash
cd <repo-root>
git add frontend/src/style.css frontend/index.html frontend/test/render.test.js
git commit -m "chore: verify unified tab styling"
```
