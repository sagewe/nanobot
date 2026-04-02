# Sidekick Skills Management Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a first-class `Skills` management tab to the Sidekick web control plane that shows builtin skills read-only and lets users create, edit, delete, enable, and disable workspace `SKILL.md` files with runtime-correct builtin fallback behavior.

**Architecture:** Extend the existing Rust `skills` module with a workspace state overlay and management-oriented catalog view, then expose that through authenticated web APIs that resolve the correct workspace in both single-user and multi-user modes. Keep the frontend aligned with the current control plane shell by adding a new `Skills` tab, a focused `frontend/src/skills.js` controller, and a master-detail editor that talks to the new API while preserving raw file editing semantics.

**Tech Stack:** Rust (`axum`, `serde`, `serde_json`, `tempfile`), existing `Sidekick` `skills` and `web` modules, frontend JavaScript with Vite/Vitest, generated `frontend/dist` assets.

---

## File Map

### Rust Runtime and API

- Modify: `<repo-root>/src/skills/mod.rs`
  - Add workspace skill state persistence, management catalog data, effective-source evaluation, and raw-detail helpers.
- Modify: `<repo-root>/tests/skills.rs`
  - Cover disabled workspace skills, builtin fallback, slug identity, and extra-file detection.
- Modify: `<repo-root>/src/web/mod.rs`
  - Add workspace resolution for single-user and multi-user web requests and register new skills routes.
- Modify: `<repo-root>/src/web/api.rs`
  - Add skills list, detail, create, update, toggle-state, and delete handlers plus DTOs.
- Modify: `<repo-root>/tests/web_server.rs`
  - Exercise the new API against temporary workspaces and authenticated user runtimes.

### Frontend

- Modify: `<repo-root>/frontend/index.html`
  - Add the new `Skills` tab button and pane shell markup.
- Modify: `<repo-root>/frontend/src/main.js`
  - Register the tab, instantiate the skills controller, and connect tab lifecycle events.
- Modify: `<repo-root>/frontend/src/api.js`
  - Add fetch helpers for the skills API namespace.
- Create: `<repo-root>/frontend/src/skills.js`
  - Own rendering and interaction logic for the skills pane so `main.js` does not absorb all editor behavior.
- Modify: `<repo-root>/frontend/src/i18n.js`
  - Add English and Chinese labels, actions, warnings, and confirmations for the skills UI.
- Modify: `<repo-root>/frontend/src/style.css`
  - Add master-detail layout, badges, editor, empty states, and responsive rules for the skills pane.
- Create: `<repo-root>/frontend/test/skills.test.js`
  - JSDOM tests for skills-pane rendering, dirty-state handling, and interaction callbacks.

### Shell and Regression Tests

- Modify: `<repo-root>/tests/web_page.rs`
  - Assert the page shell exposes the `Skills` tab and editor surface.
- Modify: `<repo-root>/frontend/test/render.test.js`
  - Keep file-based shell assertions aligned with the new tab and any added source imports if needed.

### Generated Assets

- Regenerate: `<repo-root>/frontend/dist/**`
  - Rebuild Vite output after frontend source changes so the embedded web app serves the new page in production.

### Reference Docs

- Read: `<repo-root>/docs/superpowers/specs/2026-04-02-sidekick-skills-management-design.md`
- Reuse context from: `<repo-root>/docs/superpowers/specs/2026-04-02-sidekick-skills-design.md`

## Task 1: Extend the Skills Runtime with Management State and Effective View

**Files:**
- Modify: `<repo-root>/src/skills/mod.rs`
- Modify: `<repo-root>/tests/skills.rs`

- [ ] **Step 1: Write the failing runtime tests**

