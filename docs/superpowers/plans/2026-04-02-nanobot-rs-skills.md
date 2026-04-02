# nanobot-rs Skills Module Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a first-class `nanobot-rs` skills module that discovers builtin and workspace skills, selects active skills locally, injects them into main-agent prompts, and keeps builtin skill files readable even when workspace path restrictions are enabled.

**Architecture:** Build a standalone Rust `skills` module that owns catalog discovery, tolerant frontmatter parsing, requirement checks, explicit and semantic selection, and prompt section rendering. Integrate that module into `ContextBuilder` for main-agent prompt assembly, keep subagents summary-only, and extend read-only file browsing so builtin `SKILL.md` files remain inspectable without relaxing write restrictions outside the workspace.

**Tech Stack:** Rust (`std`, `anyhow`, `regex`, `serde_json`), existing `nanobot-rs` agent/tools stack, `tempfile` integration tests.

---

## File Map

### Rust Modules

- Create: `/Users/sage/nanobot/nanobot-rs/src/skills/mod.rs`
  - Skill discovery, metadata parsing, availability checks, local selection, and prompt rendering.
- Modify: `/Users/sage/nanobot/nanobot-rs/src/lib.rs`
  - Export the new `skills` module.
- Modify: `/Users/sage/nanobot/nanobot-rs/src/agent/mod.rs`
  - Replace ad hoc workspace skill scanning with the new catalog and selector.
  - Add a summary-only subagent prompt helper.
- Modify: `/Users/sage/nanobot/nanobot-rs/src/tools/mod.rs`
  - Let `read_file` and `list_dir` allow a read-only builtin skills root even when workspace restriction is enabled.

### Builtin Skills Tree

- Create: `/Users/sage/nanobot/nanobot-rs/skills/README.md`
  - Document the Rust builtin skills directory contract and naming conventions.

### Tests

- Create: `/Users/sage/nanobot/nanobot-rs/tests/skills.rs`
  - Discovery, parsing, selection, and prompt-section rendering tests.
- Modify: `/Users/sage/nanobot/nanobot-rs/tests/agent.rs`
  - Main-agent and subagent prompt integration tests.
- Modify: `/Users/sage/nanobot/nanobot-rs/tests/tools.rs`
  - Builtin skill root readability tests under `restrict_to_workspace=true`.

### Reference Spec

- Read: `/Users/sage/nanobot/nanobot-rs/docs/superpowers/specs/2026-04-02-nanobot-rs-skills-design.md`

## Task 1: Add the Skills Catalog, Parsing, and Availability Model

**Files:**
- Create: `/Users/sage/nanobot/nanobot-rs/src/skills/mod.rs`
- Modify: `/Users/sage/nanobot/nanobot-rs/src/lib.rs`
- Create: `/Users/sage/nanobot/nanobot-rs/tests/skills.rs`
- Create: `/Users/sage/nanobot/nanobot-rs/skills/README.md`

- [ ] **Step 1: Write the failing catalog tests**

```rust
#[test]
fn catalog_prefers_workspace_skill_over_builtin_by_normalized_name() {
    let temp = tempdir().expect("tempdir");
    let builtin_root = temp.path().join("builtin");
    let workspace = temp.path().join("workspace");
    write_skill(
        &builtin_root,
        "Weather-Tool",
        r#"---
name: weather-tool
description: builtin weather
---

Builtin weather body
"#,
    );
    write_skill(
        &workspace.join("skills"),
        "weather_tool",
        r#"---
name: weather_tool
description: workspace weather
---

Workspace weather body
"#,
    );

    let catalog = SkillsCatalog::with_builtin_root(workspace.clone(), builtin_root)
        .discover()
        .expect("discover");

    let weather = catalog.find("weather-tool").expect("weather skill");
    assert_eq!(weather.description, "workspace weather");
    assert!(weather.path.ends_with("workspace/skills/weather_tool/SKILL.md"));
}

#[test]
fn catalog_parses_frontmatter_and_requirement_state() {
    let temp = tempdir().expect("tempdir");
    let builtin_root = temp.path().join("builtin");
    write_skill(
        &builtin_root,
        "shell-check",
        r#"---
name: shell-check
description: verify shell commands
always: true
metadata: '{"nanobot":{"requires":{"bins":["definitely_missing_binary"],"env":["SKILL_TOKEN"]}}}'
---

# Shell Check

Inspect shell commands carefully.
"#,
    );

    let catalog = SkillsCatalog::with_builtin_root(temp.path().join("workspace"), builtin_root)
        .discover()
        .expect("discover");

    let skill = catalog.find("shell-check").expect("skill");
    assert!(skill.metadata.always);
    assert!(!skill.available);
    assert!(skill.missing_requirements.contains("CLI: definitely_missing_binary"));
    assert!(skill.missing_requirements.contains("ENV: SKILL_TOKEN"));
    assert!(skill.body.contains("Inspect shell commands carefully."));
}
```

