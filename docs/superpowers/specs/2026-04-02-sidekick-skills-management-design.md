# Sidekick Skills Management Page Design

Date: 2026-04-02
Repo: `<repo-root>`
Status: Draft approved for spec write-up

## Summary

Add a dedicated `Skills` management tab to the Sidekick web control plane. The page will show builtin skills as read-only reference items and allow full workspace-skill management for `SKILL.md` files under `<workspace>/skills/<id>/SKILL.md`. Runtime discovery, availability checks, and builtin/workspace merge behavior will continue to come from the Rust `skills` module. The web layer will add management APIs for listing, reading, creating, updating, deleting, and enabling or disabling workspace skills via a separate workspace state file.

## Goals

- Add a top-level `Skills` tab to the existing web control plane.
- Reuse the Rust `skills` module as the single source of truth for catalog discovery, availability, and builtin/workspace merge semantics.
- Show builtin skills as read-only items.
- Let users create, edit, delete, enable, and disable workspace skills.
- Edit workspace skills as raw `SKILL.md` text rather than form-derived documents.
- Persist enabled or disabled state outside `SKILL.md` in a dedicated workspace-managed state file.
- Keep the UI aligned with the current control plane layout and styling.
- Preserve the runtime rule that workspace skills can override builtin skills, while disabled workspace skills allow builtin fallthrough to become effective again.

## Non-Goals

- Do not allow editing builtin skills in the web page.
- Do not manage arbitrary files inside a skill directory beyond `SKILL.md`.
- Do not add live preview, diff view, autosave, or rich frontmatter forms in this phase.
- Do not add browser-side markdown rendering of the skill body beyond plain text editing.
- Do not add skill install, import, sync, or remote registry features.
- Do not add browser automation tests in this phase.

## Current State

- `Sidekick` already has a runtime `skills` module that discovers builtin and workspace skills, parses core metadata, evaluates requirements, and selects active skills for prompt assembly.
- The web control plane currently has no `Skills` tab, no skills management API, and no way to edit workspace skills from the UI.
- The control plane layout is a sidebar-tab shell with large content panes for features like sessions, jobs, MCP, settings, and users.
- The current settings page already uses a structure that mixes high-level controls with a raw editor, which is compatible with a raw `SKILL.md` workflow.

## Desired State

### Information Architecture

- Add a new top-level sidebar tab named `Skills`.
- The tab uses a master-detail layout:
  - left rail: searchable skill list
  - right pane: selected skill details and editor
- The left rail is split into two sections:
  - `Workspace Skills`
  - `Builtin Skills`
- `Workspace Skills` are manageable.
- `Builtin Skills` are visible for inspection and copy-based customization, but remain read-only.

### Identity Model

The management page must treat the directory slug as the stable identity of a skill.

- Workspace skill identity is `<workspace>/skills/<id>/`.
- Builtin skill identity is `skills/<id>/`.
- Frontmatter `name` remains editable display metadata, not the storage key.
- The API and UI should consistently use the directory slug as `id`.
- If a user edits `name:` inside `SKILL.md`, the skill remains the same managed item.

This avoids path churn and keeps management behavior stable under raw text editing.

### State Persistence

Enabled or disabled state should be stored separately from `SKILL.md`.

- Use a workspace-local management file such as `<workspace>/.sidekick/skills-state.json`.
- The state file maps workspace skill ids to management state, initially just:
  - `enabled: true|false`
- Missing entries default to enabled.
- Builtin skills do not get independent persisted toggle state in this phase.

Separating state from `SKILL.md` preserves file fidelity and prevents the web page from rewriting user-authored documents just to toggle a switch.

### Effective Merge Behavior

The runtime and management page must agree on override behavior.

- Builtin and workspace skills are both discovered through the Rust `skills` module.
- Workspace skills continue to override builtin skills by normalized skill name when enabled.
- If a workspace skill is disabled, it no longer blocks the builtin skill from becoming the effective skill.
- The management summary returned to the UI should surface:
  - whether a workspace skill overrides a builtin skill
  - whether a builtin skill is shadowed by an enabled workspace skill
  - which source is currently effective

This is the most important correctness rule for the page because it ensures the management UI matches the behavior the agent actually uses at runtime.

## Backend Design

### Layering

The implementation should stay split into two responsibilities:

- `skills` runtime layer
  - discovery
  - parsing
  - requirement checks
  - availability
  - builtin/workspace merge
  - effective skill evaluation with state overlay
- web management layer
  - web DTOs
  - authenticated request handling
  - workspace write operations
  - enabled/disabled persistence

The web layer should not reimplement catalog discovery logic.

### Catalog Extensions

The runtime `skills` module should be extended so the web layer can query management-oriented data without duplicating logic:

- stable `id` derived from the skill directory slug
- read-only `source`
- raw and parsed display fields
- availability and missing requirement summaries
- management flags such as:
  - `enabled`
  - `effective`
  - `overrides_builtin`
  - `shadowed_by_workspace`
  - `has_extra_files`
  - `read_only`

The runtime should also expose a state-aware discovery mode that applies workspace toggle state before producing the effective merged view.

### API Surface

The web API should add a dedicated skills namespace:

- `GET /api/skills`
  - returns grouped summaries for `workspace` and `builtin`
  - includes management flags and effective status
- `GET /api/skills/{source}/{id}`
  - returns full details for one skill
  - workspace details include editable raw `SKILL.md`
  - builtin details are marked `readOnly=true`
- `POST /api/skills/workspace`
  - creates a new workspace skill directory and `SKILL.md`
- `PUT /api/skills/workspace/{id}`
  - overwrites the raw `SKILL.md` contents for an existing workspace skill
- `PUT /api/skills/workspace/{id}/state`
  - updates enabled or disabled state only