```rust
#[test]
fn managed_catalog_disables_workspace_skill_and_restores_builtin_effective_entry() {
    let temp = tempdir().expect("tempdir");
    let builtin_root = temp.path().join("builtin");
    let workspace = temp.path().join("workspace");
    write_skill(
        &builtin_root,
        "weather",
        r#"---
name: weather
description: builtin weather
---

Builtin weather body
"#,
    );
    write_skill(
        &workspace.join("skills"),
        "weather",
        r#"---
name: weather
description: workspace weather
---

Workspace weather body
"#,
    );
    fs::create_dir_all(workspace.join(".Sidekick")).expect("state dir");
    fs::write(
        workspace.join(".sidekick/skills-state.json"),
        r#"{"weather":{"enabled":false}}"#,
    )
    .expect("state file");

    let managed = SkillsCatalog::with_builtin_root(workspace.clone(), builtin_root)
        .discover_managed()
        .expect("managed catalog");

    let workspace_skill = managed.workspace.iter().find(|skill| skill.id == "weather").unwrap();
    let builtin_skill = managed.builtin.iter().find(|skill| skill.id == "weather").unwrap();

    assert!(!workspace_skill.enabled);
    assert!(!workspace_skill.effective);
    assert!(builtin_skill.effective);
}

#[test]
fn managed_catalog_uses_directory_slug_and_reports_extra_files() {
    let temp = tempdir().expect("tempdir");
    let workspace = temp.path().join("workspace");
    write_skill(
        &workspace.join("skills"),
        "release-check",
        r#"---
name: Release Checklist
description: release skill
---

Body
"#,
    );
    fs::write(
        workspace.join("skills/release-check/notes.txt"),
        "extra",
    )
    .expect("extra file");

    let managed = SkillsCatalog::with_builtin_root(workspace, temp.path().join("builtin"))
        .discover_managed()
        .expect("managed catalog");

    let skill = managed.workspace.iter().find(|skill| skill.id == "release-check").unwrap();
    assert_eq!(skill.entry.name, "Release Checklist");
    assert!(skill.has_extra_files);
}
```

- [ ] **Step 2: Run the runtime tests and confirm they fail**

Run: `cargo test --test skills managed_catalog_disables_workspace_skill_and_restores_builtin_effective_entry -- --exact`
Expected: FAIL because `SkillsCatalog` does not yet expose a management view, workspace state parsing, or effective fallback metadata.

- [ ] **Step 3: Implement the management state overlay**

```rust
pub struct ManagedSkillEntry {
    pub id: String,
    pub source: SkillSource,
    pub enabled: bool,
    pub effective: bool,
    pub overrides_builtin: bool,
    pub shadowed_by_workspace: bool,
    pub has_extra_files: bool,
    pub entry: SkillEntry,
}

pub struct ManagedSkills {
    pub workspace: Vec<ManagedSkillEntry>,
    pub builtin: Vec<ManagedSkillEntry>,
}

impl SkillsCatalog {
    pub fn discover_managed(&self) -> Result<ManagedSkills> {
        // Load workspace state from .sidekick/skills-state.json, keep slug identity,
        // compute effective rows after enabled/disabled overlay, and preserve both groups.
    }
}
```

Implementation notes:
- Keep directory slug as the stable `id`, independent of frontmatter `name`.
- Load and save state from `<workspace>/.sidekick/skills-state.json`.
- Default missing state entries to enabled.
- Treat disabled workspace skills as non-effective so builtin skills with the same normalized name can become effective again.
- Detect `has_extra_files` by checking for files other than `SKILL.md` under the skill directory.
- Keep the existing prompt-selection path working by deriving the effective catalog from the same state-aware discovery logic.

- [ ] **Step 4: Re-run the runtime test suite**

Run: `cargo test --test skills`
Expected: PASS with management-state, builtin fallback, and slug-identity coverage.

- [ ] **Step 5: Commit the runtime management layer**

```bash
git add src/skills/mod.rs tests/skills.rs
git commit -m "feat: add managed skills catalog state overlay"
```

## Task 2: Add Skills Management Web APIs and Workspace Resolution

**Files:**
- Modify: `<repo-root>/src/web/mod.rs`
- Modify: `<repo-root>/src/web/api.rs`
- Modify: `<repo-root>/tests/web_server.rs`

- [ ] **Step 1: Write the failing API tests**

