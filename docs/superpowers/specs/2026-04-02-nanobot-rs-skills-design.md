# nanobot-rs Skills Module Design

Date: 2026-04-02
Repo: `/Users/sage/nanobot/nanobot-rs`
Status: Draft approved for spec write-up

## Summary

Add a first-class `skills` module to `nanobot-rs` that discovers builtin and workspace skills, parses skill metadata, checks availability requirements, and selects active skills for each main-agent turn using a local priority order of `always > explicit > semantic`. The selected skill bodies are injected into the main agent system prompt as active skills, while the full skill catalog remains available as a summary for additional on-demand reading. `nanobot-rs` will maintain its own builtin skills directory rather than reusing the Python runtime's skill tree.

## Goals

- Add a dedicated Rust `skills` module instead of embedding skills logic inside `agent::ContextBuilder`.
- Support builtin skills under `nanobot-rs/skills/<name>/SKILL.md`.
- Support workspace skills under `<workspace>/skills/<name>/SKILL.md`.
- Make workspace skills override builtin skills with the same normalized name.
- Parse core frontmatter fields compatible with current nanobot skills usage:
  - `name`
  - `description`
  - `always`
  - `metadata`
- Parse `metadata` JSON for `nanobot` and `openclaw` keys, including:
  - `requires.bins`
  - `requires.env`
  - optional future fields such as `keywords` or `tags`
- Select active skills locally with a deterministic priority order:
  1. available `always` skills
  2. available explicit matches from the current user message
  3. top-scoring available semantic matches from the current user message
- Inject selected skills into the main agent prompt as full skill content.
- Keep a summary of all discovered skills in the prompt so the model can still read additional `SKILL.md` files when needed.
- Keep subagent skill behavior conservative by exposing only the skills summary and not auto-injecting semantic matches.

## Non-Goals

- Do not add a new tool protocol for skills.
- Do not make skills executable objects or turn them into MCP-style tools.
- Do not add remote skill installation, registry sync, or hot update behavior.
- Do not persist selected skills into session state.
- Do not add an extra LLM call for skill routing.
- Do not aim for byte-for-byte feature parity with the Python runtime.
- Do not auto-inject semantic skills into subagent prompts in this phase.

## Current State

- `nanobot-rs` has no standalone `skills` module.
- Main agent prompt construction lives inside `agent::ContextBuilder`.
- `ContextBuilder` currently scans only `<workspace>/skills/*/SKILL.md` and emits a plain list of paths under a `## Skills` section.
- There is no builtin skills directory inside `nanobot-rs`.
- There is no frontmatter parsing, requirements checking, `always` handling, explicit skill matching, or semantic skill matching in Rust.
- Subagent prompts currently contain a small standalone system prompt and no skill selection behavior.
- The Python runtime already has a `SkillsLoader`, but `nanobot-rs` should establish its own builtin skill source and prompt behavior.

## Desired State

### Module Layout

- Add `src/skills/mod.rs` and export it from `src/lib.rs`.
- Keep the initial implementation in one module unless it becomes too large; splitting can happen later if needed.
- Add a new repository directory `nanobot-rs/skills/` for Rust builtin skills.

### Data Model

The `skills` module should expose focused types that separate discovery, metadata, and selection:

- `SkillEntry`
  - normalized skill name
  - display name
  - description
  - source (`builtin` or `workspace`)
  - `SKILL.md` path
  - raw content
  - stripped body content
  - parsed metadata
  - availability status
  - missing requirement summary
  - search text used for selection
- `SkillMetadata`
  - `always`
  - `requires.bins`
  - `requires.env`
  - optional `keywords` and `tags`
- `SelectedSkills`
  - ordered active skill entries
  - per-skill selection reason (`always`, `explicit`, `semantic`)
  - explicit request status entries for named but unavailable skills

### Discovery Rules

- Builtin skills load from `nanobot-rs/skills`.
- Workspace skills load from `<workspace>/skills`.
- Each skill lives in a directory containing `SKILL.md`.
- Skill names are normalized for matching and override resolution.
- Workspace skills override builtin skills with the same normalized name.
- Discovery happens fresh for each prompt build in this phase. No cross-turn cache is required.

### Metadata and Content Parsing

- Parse YAML-like frontmatter only when the file starts with `---`.
- Keep parsing tolerant:
  - if frontmatter parsing fails, keep the skill as a body-only skill
  - if the file cannot be read, skip the skill and log a warning
- Extract `description` from frontmatter when present, otherwise fall back to the skill name.
- Parse the `metadata` field as JSON text and accept either the `nanobot` or `openclaw` object.
- Availability is derived from `requires.bins` and `requires.env`.
- The stripped body is what gets injected into `Active Skills`.

### Selection Strategy

The selector operates only on the current incoming user message. It does not inspect the full conversation history in this phase.

#### Priority Order

1. Available `always` skills
2. Available explicit matches
3. Available semantic matches

Later categories cannot displace earlier ones. Deduplicate by normalized skill name while preserving the first applicable reason.

#### Explicit Matching

Explicit matching should recognize:

- `$skill-name`
- backticked skill names such as `` `weather` ``
- normalized exact matches against the user message, with support for case folding and separators like spaces, `_`, and `-`

Explicit matching should only activate a skill when the message clearly names it. Incidental substring hits should not count.

#### Semantic Matching

Semantic matching remains local and deterministic. The first implementation should score skills using token overlap between the user message and the skill's indexed text:

- normalized skill name
- description
- optional keywords or tags from metadata
- headings and leading paragraphs from the skill body

