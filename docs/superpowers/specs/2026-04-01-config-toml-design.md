# Config TOML Migration Design

Date: 2026-04-01
Repo: `<repo-root>`
Status: Draft approved for spec write-up

## Summary

Unify all human-edited configuration in `Sidekick` under `config.toml` while leaving machine-managed runtime state in its current JSON or JSONL formats. This keeps editable configuration consistent for operators and users without expanding the migration into session, audit, cron, MCP, or connector state.

## Goals

- Make user-facing configuration files TOML by default.
- Keep the existing `Config` Rust type as the source of truth.
- Preserve compatibility with existing JSON config during migration.
- Make all new writes produce TOML only.
- Update the web settings editor and CLI messaging to reflect TOML as the canonical format.

## Non-Goals

- Do not convert machine-managed state such as:
  - `control/users.json`
  - `control/system.json`
  - `control/web_sessions.json`
  - `control/migration.json`
  - `control/audit.jsonl`
  - `workspace/cron/jobs.json`
  - `workspace/mcp/tools.json`
  - session logs, Weixin state, or other runtime caches
- Do not change API payloads from structured JSON objects to raw TOML text.
- Do not redesign the overall config schema.

## Current State

- Runtime config loading already supports both TOML and JSON based on file extension.
- The multi-user control plane still points user config paths at `users/<user_id>/config.json`.
- Legacy onboarding and migration logic still assume root `config.json`.
- The web settings page exposes an "Advanced JSON" editor even though TOML is now the desired operator-facing format.

## Desired State

### Canonical File Paths

- Legacy single-user config becomes `~/.config.toml`
- Per-user config becomes `~/.users/<user_id>/config.toml`

### Read Behavior

- Prefer `config.toml` if present.
- Fall back to `config.json` only when TOML does not exist.
- If both exist, load TOML and log that JSON was ignored.

### Write Behavior

- All config writes produce `config.toml` only.
- Writing follows an atomic flow:
  1. serialize config to TOML
  2. write `config.toml.tmp`
  3. rename to `config.toml`
  4. remove stale `config.json` if present
- If TOML serialization or write fails, keep the prior files untouched.

## Scope of Code Changes

### Config Layer

- Keep `load_config` dual-format during migration.
- Add helpers for canonical TOML path resolution where callers currently hardcode JSON file names.
- Keep save logic TOML-first and make callers route through canonical TOML paths.

### Control Plane

- Change `ControlStore::user_config_path` to return `config.toml`.
- Update user config load/save helpers to prefer TOML and clean up migrated JSON.
- Keep control-plane metadata files in JSON.

### CLI

- Update onboarding and legacy migration to read old `config.json` but emit `config.toml`.
- Update help text, error messages, and path reporting to refer to TOML as the default config file.
- Keep `agent --user` and other runtime commands loading through control-store path resolution, which now targets TOML.

### Web

- Change the settings editor label from `Advanced JSON` to `Advanced TOML`.
- Keep HTTP APIs structured as config objects rather than raw TOML text.
- Convert between editor text and structured config at the boundary:
  - render config as TOML for editing
  - parse TOML on submit
  - return parse errors with file context and field location when possible

## Migration Rules

### Legacy Root Config

- If `config.toml` exists, use it.
- Else if `config.json` exists, read it and write canonical `config.toml`.
- Remove `config.json` only after a successful TOML write.

### Per-User Config

- If `users/<id>/config.toml` exists, use it.
- Else if `users/<id>/config.json` exists, read it and rewrite as TOML in place.
- Cleanup of stale JSON happens after successful TOML write.

### Idempotence

- Re-running migration must be safe.
- Systems already on TOML should not rewrite files unless the config content changes.

## Error Handling

- TOML parse failures should identify the config source path and include the parse error.
- When both TOML and JSON exist, TOML wins and a warning is emitted so operators understand why JSON edits had no effect.
- Web save failures must not replace the currently active config file.
- Migration failures must leave the old JSON config in place.

## Testing Strategy

### Config Tests

- load prefers TOML over JSON
- load falls back to JSON when TOML is absent
- save writes TOML and removes stale JSON after success

### Migration Tests

- legacy root `config.json` migrates to `config.toml`
- per-user `config.json` migrates to `config.toml`
- failed TOML write keeps JSON intact
- both files present loads TOML

### CLI Tests

- onboarding emits TOML config
- `migrate-legacy` produces TOML config
- user-facing path text references TOML

### Web Tests

- settings UI labels advanced editor as TOML
- config page can render TOML content
- invalid TOML submission returns an error and preserves existing config

## Risks and Tradeoffs

- TOML round-tripping may reorder keys or normalize formatting; this is acceptable because the file remains user-editable and deterministic formatting is preferable to mixed legacy output.
- Keeping read compatibility for JSON adds a temporary branching path, but it reduces migration risk and allows safe upgrades from existing installs.
- Leaving machine-managed files in JSON means the storage directory remains mixed-format, but that is intentional because the goal is operator-facing consistency rather than full persistence uniformity.

## Acceptance Criteria

- New installs create `config.toml` rather than `config.json`.
- Existing JSON config is auto-migrated to TOML on first successful write or migration path.
- Multi-user user config lives at `users/<user_id>/config.toml`.
- Web settings presents TOML as the advanced editing format.
- Control-plane state and runtime state remain unchanged in format.