```rust
#[tokio::test]
async fn skills_api_lists_builtin_and_workspace_entries_for_authenticated_user() {
    let (state, dir) = multiuser_state();
    let store = ControlStore::new(dir.path()).expect("control store");
    let alice = store.find_by_username("alice").expect("lookup").expect("alice");
    let workspace = store.user_workspace_path(&alice.user_id);
    fs::create_dir_all(workspace.join("skills/weather")).expect("workspace skill dir");
    fs::write(
        workspace.join("skills/weather/SKILL.md"),
        "---\nname: weather\ndescription: workspace weather\n---\n\nBody\n",
    )
    .expect("workspace skill");

    let app = web::build_router(state);
    let cookie = login_cookie(&app, "alice", "password123").await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/skills")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("skills list");

    assert_eq!(response.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(payload["workspace"][0]["id"], json!("weather"));
}

#[tokio::test]
async fn skills_api_toggles_state_without_rewriting_skill_body() {
    let app = build_single_user_skills_router().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/skills/workspace/weather/state")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"enabled":false}"#))
                .unwrap(),
        )
        .await
        .expect("toggle state");

    assert_eq!(response.status(), StatusCode::OK);
    assert!(workspace_path().join(".sidekick/skills-state.json").exists());
    let raw = fs::read_to_string(workspace_path().join("skills/weather/SKILL.md")).unwrap();
    assert!(raw.contains("description: workspace weather"));
}
```

- [ ] **Step 2: Run the API tests and confirm they fail**

Run: `cargo test --test web_server skills_api_lists_builtin_and_workspace_entries_for_authenticated_user -- --exact`
Expected: FAIL because there are no `/api/skills` routes, no workspace resolver in `AppState`, and no handlers for raw skill mutations.

- [ ] **Step 3: Implement the web API surface**

```rust
impl AppState {
    pub async fn workspace_for_user(
        &self,
        user: Option<&AuthenticatedUser>,
    ) -> Result<PathBuf> {
        // Use runtime.workspace_path() for authenticated multi-user requests
        // and a configured single-user workspace path when auth is disabled.
    }
}

pub async fn list_skills(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SkillsListResponse>, ApiError> {
    let user = authenticated_user(&state, &headers).await?;
    let workspace = state.workspace_for_user(user.as_ref()).await?;
    let catalog = SkillsCatalog::new(workspace).discover_managed().map_err(ApiError::internal)?;
    Ok(Json(SkillsListResponse::from_catalog(catalog)))
}
```

Implementation notes:
- Add `AppState::new_with_workspace(...)` or equivalent so single-user web serving can resolve a workspace without auth.
- Register:
  - `GET /api/skills`
  - `GET /api/skills/{source}/{id}`
  - `POST /api/skills/workspace`
  - `PUT /api/skills/workspace/{id}`
  - `PUT /api/skills/workspace/{id}/state`
  - `DELETE /api/skills/workspace/{id}`
- Keep builtin writes rejected with a clear client error.
- Ensure create and update preserve raw `SKILL.md` text exactly as submitted.
- Create parent directories like `.sidekick/` on demand instead of changing workspace template bootstrap logic.

- [ ] **Step 4: Re-run the web API tests**

Run: `cargo test --test web_server`
Expected: PASS for skills list/detail/create/update/toggle/delete coverage, including multi-user scoping and single-user workspace resolution.

- [ ] **Step 5: Commit the API layer**

```bash
git add src/web/mod.rs src/web/api.rs tests/web_server.rs
git commit -m "feat: add skills management api"
```

## Task 3: Add the Skills Tab Shell, Styles, and Translation Keys

**Files:**
- Modify: `<repo-root>/frontend/index.html`
- Modify: `<repo-root>/frontend/src/i18n.js`
- Modify: `<repo-root>/frontend/src/style.css`
- Modify: `<repo-root>/tests/web_page.rs`
- Modify: `<repo-root>/frontend/test/render.test.js`

- [ ] **Step 1: Write the failing shell tests**

```rust
#[test]
fn page_shell_includes_skills_tab_and_editor_regions() {
    let html = sidekick::web::page::render_index_html();

    assert!(html.contains("data-tab=\"skills\""));
    assert!(html.contains("class=\"skills-pane\""));
    assert!(html.contains("id=\"skills-search\""));
    assert!(html.contains("id=\"skills-workspace-list\""));
    assert!(html.contains("id=\"skills-builtin-list\""));
    assert!(html.contains("id=\"skill-editor\""));
    assert!(html.contains("id=\"skill-enabled-toggle\""));
}
```