Rules:

- ignore common stop-like tokens and punctuation-only fragments
- require more than one meaningful token match
- enforce a minimum score threshold
- select only the top `N` semantic matches, with `N` intentionally small such as `2` or `3`

The goal is predictable usefulness, not broad fuzzy activation.

### Prompt Assembly

Main agent system prompt should contain:

1. identity and workspace context
2. bootstrap files
3. memory content
4. `Active Skills` section with full stripped content for selected skills in this order:
   - available `always` skills
   - available explicit matches
   - available semantic matches
5. optional `Requested Skills Status` section for explicit matches that were found but unavailable
6. `Skills` summary section for the full discovered catalog

The `Skills` summary should include:

- skill name
- description
- path
- source
- availability state
- missing requirements when unavailable

If prompt budget needs to be constrained later, lower-priority semantic skills should be dropped before trimming `always` or explicit skills.

### Agent Integration

- `ContextBuilder` should depend on the new `skills` module instead of directly scanning the workspace skills directory.
- `build_system_prompt` should accept the current user message so selection can happen before the final prompt is assembled.
- `build_messages` should pass the current user message into `build_system_prompt`.
- Main agent behavior should use the full selector output.
- Subagent prompt building should use the shared catalog summary but should not auto-inject selected skill bodies in this phase.

## Scope of Code Changes

### Skills Module

- Add discovery helpers for builtin and workspace skill directories.
- Add frontmatter parsing and tolerant metadata extraction.
- Add requirement checks against installed binaries and environment variables.
- Add prompt rendering helpers for:
  - active skill content
  - requested skill status
  - full skills summary
- Add a local selector for `always`, explicit, and semantic matches.

### Agent Context

- Replace ad hoc workspace skill scanning in `ContextBuilder`.
- Extend prompt construction to include selected skill bodies and the new summary format.
- Ensure prompt assembly remains deterministic for testability.

### Subagent Context

- Reuse the catalog summary output where practical.
- Keep subagent prompts summary-only in this phase.

### Builtin Skills Tree

- Create `nanobot-rs/skills/README.md`.
- Add an initial builtin skills set only as needed for Rust runtime bring-up. The module design must not depend on a large initial catalog.

## Error Handling

- If a skill directory is malformed or unreadable, skip that skill and emit a warning.
- If frontmatter parsing fails, continue with body-only parsing.
- If metadata JSON parsing fails, treat metadata as empty and keep the skill available unless requirements cannot be evaluated from parsed fields.
- Skills with unmet requirements remain visible in the summary with `available=false`.
- Unavailable skills are never auto-injected by `always` or semantic selection.
- If the user explicitly requests an unavailable skill, include it in `Requested Skills Status` so the model can explain why it was not activated.
- If skill selection fails unexpectedly, fall back to prompt assembly with the skills summary only and no active skill injection.
- Subagent prompt assembly should fail closed to "no skills section" rather than blocking the subagent request.

## Testing Strategy

### Skills Module Unit Tests

- discover builtin skills from `nanobot-rs/skills`
- discover workspace skills from `<workspace>/skills`
- workspace skill overrides builtin skill by normalized name
- parse frontmatter fields and stripped body correctly
- parse `metadata` JSON under both `nanobot` and `openclaw`
- detect availability from required binaries and environment variables
- compute missing requirement summaries for unavailable skills

### Selector Unit Tests

- include available `always` skills automatically
- match `$skill-name` explicitly
- match backticked skill names explicitly
- normalize separators and case for explicit matching
- avoid false positives on incidental substrings
- rank semantic matches deterministically
- enforce the semantic threshold and top-`N` limit
- do not duplicate a skill selected by multiple mechanisms
- report explicitly requested but unavailable skills separately

### Prompt Integration Tests

- main agent prompt includes `Active Skills` with the expected order
- main agent prompt includes `Requested Skills Status` for unavailable explicit matches
- main agent prompt includes a full `Skills` summary
- summary includes availability and missing requirements
- selector failure falls back to summary-only prompt assembly
- subagent prompt includes summary only and no active skill bodies

### Test Infrastructure

- use `tempfile` to build synthetic builtin and workspace skill trees
- avoid depending on repository skill contents for selection behavior tests
- keep tests local and deterministic without network calls

## Risks and Tradeoffs

- Local semantic matching is more predictable than LLM routing, but it will miss some valid matches and occasionally rank borderline matches imperfectly.
- Recomputing the catalog on each prompt keeps behavior fresh and simple, but it may become a performance consideration if the skill tree grows substantially.
- Tolerant parsing improves robustness, but it also means malformed metadata can silently degrade skill quality unless warnings are visible in logs.
- Maintaining a Rust-specific builtin skills tree gives `nanobot-rs` independence, but it creates a second skill catalog that must be curated separately from Python.
- Prompt growth becomes a real constraint once many `always` skills exist, so builtin skill curation matters.

## Acceptance Criteria

- `nanobot-rs` exports a standalone `skills` module.
- `ContextBuilder` no longer performs direct skill directory scanning itself.
- Main agent prompt includes selected active skills using the local priority order `always > explicit > semantic`.
- Main agent prompt still includes a full skills summary with availability details.
- Explicitly requested but unavailable skills are surfaced to the model.
- Workspace skills override builtin skills by name.
- Builtin skills are sourced from `nanobot-rs/skills`, not the Python runtime tree.
- Subagents receive skills summary context but do not auto-inject semantic skills.
- New tests cover discovery, parsing, selection, prompt assembly, and fallback behavior.
