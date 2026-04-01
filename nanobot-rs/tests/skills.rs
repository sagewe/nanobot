use std::fs;

use nanobot_rs::skills::SkillsCatalog;
use tempfile::tempdir;

fn write_skill(root: &std::path::Path, name: &str, content: &str) {
    let skill_dir = root.join(name);
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(skill_dir.join("SKILL.md"), content).expect("write skill");
}

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
    assert!(
        skill
            .missing_requirements
            .contains("CLI: definitely_missing_binary")
    );
    assert!(skill.missing_requirements.contains("ENV: SKILL_TOKEN"));
    assert!(skill.body.contains("Inspect shell commands carefully."));
}
