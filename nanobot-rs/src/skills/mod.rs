use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillRequirements {
    pub bins: Vec<String>,
    pub env: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillMetadata {
    pub always: bool,
    pub requires: SkillRequirements,
    pub keywords: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    Builtin,
    Workspace,
}

#[derive(Debug, Clone)]
pub struct SkillEntry {
    pub name: String,
    pub normalized_name: String,
    pub description: String,
    pub source: SkillSource,
    pub path: PathBuf,
    pub raw_content: String,
    pub body: String,
    pub metadata: SkillMetadata,
    pub available: bool,
    pub missing_requirements: String,
}

#[derive(Debug, Clone, Default)]
pub struct DiscoveredSkills {
    entries: Vec<SkillEntry>,
}

impl DiscoveredSkills {
    pub fn find(&self, name: &str) -> Option<&SkillEntry> {
        let normalized = normalize_skill_name(name);
        self.entries
            .iter()
            .find(|entry| entry.normalized_name == normalized)
    }

    pub fn entries(&self) -> &[SkillEntry] {
        &self.entries
    }
}

#[derive(Debug, Clone)]
pub struct SkillsCatalog {
    workspace: PathBuf,
    builtin_root: PathBuf,
}

impl SkillsCatalog {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            workspace,
            builtin_root: builtin_skills_root(),
        }
    }

    pub fn with_builtin_root(workspace: PathBuf, builtin_root: PathBuf) -> Self {
        Self {
            workspace,
            builtin_root,
        }
    }

    pub fn discover(&self) -> Result<DiscoveredSkills> {
        let mut merged = BTreeMap::new();
        self.collect_root(&self.builtin_root, SkillSource::Builtin, &mut merged)?;
        self.collect_root(
            &self.workspace.join("skills"),
            SkillSource::Workspace,
            &mut merged,
        )?;
        Ok(DiscoveredSkills {
            entries: merged.into_values().collect(),
        })
    }

    fn collect_root(
        &self,
        root: &Path,
        source: SkillSource,
        merged: &mut BTreeMap<String, SkillEntry>,
    ) -> Result<()> {
        if !root.exists() {
            return Ok(());
        }
        let mut dirs = fs::read_dir(root)?
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .collect::<Vec<_>>();
        dirs.sort_by_key(|entry| entry.file_name());
        for entry in dirs {
            let skill_path = entry.path().join("SKILL.md");
            if !skill_path.exists() {
                continue;
            }
            let Some(skill) = load_skill(&skill_path, source)? else {
                continue;
            };
            merged.insert(skill.normalized_name.clone(), skill);
        }
        Ok(())
    }
}

pub fn builtin_skills_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills")
}

pub fn normalize_skill_name(name: &str) -> String {
    let mut normalized = String::new();
    let mut last_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if (ch == '-' || ch == '_' || ch.is_whitespace())
            && !last_dash
            && !normalized.is_empty()
        {
            normalized.push('-');
            last_dash = true;
        }
    }
    normalized.trim_matches('-').to_string()
}

fn load_skill(path: &Path, source: SkillSource) -> Result<Option<SkillEntry>> {
    let raw_content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    let parsed = parse_skill_content(&raw_content);
    let name = parsed
        .meta
        .get("name")
        .cloned()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            path.parent()
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "skill".to_string())
        });
    let normalized_name = normalize_skill_name(&name);
    if normalized_name.is_empty() {
        return Ok(None);
    }

    let metadata = parse_skill_metadata(parsed.meta.get("always"), parsed.meta.get("metadata"));
    let description = parsed
        .meta
        .get("description")
        .cloned()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| name.clone());
    let missing_requirements = collect_missing_requirements(&metadata);
    Ok(Some(SkillEntry {
        name,
        normalized_name,
        description,
        source,
        path: path.to_path_buf(),
        raw_content,
        body: parsed.body,
        metadata,
        available: missing_requirements.is_empty(),
        missing_requirements,
    }))
}

#[derive(Debug, Default)]
struct ParsedSkillContent {
    meta: BTreeMap<String, String>,
    body: String,
}

fn parse_skill_content(content: &str) -> ParsedSkillContent {
    if let Some(rest) = content.strip_prefix("---\n")
        && let Some((frontmatter, body)) = rest.split_once("\n---\n")
    {
        let mut meta = BTreeMap::new();
        for line in frontmatter.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            meta.insert(key.trim().to_string(), trim_wrapped_quotes(value.trim()));
        }
        return ParsedSkillContent {
            meta,
            body: body.trim().to_string(),
        };
    }
    ParsedSkillContent {
        meta: BTreeMap::new(),
        body: content.trim().to_string(),
    }
}

fn parse_skill_metadata(always: Option<&String>, metadata_json: Option<&String>) -> SkillMetadata {
    let mut metadata = SkillMetadata {
        always: always.is_some_and(|value| value.eq_ignore_ascii_case("true")),
        ..SkillMetadata::default()
    };
    let Some(metadata_json) = metadata_json else {
        return metadata;
    };
    let Ok(value) = serde_json::from_str::<Value>(metadata_json) else {
        return metadata;
    };
    let Some(root) = value
        .get("nanobot")
        .or_else(|| value.get("openclaw"))
        .and_then(Value::as_object)
    else {
        return metadata;
    };

    if let Some(always) = root.get("always").and_then(Value::as_bool) {
        metadata.always = always;
    }
    if let Some(requires) = root.get("requires").and_then(Value::as_object) {
        metadata.requires.bins = requires
            .get("bins")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(ToString::to_string)
            .collect();
        metadata.requires.env = requires
            .get("env")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(ToString::to_string)
            .collect();
    }
    metadata.keywords = root
        .get("keywords")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .collect();
    metadata.tags = root
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .collect();
    metadata
}

fn collect_missing_requirements(metadata: &SkillMetadata) -> String {
    let mut missing = Vec::new();
    for bin in &metadata.requires.bins {
        if !binary_exists(bin) {
            missing.push(format!("CLI: {bin}"));
        }
    }
    for env_name in &metadata.requires.env {
        if env::var_os(env_name).is_none() {
            missing.push(format!("ENV: {env_name}"));
        }
    }
    missing.join(", ")
}

fn binary_exists(bin: &str) -> bool {
    if bin.contains(std::path::MAIN_SEPARATOR) {
        return Path::new(bin).is_file();
    }
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };
    let candidates = executable_candidates(bin);
    env::split_paths(&paths).any(|dir| {
        candidates
            .iter()
            .any(|candidate| dir.join(candidate).is_file())
    })
}

fn executable_candidates(bin: &str) -> Vec<OsString> {
    #[cfg(windows)]
    {
        let pathext =
            env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
        let exts = pathext
            .to_string_lossy()
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(|ext| OsString::from(format!("{bin}{ext}")))
            .collect::<Vec<_>>();
        if Path::new(bin).extension().is_some() {
            vec![OsString::from(bin)]
        } else {
            let mut values = vec![OsString::from(bin)];
            values.extend(exts);
            values
        }
    }
    #[cfg(not(windows))]
    {
        vec![OsString::from(bin)]
    }
}

fn trim_wrapped_quotes(value: &str) -> String {
    value
        .trim()
        .trim_matches(|ch| ch == '"' || ch == '\'')
        .to_string()
}
