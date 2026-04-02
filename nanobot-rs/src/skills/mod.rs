use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

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
    pub search_text: String,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone, Default)]
pub struct ManagedSkills {
    pub workspace: Vec<ManagedSkillEntry>,
    pub builtin: Vec<ManagedSkillEntry>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
struct ManagedSkillState {
    #[serde(default = "default_enabled")]
    enabled: bool,
}

fn default_enabled() -> bool {
    true
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

    pub fn render_summary(&self) -> String {
        self.entries
            .iter()
            .map(|entry| {
                let source = match entry.source {
                    SkillSource::Builtin => "builtin",
                    SkillSource::Workspace => "workspace",
                };
                if entry.available {
                    format!(
                        "- {}: {} [{}] ({})",
                        entry.name,
                        entry.description,
                        source,
                        entry.path.display()
                    )
                } else {
                    format!(
                        "- {}: {} [{}] ({}) unavailable: {}",
                        entry.name,
                        entry.description,
                        source,
                        entry.path.display(),
                        entry.missing_requirements
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl ManagedSkills {
    fn into_discovered(self) -> DiscoveredSkills {
        let mut merged = BTreeMap::new();
        for skill in self
            .builtin
            .into_iter()
            .chain(self.workspace.into_iter())
            .filter(|skill| skill.effective)
        {
            merged.insert(skill.entry.normalized_name.clone(), skill.entry);
        }
        DiscoveredSkills {
            entries: merged.into_values().collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionReason {
    Always,
    Explicit,
    Semantic,
}

#[derive(Debug, Clone)]
pub struct SelectedSkill {
    pub entry: SkillEntry,
    pub reason: SelectionReason,
}

#[derive(Debug, Clone)]
pub struct RequestedSkillStatus {
    pub name: String,
    pub missing_requirements: String,
}

#[derive(Debug, Clone, Default)]
pub struct SelectedSkills {
    pub active: Vec<SelectedSkill>,
    pub requested_unavailable: Vec<RequestedSkillStatus>,
}

impl SelectedSkills {
    pub fn render_active_skills(&self) -> String {
        self.active
            .iter()
            .map(|selected| {
                format!(
                    "### Skill: {}\n\n{}",
                    selected.entry.name, selected.entry.body
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn render_requested_status(&self) -> String {
        self.requested_unavailable
            .iter()
            .map(|status| {
                if status.missing_requirements.is_empty() {
                    format!("- {}: unavailable", status.name)
                } else {
                    format!(
                        "- {}: unavailable ({})",
                        status.name, status.missing_requirements
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone)]
pub struct SkillSelector {
    semantic_limit: usize,
    semantic_threshold: usize,
}

impl Default for SkillSelector {
    fn default() -> Self {
        Self {
            semantic_limit: 3,
            semantic_threshold: 2,
        }
    }
}

impl SkillSelector {
    pub fn select(&self, catalog: &DiscoveredSkills, message: &str) -> Result<SelectedSkills> {
        let mut selected = SelectedSkills::default();
        let mut seen = HashSet::new();

        for entry in catalog.entries() {
            if entry.metadata.always
                && entry.available
                && seen.insert(entry.normalized_name.clone())
            {
                selected.active.push(SelectedSkill {
                    entry: entry.clone(),
                    reason: SelectionReason::Always,
                });
            }
        }

        for explicit_name in explicit_mentions(message) {
            let Some(entry) = catalog.find(&explicit_name) else {
                continue;
            };
            if entry.available {
                if seen.insert(entry.normalized_name.clone()) {
                    selected.active.push(SelectedSkill {
                        entry: entry.clone(),
                        reason: SelectionReason::Explicit,
                    });
                }
            } else {
                selected.requested_unavailable.push(RequestedSkillStatus {
                    name: entry.name.clone(),
                    missing_requirements: entry.missing_requirements.clone(),
                });
            }
        }

        let message_tokens = semantic_tokens(message);
        let mut semantic_matches = catalog
            .entries()
            .iter()
            .filter(|entry| entry.available && !seen.contains(&entry.normalized_name))
            .filter_map(|entry| {
                let score = semantic_score(&message_tokens, entry);
                (score >= self.semantic_threshold).then_some((score, entry))
            })
            .collect::<Vec<_>>();
        semantic_matches.sort_by(|(score_a, entry_a), (score_b, entry_b)| {
            score_b
                .cmp(score_a)
                .then_with(|| entry_a.normalized_name.cmp(&entry_b.normalized_name))
        });

        for (_, entry) in semantic_matches.into_iter().take(self.semantic_limit) {
            if seen.insert(entry.normalized_name.clone()) {
                selected.active.push(SelectedSkill {
                    entry: entry.clone(),
                    reason: SelectionReason::Semantic,
                });
            }
        }

        Ok(selected)
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
        Ok(self.discover_managed()?.into_discovered())
    }

    pub fn discover_managed(&self) -> Result<ManagedSkills> {
        let state = load_managed_state(&self.managed_state_path())?;
        let mut builtin = self.collect_root_managed(&self.builtin_root, SkillSource::Builtin)?;
        let mut workspace =
            self.collect_root_managed(&self.workspace.join("skills"), SkillSource::Workspace)?;

        let mut effective_by_name = BTreeMap::<String, (SkillSource, String)>::new();
        let mut workspace_by_name: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut builtin_by_name: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut workspace_enabled_by_name: BTreeMap<String, bool> = BTreeMap::new();
        let mut builtin_present_by_name: BTreeMap<String, bool> = BTreeMap::new();

        for (index, skill) in workspace.iter_mut().enumerate() {
            skill.enabled = state.get(&skill.id).map_or(true, |state| state.enabled);
            workspace_by_name
                .entry(skill.entry.normalized_name.clone())
                .or_default()
                .push(index);
            if skill.enabled {
                workspace_enabled_by_name.insert(skill.entry.normalized_name.clone(), true);
            }
        }
        for (index, skill) in builtin.iter_mut().enumerate() {
            builtin_by_name
                .entry(skill.entry.normalized_name.clone())
                .or_default()
                .push(index);
            builtin_present_by_name.insert(skill.entry.normalized_name.clone(), true);
        }

        for (normalized_name, workspace_indexes) in &workspace_by_name {
            let enabled_workspace_indexes = workspace_indexes
                .iter()
                .copied()
                .filter(|index| workspace[*index].enabled)
                .collect::<Vec<_>>();
            if let Some(index) = enabled_workspace_indexes.last().copied() {
                effective_by_name.insert(
                    normalized_name.clone(),
                    (SkillSource::Workspace, workspace[index].id.clone()),
                );
            }
        }

        for (normalized_name, builtin_indexes) in &builtin_by_name {
            if effective_by_name.contains_key(normalized_name) {
                continue;
            }
            if let Some(index) = builtin_indexes.last().copied() {
                effective_by_name.insert(
                    normalized_name.clone(),
                    (SkillSource::Builtin, builtin[index].id.clone()),
                );
            }
        }

        for skill in &mut workspace {
            let is_effective = effective_by_name
                .get(&skill.entry.normalized_name)
                .is_some_and(|(source, id)| *source == SkillSource::Workspace && *id == skill.id);
            skill.effective = is_effective;
            skill.overrides_builtin = is_effective
                && builtin_present_by_name
                    .get(&skill.entry.normalized_name)
                    .copied()
                    .unwrap_or(false);
            skill.shadowed_by_workspace = false;
        }

        for skill in &mut builtin {
            let is_effective = effective_by_name
                .get(&skill.entry.normalized_name)
                .is_some_and(|(source, id)| *source == SkillSource::Builtin && *id == skill.id);
            skill.effective = is_effective;
            skill.shadowed_by_workspace = workspace_enabled_by_name
                .get(&skill.entry.normalized_name)
                .copied()
                .unwrap_or(false);
            skill.overrides_builtin = false;
        }

        workspace.sort_by(|left, right| left.id.cmp(&right.id));
        builtin.sort_by(|left, right| left.id.cmp(&right.id));

        Ok(ManagedSkills { workspace, builtin })
    }

    fn collect_root_managed(
        &self,
        root: &Path,
        source: SkillSource,
    ) -> Result<Vec<ManagedSkillEntry>> {
        if !root.exists() {
            return Ok(Vec::new());
        }
        let mut dirs = fs::read_dir(root)?
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .collect::<Vec<_>>();
        dirs.sort_by_key(|entry| entry.file_name());
        let mut entries = Vec::new();
        for entry in dirs {
            let skill_path = entry.path().join("SKILL.md");
            if !skill_path.exists() {
                continue;
            }
            let Some(skill) = load_skill(&skill_path, source)? else {
                continue;
            };
            let id = entry.file_name().to_string_lossy().into_owned();
            let has_extra_files = skill_dir_has_extra_files(&entry.path());
            entries.push(ManagedSkillEntry {
                id,
                source,
                enabled: true,
                effective: false,
                overrides_builtin: false,
                shadowed_by_workspace: false,
                has_extra_files,
                entry: skill,
            });
        }
        Ok(entries)
    }

    fn managed_state_path(&self) -> PathBuf {
        self.workspace.join(".nanobot").join("skills-state.json")
    }
}

fn load_managed_state(path: &Path) -> Result<BTreeMap<String, ManagedSkillState>> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(BTreeMap::new());
        }
        Err(error) => {
            warn!(
                path = %path.display(),
                error = %error,
                "skipping unreadable skills state"
            );
            return Ok(BTreeMap::new());
        }
    };
    match serde_json::from_str(&raw) {
        Ok(state) => Ok(state),
        Err(error) => {
            warn!(
                path = %path.display(),
                error = %error,
                "skipping malformed skills state"
            );
            Ok(BTreeMap::new())
        }
    }
}

fn skill_dir_has_extra_files(path: &Path) -> bool {
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .path()
            .file_name()
            .is_none_or(|name| name != OsStr::new("SKILL.md"))
    })
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
    let search_text = build_search_text(path, parsed.meta.get("description"), &parsed.body);
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
        search_text,
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

fn build_search_text(path: &Path, description: Option<&String>, body: &str) -> String {
    let directory_name = path
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    [
        directory_name,
        description.cloned().unwrap_or_default(),
        body.to_string(),
    ]
    .join("\n")
}

fn explicit_mentions(message: &str) -> Vec<String> {
    let mut mentions = Vec::new();
    let mut seen = HashSet::new();
    let dollar = Regex::new(r"\$([A-Za-z0-9][A-Za-z0-9_\-]*)").expect("dollar regex");
    for capture in dollar.captures_iter(message) {
        let normalized = normalize_skill_name(&capture[1]);
        if !normalized.is_empty() && seen.insert(normalized.clone()) {
            mentions.push(normalized);
        }
    }
    let backticks = Regex::new(r"`([^`]+)`").expect("backtick regex");
    for capture in backticks.captures_iter(message) {
        let normalized = normalize_skill_name(&capture[1]);
        if !normalized.is_empty() && seen.insert(normalized.clone()) {
            mentions.push(normalized);
        }
    }
    mentions
}

fn semantic_score(message_tokens: &BTreeSet<String>, entry: &SkillEntry) -> usize {
    let entry_tokens = semantic_tokens(&entry.search_text);
    message_tokens.intersection(&entry_tokens).count()
}

fn semantic_tokens(text: &str) -> BTreeSet<String> {
    let token_re = Regex::new(r"[A-Za-z0-9][A-Za-z0-9_\-]*").expect("token regex");
    token_re
        .find_iter(text)
        .map(|capture| normalize_skill_name(capture.as_str()))
        .filter(|token| token.len() > 1)
        .filter(|token| !is_stop_token(token))
        .collect()
}

fn is_stop_token(token: &str) -> bool {
    matches!(
        token,
        "a" | "an"
            | "and"
            | "check"
            | "for"
            | "help"
            | "me"
            | "please"
            | "the"
            | "then"
            | "to"
            | "use"
            | "with"
    )
}