- [ ] **Step 2: Run the new skills tests and confirm they fail**

Run: `cargo test --test skills`
Expected: FAIL because `nanobot-rs` does not yet expose a `skills` module, catalog type, or tolerant frontmatter parsing.

- [ ] **Step 3: Implement the catalog and metadata model**

```rust
pub struct SkillsCatalog {
    workspace: PathBuf,
    builtin_root: PathBuf,
}

impl SkillsCatalog {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            workspace,
            builtin_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills"),
        }
    }

    pub fn with_builtin_root(workspace: PathBuf, builtin_root: PathBuf) -> Self {
        Self { workspace, builtin_root }
    }

    pub fn discover(&self) -> Result<DiscoveredSkills> {
        // Scan builtin root and workspace/skills, normalize names, let workspace win.
    }
}
```

Implementation notes:
- normalize names with lowercase plus separator folding for `-`, `_`, and spaces
- parse frontmatter only when the file begins with `---`
- keep parsing tolerant: frontmatter or metadata JSON failure should not discard the whole skill
- compute `available` and `missing_requirements` from `requires.bins` and `requires.env`
- strip frontmatter from the body before storing it for prompt injection
- keep builtin root file-backed using `env!("CARGO_MANIFEST_DIR")/skills` so builtin `SKILL.md` files remain readable through file tools in source-based workflows
- add `skills/README.md` documenting the builtin directory contract

- [ ] **Step 4: Re-run the skills tests**

Run: `cargo test --test skills`
Expected: PASS for catalog discovery, workspace override, and availability parsing.

- [ ] **Step 5: Commit the catalog foundation**

```bash
git add src/skills/mod.rs src/lib.rs tests/skills.rs skills/README.md
git commit -m "feat: add skills catalog foundation"
```

## Task 2: Implement Local Skill Selection and Prompt Section Rendering

**Files:**
- Modify: `/Users/sage/nanobot/nanobot-rs/src/skills/mod.rs`
- Modify: `/Users/sage/nanobot/nanobot-rs/tests/skills.rs`

- [ ] **Step 1: Write the failing selector tests**

```rust
#[test]
fn selector_orders_always_explicit_and_semantic_matches() {
    let temp = tempdir().expect("tempdir");
    let builtin_root = temp.path().join("builtin");
    let workspace = temp.path().join("workspace");
    write_skill(&builtin_root, "guard", FRONTMATTER_ALWAYS);
    write_skill(&builtin_root, "weather", FRONTMATTER_WEATHER);
    write_skill(&builtin_root, "tmux", FRONTMATTER_TMUX);

    let catalog = SkillsCatalog::with_builtin_root(workspace, builtin_root)
        .discover()
        .expect("discover");
    let selected = SkillSelector::default()
        .select(&catalog, "Use $weather to check rain, then help me attach to a tmux session")
        .expect("select");

    let names = selected
        .active
        .iter()
        .map(|skill| skill.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["guard", "weather", "tmux"]);
    assert_eq!(selected.active[0].reason, SelectionReason::Always);
    assert_eq!(selected.active[1].reason, SelectionReason::Explicit);
    assert_eq!(selected.active[2].reason, SelectionReason::Semantic);
}

#[test]
fn selector_reports_unavailable_explicit_requests() {
    let temp = tempdir().expect("tempdir");
    let builtin_root = temp.path().join("builtin");
    write_skill(&builtin_root, "deploy", UNAVAILABLE_DEPLOY_SKILL);

    let catalog = SkillsCatalog::with_builtin_root(temp.path().join("workspace"), builtin_root)
        .discover()
        .expect("discover");
    let selected = SkillSelector::default()
        .select(&catalog, "Please use $deploy")
        .expect("select");

    assert!(selected.active.is_empty());
    assert_eq!(selected.requested_unavailable.len(), 1);
    assert!(selected.render_requested_status().contains("CLI: kubectl"));
}
```

- [ ] **Step 2: Run the selector tests and confirm they fail**

Run: `cargo test --test skills`
Expected: FAIL because there is no `SkillSelector`, no priority ordering, and no rendered status sections.

- [ ] **Step 3: Implement selection and rendering helpers**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionReason {
    Always,
    Explicit,
    Semantic,
}

pub struct SkillSelector {
    semantic_limit: usize,
    semantic_threshold: usize,
}