- `DELETE /api/skills/workspace/{id}`
  - removes the entire workspace skill directory

### API Shapes

The list response should be optimized for the left-rail summary view and not force the UI to fetch every skill body up front.

Each summary item should include:

- `id`
- `name`
- `description`
- `source`
- `enabled`
- `effective`
- `available`
- `missingRequirements`
- `overridesBuiltin`
- `shadowedByWorkspace`
- `readOnly`
- `hasExtraFiles`

The detail response should additionally include:

- `path`
- `rawContent`
- `body`
- `normalizedName`
- `metadata`
- `parseWarnings`
- `extraFiles`

`extraFiles` is informational only in this phase.

### Write Semantics

- `POST /api/skills/workspace` requires a unique directory slug and initial raw content.
- The backend writes the file as provided and does not normalize or reformat frontmatter.
- `PUT /api/skills/workspace/{id}` is a direct raw file save.
- `PUT /api/skills/workspace/{id}/state` only updates the state file and does not touch `SKILL.md`.
- `DELETE /api/skills/workspace/{id}` deletes the entire skill directory, including unmanaged extra files.

Because deletion removes more than the editor surface manages, the API should expose this explicitly so the UI can confirm it clearly.

## Frontend Design

### Layout

The `Skills` tab should follow the selected master-detail layout.

Left rail:

- search input
- `New Skill` action
- `Workspace Skills` section
- `Builtin Skills` section
- dense rows with:
  - name
  - short description
  - source or override badge
  - state badges for enabled/disabled and available/unavailable

Right pane:

- selected skill header
- details strip for path, requirements state, effective status, and extra-file notice
- raw `SKILL.md` editor
- actions row

### Workspace Skill Detail

When a workspace skill is selected, the right pane shows:

- title and source badge
- enabled toggle
- delete button
- effective status and override summary
- parse warning banner if frontmatter is malformed
- raw text editor for `SKILL.md`
- `Save` action
- `Reload from disk` action

The enabled toggle should save independently from the editor text. Editing the text should mark the pane dirty until saved or reloaded.

### Builtin Skill Detail

When a builtin skill is selected, the right pane shows:

- title and source badge
- read-only status
- availability and requirements summary
- raw `SKILL.md` content in a non-editable editor surface
- `Create workspace copy` action

The copy action should prefill a new workspace skill using the builtin raw content, with a user-supplied or suggested new slug if needed.

### Empty States

If there are no workspace skills, the tab should show:

- an explanatory empty state
- a `Create workspace skill` action
- builtin skills still visible in the reference section

If no skill is selected, the detail pane should show a neutral instructional state rather than a blank panel.

## Interaction Rules

- Opening the tab should select the first workspace skill when one exists.
- If no workspace skills exist, keep the detail pane in empty state until the user picks a builtin skill or creates a new workspace skill.
- Leaving a dirty workspace editor should require confirmation.
- Toggling enabled state should update the list and effective status immediately after the API succeeds.
- Saving raw content should refresh both list summary and detail state from the backend response.
- Creating a workspace copy of a builtin skill should switch selection to the new workspace item after creation.
- If a workspace skill shadows a builtin skill, the builtin row should show that it is shadowed.
- If a workspace skill is disabled and builtin fallback becomes effective, both rows should reflect that change after refresh.

## Error Handling

- Saving raw content should not fail merely because frontmatter is malformed.
- Parse failures should be surfaced as warnings in the detail pane, not as hard save errors.
- Invalid create requests should be limited to:
  - invalid slug
  - directory already exists
  - empty raw content
- Builtin write attempts must return a clear client error such as `403` or `400`.
- Toggle-state failures should be reported independently from editor-save failures.
- Delete failures should be reported as filesystem management errors, not generic save failures.
- If the state file is missing or unreadable, default to all workspace skills enabled and expose a warning only in logs for the initial implementation.

## Testing Strategy

### Rust Backend Tests

- state file parsing and default-enabled behavior
- workspace enabled and disabled transitions
- builtin fallback when a same-name workspace skill is disabled
- list endpoint returns grouped builtin and workspace summaries
- detail endpoint returns raw content and management flags
- create endpoint writes a new workspace skill directory
- update endpoint overwrites raw content without normalization
- state endpoint only changes enabled status
- delete endpoint removes the entire workspace skill directory
- builtin endpoints reject write attempts

### Page Shell Tests

Extend the page-shell tests to assert:

- the presence of the `Skills` tab button
- skills-pane container markup
- search input
- workspace and builtin list sections
- raw skill editor shell
- enabled toggle shell
- create and save action hooks

### Web Integration Tests

Use temporary workspaces and temporary skill trees to validate:

- viewing builtin and workspace skills together
- editing a workspace `SKILL.md`
- creating a workspace copy of a builtin skill
- toggling a workspace skill disabled and observing builtin fallback
- deleting a workspace skill with extra files present

## Risks and Tradeoffs

- Raw text editing preserves user intent but means malformed frontmatter can be saved. This is acceptable because the page is acting as a file manager, not a schema-enforcing CMS.
- Separate state persistence keeps `SKILL.md` clean, but it introduces one more file that runtime and web code must both interpret consistently.
- Directory-slug identity is operationally correct, but it means users who rename `name:` inside the file may see a display name diverge from the underlying id. The UI should make both visible.
- Reusing the runtime `skills` module reduces duplication, but it will require expanding that module’s surface area to serve management data cleanly.

## Implementation Notes

- This feature should reuse the existing web shell, frontend module structure, and test style instead of introducing a new frontend framework or page architecture.
- The initial implementation should keep the page intentionally conservative: no autosave, no preview pane, no file tree browser, and no frontend-only skill parsing logic beyond what is needed for display.
