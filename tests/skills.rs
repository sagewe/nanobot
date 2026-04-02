use std::fs;

use sidekick::skills::{SelectionReason, SkillSelector, SkillsCatalog};
use tempfile::tempdir;

fn write_skill(root: &std::path::Path, name: &str, content: &str) {
    let skill_dir = root.join(name);
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(skill_dir.join("SKILL.md"), content).expect("write skill");
}

const FRONTMATTER_ALWAYS: &str = r#"---
name: guard
description: always-on guardrails
always: true
---

# Guard

Always apply guardrails.
"#;

const FRONTMATTER_WEATHER: &str = r#"---
name: weather
description: check rain forecasts
---

# Weather

Use weather tools to check rain and forecast conditions.
"#;

const FRONTMATTER_TMUX: &str = r#"---
name: tmux
description: manage tmux sessions
metadata: '{"sidekick":{"keywords":["attach","session","terminal"]}}'
---

# Tmux

Attach to sessions and inspect terminal state.
"#;

const UNAVAILABLE_DEPLOY_SKILL: &str = r#"---
name: deploy
description: deploy services
metadata: '{"sidekick":{"requires":{"bins":["definitely_missing_binary_for_selector_test"]}}}'
---

# Deploy

Roll out services safely.
"#;

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
    assert!(
        weather
            .path
            .ends_with("workspace/skills/weather_tool/SKILL.md")
    );
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
metadata: '{"sidekick":{"requires":{"bins":["definitely_missing_binary"],"env":["SKILL_TOKEN"]}}}'
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
    assert!(
        skill
            .missing_requirements
            .contains("CLI: definitely_missing_binary")
    );
    assert!(skill.missing_requirements.contains("ENV: SKILL_TOKEN"));
    assert!(skill.body.contains("Inspect shell commands carefully."));
}

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
        .select(
            &catalog,
            "Use $weather to check rain, then help me attach to a tmux session",
        )
        .expect("select");

    let names = selected
        .active
        .iter()
        .map(|skill| skill.entry.name.as_str())
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
    assert!(
        selected
            .render_requested_status()
            .contains("CLI: definitely_missing_binary_for_selector_test")
    );
}

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
    fs::create_dir_all(workspace.join(".sidekick")).expect("state dir");
    fs::write(
        workspace.join(".sidekick/skills-state.json"),
        r#"{"weather":{"enabled":false}}"#,
    )
    .expect("state file");

    let managed = SkillsCatalog::with_builtin_root(workspace.clone(), builtin_root)
        .discover_managed()
        .expect("managed catalog");

    let workspace_skill = managed
        .workspace
        .iter()
        .find(|skill| skill.id == "weather")
        .expect("workspace skill");
    let builtin_skill = managed
        .builtin
        .iter()
        .find(|skill| skill.id == "weather")
        .expect("builtin skill");
    let discovered = SkillsCatalog::with_builtin_root(workspace, temp.path().join("builtin"))
        .discover()
        .expect("discover");
    let effective_skill = discovered.find("weather").expect("effective weather");

    assert!(!workspace_skill.enabled);
    assert!(!workspace_skill.effective);
    assert!(builtin_skill.effective);
    assert_eq!(effective_skill.description, "builtin weather");
    assert!(effective_skill.path.ends_with("builtin/weather/SKILL.md"));
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
    fs::write(workspace.join("skills/release-check/notes.txt"), "extra").expect("extra file");

    let managed = SkillsCatalog::with_builtin_root(workspace, temp.path().join("builtin"))
        .discover_managed()
        .expect("managed catalog");

    let skill = managed
        .workspace
        .iter()
        .find(|skill| skill.id == "release-check")
        .expect("workspace skill");

    assert_eq!(skill.entry.name, "Release Checklist");
    assert!(skill.has_extra_files);
}

#[test]
fn discover_ignores_malformed_state_and_keeps_workspace_skill_effective() {
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
    fs::create_dir_all(workspace.join(".sidekick")).expect("state dir");
    fs::write(
        workspace.join(".sidekick/skills-state.json"),
        r#"{"weather":not-json}"#,
    )
    .expect("state file");

    let catalog = SkillsCatalog::with_builtin_root(workspace, builtin_root)
        .discover()
        .expect("discover");

    let weather = catalog.find("weather").expect("weather skill");
    assert_eq!(weather.description, "workspace weather");
    assert!(weather.path.ends_with("workspace/skills/weather/SKILL.md"));
}

#[test]
fn discover_ignores_unreadable_state_and_keeps_builtin_skill_effective() {
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
    fs::create_dir_all(workspace.join(".sidekick/skills-state.json")).expect("state dir");

    let catalog = SkillsCatalog::with_builtin_root(workspace, builtin_root)
        .discover()
        .expect("discover");

    let weather = catalog.find("weather").expect("weather skill");
    assert_eq!(weather.description, "builtin weather");
    assert!(weather.path.ends_with("builtin/weather/SKILL.md"));
}

#[test]
fn discover_reads_legacy_nanobot_state_when_sidekick_state_is_missing() {
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
    fs::create_dir_all(workspace.join(".nanobot")).expect("legacy state dir");
    fs::write(
        workspace.join(".nanobot/skills-state.json"),
        r#"{"weather":{"enabled":false}}"#,
    )
    .expect("legacy state file");

    let managed = SkillsCatalog::with_builtin_root(workspace, builtin_root)
        .discover_managed()
        .expect("managed catalog");

    let workspace_skill = managed
        .workspace
        .iter()
        .find(|skill| skill.id == "weather")
        .expect("workspace skill");
    let builtin_skill = managed
        .builtin
        .iter()
        .find(|skill| skill.id == "weather")
        .expect("builtin skill");

    assert!(!workspace_skill.enabled);
    assert!(builtin_skill.effective);
}