```js
it("adds translated labels and shell markup for the skills pane", async () => {
  const html = readHtml();
  const css = readCss();
  const { TRANSLATIONS } = await import("../src/i18n.js");

  expect(html).toContain('data-tab="skills"');
  expect(html).toContain('class="skills-pane"');
  expect(css).toContain(".skills-layout");
  expect(TRANSLATIONS.en.tab_skills).toBe("Skills");
  expect(TRANSLATIONS.zh.tab_skills).toBe("技能");
});
```

- [ ] **Step 2: Run the shell tests and confirm they fail**

Run: `cargo test --test web_page page_shell_includes_skills_tab_and_editor_regions -- --exact`
Expected: FAIL because the page shell does not yet define a `Skills` tab or any skills-pane DOM hooks.

Run: `cd <repo-root>/frontend && npm test -- --run test/render.test.js`
Expected: FAIL because the tab markup, styles, and translation keys are missing.

- [ ] **Step 3: Implement the shell and styling**

```html
<button class="tab-btn" data-tab="skills" role="tab">
  <svg><!-- icon --></svg>
  <span class="tab-label" data-i18n="tab_skills">Skills</span>
</button>

<section class="skills-pane" hidden>
  <div class="skills-layout">
    <aside class="skills-sidebar">
      <input type="search" id="skills-search" />
      <button id="skills-create-button" type="button"></button>
      <div id="skills-workspace-list"></div>
      <div id="skills-builtin-list"></div>
    </aside>
    <section class="skills-detail">
      <textarea id="skill-editor" spellcheck="false"></textarea>
    </section>
  </div>
</section>
```

Implementation notes:
- Keep the shell aligned with existing control-plane panels rather than inventing a new page architecture.
- Add `tab_skills` and all supporting labels to both `en` and `zh`.
- Introduce master-detail CSS that collapses cleanly on narrow widths.
- Add only static shell hooks in this task; live data binding comes next.

- [ ] **Step 4: Re-run the shell tests**

Run: `cargo test --test web_page`
Expected: PASS with the new tab and editor shell present.

Run: `cd <repo-root>/frontend && npm test -- --run test/render.test.js`
Expected: PASS with updated markup, CSS, and translations.

- [ ] **Step 5: Commit the shell layer**

```bash
git add frontend/index.html frontend/src/i18n.js frontend/src/style.css tests/web_page.rs frontend/test/render.test.js
git commit -m "feat: add skills tab shell"
```

## Task 4: Implement the Skills Pane Controller and Frontend API Wiring

**Files:**
- Create: `<repo-root>/frontend/src/skills.js`
- Modify: `<repo-root>/frontend/src/api.js`
- Modify: `<repo-root>/frontend/src/main.js`
- Create: `<repo-root>/frontend/test/skills.test.js`

- [ ] **Step 1: Write the failing frontend interaction tests**