impl SkillSelector {
    pub fn select(&self, catalog: &DiscoveredSkills, message: &str) -> Result<SelectedSkills> {
        // 1. available always skills
        // 2. explicit hits from $skill-name, backticks, and normalized exact name matches
        // 3. top-N semantic matches from token overlap
    }
}
```

Implementation notes:
- deduplicate by normalized skill name while preserving the earliest reason
- ignore one-character tokens and a small set of stop-like tokens in semantic matching
- require more than one meaningful token overlap for semantic selection
- keep rendering separate:
  - `render_active_skills()`
  - `render_requested_status()`
  - `render_catalog_summary()`
- keep summary output deterministic by sorting discovered skills by normalized name after override resolution

- [ ] **Step 4: Re-run the selector tests**

Run: `cargo test --test skills`
Expected: PASS with deterministic selection order and unavailable explicit request reporting.

- [ ] **Step 5: Commit the selector layer**

```bash
git add src/skills/mod.rs tests/skills.rs
git commit -m "feat: add local skill selection"
```

## Task 3: Inject Active Skills into Main-Agent Prompts and Keep Subagents Summary-Only

**Files:**
- Modify: `/Users/sage/nanobot/nanobot-rs/src/agent/mod.rs`
- Modify: `/Users/sage/nanobot/nanobot-rs/tests/agent.rs`

- [ ] **Step 1: Write the failing prompt integration tests**

```rust
#[test]
fn context_builder_includes_active_skills_requested_status_and_summary() {
    let temp = tempdir().expect("tempdir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(workspace.join("skills")).expect("workspace skills");
    write_skill(&workspace.join("skills"), "always-on", ALWAYS_SKILL);
    write_skill(&workspace.join("skills"), "weather", WEATHER_SKILL);
    write_skill(&workspace.join("skills"), "deploy", UNAVAILABLE_DEPLOY_SKILL);

    let context = ContextBuilder::new(workspace.clone());
    let prompt = context.build_system_prompt("use $weather and $deploy to check conditions");

    assert!(prompt.contains("## Active Skills"));
    assert!(prompt.contains("### Skill: always-on"));
    assert!(prompt.contains("### Skill: weather"));
    assert!(prompt.contains("## Requested Skills Status"));
    assert!(prompt.contains("deploy"));
    assert!(prompt.contains("## Skills"));
}

#[tokio::test]
async fn subagent_prompt_only_includes_skills_summary() {
    let dir = tempdir().expect("tempdir");
    write_skill(&dir.path().join("skills"), "weather", WEATHER_SKILL);
    let provider = capturing_provider();
    let manager = SubagentManager::new(
        provider.clone(),
        dir.path().to_path_buf(),
        MessageBus::new(8),
        "mock-model".to_string(),
        2,
        10,
        false,
        WebToolsConfig::default(),
    );

    let _ = manager
        .spawn("check rain".to_string(), Some("weather".to_string()), "cli".to_string(), "test".to_string())
        .await;

    let system_prompt = provider.first_system_prompt().await;
    assert!(system_prompt.contains("## Skills"));
    assert!(!system_prompt.contains("## Active Skills"));
}
```

- [ ] **Step 2: Run the targeted agent tests and confirm they fail**

Run: `cargo test --test agent`
Expected: FAIL because `ContextBuilder::build_system_prompt()` still takes no user message, only lists workspace skill paths, and subagent prompts do not use the shared summary renderer.

- [ ] **Step 3: Integrate the skills module into prompt construction**

```rust
impl ContextBuilder {
    pub fn build_system_prompt(&self, current_message: &str) -> String {
        let catalog = SkillsCatalog::new(self.workspace.clone()).discover();
        let mut parts = vec![self.identity_prompt(), self.bootstrap_prompt()];

        if let Ok(catalog) = catalog {
            let selected = SkillSelector::default().select(&catalog, current_message);
            if let Ok(selected) = selected {
                if let Some(active) = selected.render_active_skills() {
                    parts.push(format!("## Active Skills\n\n{active}"));
                }
                if let Some(status) = selected.render_requested_status() {
                    parts.push(format!("## Requested Skills Status\n\n{status}"));
                }
                if let Some(summary) = catalog.render_summary() {
                    parts.push(format!("## Skills\n\n{summary}"));
                }
            }
        }

        parts.join("\n\n---\n\n")
    }
}
```

Implementation notes:
- thread `current_message` through `build_messages()` and all direct call sites
- keep fallback behavior: if discovery or selection fails, still emit summary-only when possible
- extract a reusable subagent prompt builder method so tests can assert its output through the provider capture path
- keep subagent prompts summary-only even when the task text would semantically match a skill

- [ ] **Step 4: Re-run the agent tests**

Run: `cargo test --test agent`
Expected: PASS with main-agent skill injection and summary-only subagent prompts.

- [ ] **Step 5: Commit the prompt integration**

```bash
git add src/agent/mod.rs tests/agent.rs
git commit -m "feat: inject skills into agent prompts"
```

## Task 4: Keep Builtin Skill Files Readable When Workspace Restriction Is Enabled

**Files:**
- Modify: `/Users/sage/nanobot/nanobot-rs/src/tools/mod.rs`
- Modify: `/Users/sage/nanobot/nanobot-rs/src/skills/mod.rs`
- Modify: `/Users/sage/nanobot/nanobot-rs/tests/tools.rs`

- [ ] **Step 1: Write the failing tool restriction tests**

```rust
#[tokio::test]
async fn read_file_allows_builtin_skill_root_when_workspace_is_restricted() {
    let dir = tempdir().expect("tempdir");
    let workspace = dir.path().join("workspace");
    let builtin_root = dir.path().join("builtin");
    std::fs::create_dir_all(&workspace).expect("workspace");
    write_skill(&builtin_root, "weather", WEATHER_SKILL);

    let tool = ReadFileTool::with_additional_roots(
        workspace.clone(),
        true,
        vec![builtin_root.clone()],
    );
    let result = tool
        .execute(json!({"path": builtin_root.join("weather").join("SKILL.md").display().to_string()}))
        .await;

    assert!(result.contains("Weather"));
}

#[tokio::test]
async fn list_dir_allows_builtin_skill_root_when_workspace_is_restricted() {
    let dir = tempdir().expect("tempdir");
    let workspace = dir.path().join("workspace");
    let builtin_root = dir.path().join("builtin");
    std::fs::create_dir_all(&workspace).expect("workspace");
    write_skill(&builtin_root, "weather", WEATHER_SKILL);

    let tool = ListDirTool::with_additional_roots(
        workspace.clone(),
        true,
        vec![builtin_root.clone()],
    );
    let result = tool
        .execute(json!({"path": builtin_root.display().to_string(), "recursive": true}))
        .await;

    assert!(result.contains("weather/SKILL.md"));
}
```

- [ ] **Step 2: Run the targeted tools tests and confirm they fail**

Run: `cargo test --test tools`
Expected: FAIL because `resolve_path()` only allows the workspace root when restriction is enabled.

- [ ] **Step 3: Add a read-only allowlist for builtin skills**

```rust
fn resolve_path_with_read_roots(
    path: &str,
    workspace: &Path,
    restrict: bool,
    extra_read_roots: &[PathBuf],
) -> anyhow::Result<PathBuf> {
    let resolved = canonicalize_like_workspace(path, workspace)?;
    if !restrict {
        return Ok(resolved);
    }
    if resolved.starts_with(workspace) || extra_read_roots.iter().any(|root| resolved.starts_with(root)) {
        return Ok(resolved);
    }
    anyhow::bail!("Path {path:?} is outside allowed roots");
}
```

Implementation notes:
- apply the allowlist to `ReadFileTool` and `ListDirTool` only
- keep `WriteFileTool`, `EditFileTool`, and `ExecTool` unchanged
- expose a helper from `skills::mod` to compute the builtin skills root for production wiring
- update `build_default_tools()` to pass the builtin root into read-only tools automatically

- [ ] **Step 4: Re-run the tools tests**

Run: `cargo test --test tools`
Expected: PASS with builtin skill reads allowed and write restrictions unchanged.

- [ ] **Step 5: Commit the read-only root change**

```bash
git add src/tools/mod.rs src/skills/mod.rs tests/tools.rs
git commit -m "fix: allow reading builtin skills outside workspace root"
```

## Task 5: Run Focused Verification and Close the Loop

**Files:**
- Modify if needed: `/Users/sage/nanobot/nanobot-rs/tests/skills.rs`
- Modify if needed: `/Users/sage/nanobot/nanobot-rs/tests/agent.rs`
- Modify if needed: `/Users/sage/nanobot/nanobot-rs/tests/tools.rs`

- [ ] **Step 1: Run the focused Rust test suite**

Run: `cargo test --test skills --test agent --test tools`
Expected: PASS with the new skills module, prompt integration, and read-only builtin root support covered together.

- [ ] **Step 2: Run the full Rust test suite**

Run: `cargo test`
Expected: PASS without regressions in agent, channel, provider, or web tests.

- [ ] **Step 3: Fix any test fallout with minimal edits**

```rust
// Keep fallout fixes local:
// - update prompt assertions if section ordering changed
// - update constructor call sites if tool signatures changed
// - do not expand scope beyond skills integration
```

- [ ] **Step 4: Re-run the full Rust test suite**

Run: `cargo test`
Expected: PASS cleanly after any fallout fixes.

- [ ] **Step 5: Commit the verification sweep**

```bash
git add tests/skills.rs tests/agent.rs tests/tools.rs src/agent/mod.rs src/tools/mod.rs src/skills/mod.rs
git commit -m "test: cover nanobot-rs skills integration"
```