```js
// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createSkillsController } from "../src/skills.js";

describe("skills pane", () => {
  it("renders grouped skills and loads detail on selection", async () => {
    const api = {
      fetchSkills: vi.fn().mockResolvedValue({
        workspace: [{ id: "weather", name: "Weather", source: "workspace", enabled: true, effective: true, available: true }],
        builtin: [{ id: "shell", name: "Shell", source: "builtin", enabled: true, effective: true, available: true }],
      }),
      fetchSkillDetail: vi.fn().mockResolvedValue({
        id: "weather",
        source: "workspace",
        rawContent: "---\\nname: Weather\\n---\\n\\nBody",
        readOnly: false,
      }),
    };

    const controller = createSkillsController({ api, root: document.body });
    await controller.load();

    expect(api.fetchSkills).toHaveBeenCalled();
    expect(document.querySelector("[data-skill-id='weather']")).not.toBeNull();
    expect(document.getElementById("skill-editor").value).toContain("Body");
  });

  it("separates state toggles from raw-content saves", async () => {
    const api = {
      fetchSkills: vi.fn().mockResolvedValue(/* ... */),
      fetchSkillDetail: vi.fn().mockResolvedValue(/* ... */),
      updateWorkspaceSkillState: vi.fn().mockResolvedValue({ ok: true }),
      updateWorkspaceSkill: vi.fn().mockResolvedValue({ ok: true }),
    };

    const controller = createSkillsController({ api, root: document.body });
    await controller.load();
    document.getElementById("skill-enabled-toggle").click();
    expect(api.updateWorkspaceSkillState).toHaveBeenCalled();
    expect(api.updateWorkspaceSkill).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run the frontend interaction tests and confirm they fail**

Run: `cd <repo-root>/frontend && npm test -- --run test/skills.test.js`
Expected: FAIL because there is no dedicated skills controller, no API helpers, and no tab lifecycle wiring.

- [ ] **Step 3: Implement the controller and API helpers**

```js
export function createSkillsController({ root, api, setStatus, t, confirmDelete }) {
  return {
    async load() {
      const payload = await api.fetchSkills();
      renderSkillLists(root, payload);
      // Select first workspace skill or keep empty state.
    },
    async selectSkill(source, id) {
      const detail = await api.fetchSkillDetail(source, id);
      renderSkillDetail(root, detail);
    },
  };
}
```

Implementation notes:
- Keep `frontend/src/skills.js` responsible for:
  - list rendering
  - detail rendering
  - dirty-state tracking
  - invoking injected API callbacks
- Add API helpers in `frontend/src/api.js` for all six skills endpoints.
- Keep `frontend/src/main.js` thin:
  - create the controller
  - show and hide the `skills` pane in `switchTab`
  - call `controller.load()` when the tab opens
- Support:
  - grouped workspace and builtin lists
  - editable workspace detail
  - read-only builtin detail
  - create workspace copy
  - raw save
  - reload from disk
  - delete workspace skill
  - immediate enable and disable toggles
- Prompt before leaving a dirty workspace detail.

- [ ] **Step 4: Re-run the frontend tests**

Run: `cd <repo-root>/frontend && npm test -- --run test/skills.test.js test/render.test.js`
Expected: PASS with grouped rendering, toggle/save separation, and tab shell coverage.

- [ ] **Step 5: Commit the frontend behavior**

```bash
git add frontend/src/skills.js frontend/src/api.js frontend/src/main.js frontend/test/skills.test.js
git commit -m "feat: wire skills management tab"
```

## Task 5: Rebuild the Frontend Bundle and Run Full Verification

**Files:**
- Regenerate: `<repo-root>/frontend/dist/**`
- Verify: `<repo-root>/tests/skills.rs`
- Verify: `<repo-root>/tests/web_server.rs`
- Verify: `<repo-root>/tests/web_page.rs`
- Verify: `<repo-root>/frontend/test/render.test.js`
- Verify: `<repo-root>/frontend/test/skills.test.js`

- [ ] **Step 1: Run the targeted Rust and frontend test suites**

Run: `cargo test --test skills --test web_server --test web_page`
Expected: PASS with runtime, API, and page-shell coverage.

Run: `cd <repo-root>/frontend && npm test -- --run test/render.test.js test/skills.test.js`
Expected: PASS with shell and controller behavior covered in Vitest.

- [ ] **Step 2: Rebuild the Vite bundle**

Run: `cd <repo-root>/frontend && npm run build`
Expected: PASS and refreshed files under `frontend/dist/`.

- [ ] **Step 3: Run the full Rust regression suite**

Run: `cargo test`
Expected: PASS so the new skills management page does not regress existing agent, config, or web behavior.

- [ ] **Step 4: Inspect generated artifacts and git status**

Run: `git -C <repo-root> status --short`
Expected: only the intended source, test, and regenerated `frontend/dist` files are modified.

- [ ] **Step 5: Commit the final integrated feature**

```bash
git add frontend/dist frontend/index.html frontend/src/main.js frontend/src/api.js frontend/src/skills.js frontend/src/i18n.js frontend/src/style.css frontend/test/skills.test.js frontend/test/render.test.js src/skills/mod.rs src/web/mod.rs src/web/api.rs tests/skills.rs tests/web_server.rs tests/web_page.rs
git commit -m "feat: add skills management page"
```

## Execution Notes

- Follow `@superpowers:test-driven-development` during implementation: write the failing test first for each task, verify the failure, then implement the minimum fix.
- Use `@superpowers:verification-before-completion` before claiming the page is complete.
- If implementation runs in a separate branch or worktree, start with `@superpowers:using-git-worktrees`.
- For execution, prefer `@superpowers:subagent-driven-development` or `@superpowers:executing-plans` rather than ad hoc edits.
