function normalizeSummary(skill = {}) {
  return {
    id: skill.id || "",
    name: skill.name || skill.id || "",
    description: skill.description || "",
    source: skill.source || (skill.readOnly ? "builtin" : "workspace"),
    enabled: skill.enabled !== false,
    effective: Boolean(skill.effective),
    available: skill.available !== false,
    missingRequirements: skill.missingRequirements || [],
    overridesBuiltin: Boolean(skill.overridesBuiltin),
    shadowedByWorkspace: Boolean(skill.shadowedByWorkspace),
    readOnly: Boolean(skill.readOnly),
    hasExtraFiles: Boolean(skill.hasExtraFiles),
  };
}

function normalizeDetail(detail = {}, fallback = {}) {
  const summary = normalizeSummary({ ...fallback, ...detail });
  return {
    ...summary,
    path: detail.path || "",
    rawContent: detail.rawContent || "",
    body: detail.body || "",
    normalizedName: detail.normalizedName || summary.id,
    metadata: detail.metadata || {
      always: false,
      requires: { bins: [], env: [] },
      keywords: [],
      tags: [],
    },
    parseWarnings: detail.parseWarnings || [],
    extraFiles: detail.extraFiles || [],
  };
}

function findSummary(groups, selected) {
  if (!selected) return null;
  return [...groups.workspace, ...groups.builtin].find(
    (skill) => skill.source === selected.source && skill.id === selected.id
  ) || null;
}

function matchesSearch(skill, term) {
  if (!term) return true;
  const haystack = [skill.id, skill.name, skill.description].join(" ").toLowerCase();
  return haystack.includes(term);
}

function skillTemplate(id) {
  const title = id
    .split(/[-_]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
  return `---\nname: ${title || "New Skill"}\ndescription: \n---\n\n`;
}

export function createSkillsController({ root, api, setStatus, t, confirmDelete }) {
  const text = (key, fallback) => {
    const translated = typeof t === "function" ? t(key) : key;
    return translated && translated !== key ? translated : fallback;
  };
  const confirmAction = typeof confirmDelete === "function"
    ? confirmDelete
    : (message) => window.confirm(message);
  const status = typeof setStatus === "function" ? setStatus : () => {};
  const detailSection = root.querySelector(".skills-detail");
  const detailHeader = detailSection.querySelector(".skills-detail-header");
  const detailTitle = detailHeader.querySelector(".settings-section-title");
  const createButton = root.querySelector("#skills-create-button");
  const searchInput = root.querySelector("#skills-search");
  const workspaceList = root.querySelector("#skills-workspace-list");
  const builtinList = root.querySelector("#skills-builtin-list");
  const enabledToggle = root.querySelector("#skill-enabled-toggle");
  const editorLabel = detailSection.querySelector(".skills-editor");
  const editor = root.querySelector("#skill-editor");

  const meta = document.createElement("div");
  meta.className = "skills-detail-meta";
  const warnings = document.createElement("div");
  warnings.className = "skills-detail-warnings";
  const emptyState = document.createElement("div");
  emptyState.className = "skills-empty-state";
  const actions = document.createElement("div");
  actions.className = "skills-actions";
  const saveButton = document.createElement("button");
  saveButton.type = "button";
  const reloadButton = document.createElement("button");
  reloadButton.type = "button";
  const deleteButton = document.createElement("button");
  deleteButton.type = "button";
  const copyButton = document.createElement("button");
  copyButton.type = "button";
  actions.append(saveButton, reloadButton, deleteButton, copyButton);
  editorLabel.before(meta);
  meta.after(warnings);
  warnings.after(emptyState);
  editorLabel.after(actions);

  const state = {
    loaded: false,
    summaries: { workspace: [], builtin: [] },
    selected: null,
    detail: null,
    dirty: false,
    search: "",
    requestToken: 0,
  };

  function setDirty(nextDirty) {
    state.dirty = Boolean(nextDirty);
    render(state.detail && !state.detail.readOnly ? editor.value : null);
  }

  function renderList(container, items) {
    container.innerHTML = "";
    if (!items.length) {
      const empty = document.createElement("div");
      empty.className = "skills-list-empty";
      empty.textContent = text("skills_list_empty", "No skills found.");
      container.append(empty);
      return;
    }

    for (const skill of items) {
      const button = document.createElement("button");
      button.type = "button";
      button.dataset.id = skill.id;
      button.dataset.source = skill.source;
      button.dataset.selected = String(
        state.selected?.id === skill.id && state.selected?.source === skill.source
      );
      button.className = "skills-list-item";
      button.textContent = [skill.name, skill.description].filter(Boolean).join(" ");
      container.append(button);
    }
  }

  function renderDetail(preserveEditorValue = null) {
    const detail = state.detail;
    detailTitle.textContent = detail ? detail.name : text("skill_editor_title", "Skill Editor");
    enabledToggle.disabled = !detail || detail.readOnly;
    enabledToggle.checked = Boolean(detail?.enabled);

    saveButton.textContent = text("skills_save", "Save");
    reloadButton.textContent = text("skills_reload", "Reload from disk");
    deleteButton.textContent = text("skills_delete", "Delete");
    copyButton.textContent = text("skills_copy_builtin", "Create workspace copy");

    if (!detail) {
      meta.hidden = true;
      warnings.hidden = true;
      actions.hidden = true;
      emptyState.hidden = false;
      emptyState.textContent = state.summaries.workspace.length
        ? text("skills_no_selection", "Select a skill to inspect or edit.")
        : text("skills_empty_detail", "Create a workspace skill or choose a built-in reference.");
      editor.readOnly = true;
      editor.value = "";
      editorLabel.hidden = true;
      return;
    }

    meta.hidden = false;
    warnings.hidden = !detail.parseWarnings.length;
    emptyState.hidden = true;
    editorLabel.hidden = false;
    actions.hidden = false;
    editor.readOnly = detail.readOnly;
    editor.value = preserveEditorValue ?? detail.rawContent;

    const details = [
      detail.readOnly ? text("skills_source_builtin", "Built-in") : text("skills_source_workspace", "Workspace"),
      detail.effective ? text("skills_effective", "Effective") : text("skills_not_effective", "Not effective"),
      detail.enabled ? text("skills_enabled_state", "Enabled") : text("skills_disabled_state", "Disabled"),
      detail.path,
    ].filter(Boolean);
    if (detail.hasExtraFiles || detail.extraFiles.length) {
      details.push(text("skills_has_extra_files", "Has extra files"));
    }
    meta.textContent = details.join(" • ");
    warnings.textContent = detail.parseWarnings.join("\n");

    saveButton.hidden = detail.readOnly;
    reloadButton.hidden = detail.readOnly;
    deleteButton.hidden = detail.readOnly;
    copyButton.hidden = !detail.readOnly;
  }

  function render(preserveEditorValue = null) {
    const workspaceItems = state.summaries.workspace.filter((skill) => matchesSearch(skill, state.search));
    const builtinItems = state.summaries.builtin.filter((skill) => matchesSearch(skill, state.search));
    renderList(workspaceList, workspaceItems);
    renderList(builtinList, builtinItems);
    renderDetail(preserveEditorValue);
  }

  async function refreshSummaries() {
    const payload = await api.fetchSkillsList();
    state.summaries = {
      workspace: (payload.workspace || []).map(normalizeSummary),
      builtin: (payload.builtin || []).map(normalizeSummary),
    };
  }

  async function loadSelectedDetail(selected) {
    const summary = findSummary(state.summaries, selected) || selected;
    const token = ++state.requestToken;
    const detail = normalizeDetail(
      await api.fetchSkillDetail(selected.source, selected.id),
      summary
    );
    if (token !== state.requestToken) return;
    state.selected = { source: detail.source, id: detail.id };
    state.detail = detail;
    state.dirty = false;
    render();
  }

  function canLeaveDirtyDetail() {
    return !state.dirty || confirmAction(text("skills_discard_confirm", "Discard unsaved skill changes?"));
  }

  function discardDirtyDetail() {
    if (!state.dirty) return;
    state.dirty = false;
    render();
  }

  async function selectSkill(selected) {
    if (!selected) {
      state.selected = null;
      state.detail = null;
      state.dirty = false;
      render();
      return true;
    }
    if (state.selected?.source === selected.source && state.selected?.id === selected.id) {
      return true;
    }
    if (!canLeaveDirtyDetail()) {
      render(state.detail && !state.detail.readOnly ? editor.value : null);
      return false;
    }
    state.selected = { source: selected.source, id: selected.id };
    state.detail = null;
    render();
    await loadSelectedDetail(selected);
    return true;
  }

  async function refreshAfterMutation(nextDetail, options = {}) {
    const draft = options.preserveDraft ? editor.value : null;
    await refreshSummaries();
    state.detail = normalizeDetail(nextDetail, findSummary(state.summaries, nextDetail));
    state.selected = { source: state.detail.source, id: state.detail.id };
    state.dirty = Boolean(options.preserveDraft);
    render(draft);
  }

  async function createWorkspaceSkill(rawContent, suggestedId = "") {
    if (!canLeaveDirtyDetail()) return;
    const id = (window.prompt(
      text("skills_prompt_id", "Workspace skill id:"),
      suggestedId
    ) || "").trim();
    if (!id) return;
    const detail = normalizeDetail(await api.createWorkspaceSkill({ id, rawContent }), {
      id,
      source: "workspace",
    });
    await refreshAfterMutation(detail);
    status(text("skills_create_success", "Skill created."), "idle");
  }

  async function saveCurrentSkill() {
    if (!state.detail || state.detail.readOnly) return;
    const detail = normalizeDetail(
      await api.updateWorkspaceSkill(state.detail.id, editor.value),
      state.detail
    );
    await refreshAfterMutation(detail);
    status(text("skills_save_success", "Skill saved."), "idle");
  }

  async function reloadCurrentSkill() {
    if (!state.detail) return;
    if (state.dirty && !confirmAction(text("skills_reload_confirm", "Reload from disk and discard unsaved changes?"))) {
      render(state.detail && !state.detail.readOnly ? editor.value : null);
      return;
    }
    await loadSelectedDetail(state.selected);
    status(text("skills_reload_success", "Skill reloaded."), "idle");
  }

  async function deleteCurrentSkill() {
    if (!state.detail || state.detail.readOnly) return;
    if (!confirmAction(text("skills_delete_confirm", "Delete this workspace skill? This cannot be undone."))) {
      return;
    }
    await api.deleteWorkspaceSkill(state.detail.id);
    await refreshSummaries();
    const nextSelection = state.summaries.workspace[0] || null;
    state.selected = null;
    state.detail = null;
    state.dirty = false;
    render();
    if (nextSelection) {
      await selectSkill(nextSelection);
    }
    status(text("skills_delete_success", "Skill deleted."), "idle");
  }

  async function toggleCurrentSkillEnabled() {
    if (!state.detail || state.detail.readOnly) return;
    const detail = normalizeDetail(
      await api.updateWorkspaceSkillState(state.detail.id, enabledToggle.checked),
      state.detail
    );
    await refreshAfterMutation(detail, { preserveDraft: state.dirty });
    status(text("skills_toggle_success", "Skill state updated."), "idle");
  }

  async function load() {
    await refreshSummaries();
    state.loaded = true;
    const firstWorkspace = state.summaries.workspace[0];
    if (firstWorkspace) {
      state.selected = null;
      state.detail = null;
      state.dirty = false;
      await selectSkill(firstWorkspace);
      return;
    }
    state.selected = null;
    state.detail = null;
    state.dirty = false;
    render();
  }

  createButton.addEventListener("click", () => {
    void createWorkspaceSkill(skillTemplate("new-skill"), "new-skill").catch((error) => {
      status(error?.message || text("skills_create_failed", "Failed to create skill"), "error");
    });
  });

  searchInput.addEventListener("input", () => {
    state.search = searchInput.value.trim().toLowerCase();
    render();
  });

  root.addEventListener("click", (event) => {
    const button = event.target.closest(".skills-list-item");
    if (!button) return;
    void selectSkill({ source: button.dataset.source, id: button.dataset.id }).catch((error) => {
      status(error?.message || text("skills_load_failed", "Failed to load skill"), "error");
    });
  });

  editor.addEventListener("input", () => {
    if (!state.detail || state.detail.readOnly) return;
    setDirty(editor.value !== state.detail.rawContent);
  });

  enabledToggle.addEventListener("change", () => {
    void toggleCurrentSkillEnabled().catch((error) => {
      enabledToggle.checked = Boolean(state.detail?.enabled);
      status(error?.message || text("skills_toggle_failed", "Failed to update skill state"), "error");
    });
  });

  saveButton.addEventListener("click", () => {
    void saveCurrentSkill().catch((error) => {
      status(error?.message || text("skills_save_failed", "Failed to save skill"), "error");
    });
  });

  reloadButton.addEventListener("click", () => {
    void reloadCurrentSkill().catch((error) => {
      status(error?.message || text("skills_reload_failed", "Failed to reload skill"), "error");
    });
  });

  deleteButton.addEventListener("click", () => {
    void deleteCurrentSkill().catch((error) => {
      status(error?.message || text("skills_delete_failed", "Failed to delete skill"), "error");
    });
  });

  copyButton.addEventListener("click", () => {
    if (!state.detail) return;
    void createWorkspaceSkill(state.detail.rawContent, `${state.detail.id}-copy`).catch((error) => {
      status(error?.message || text("skills_create_failed", "Failed to create skill"), "error");
    });
  });

  render();

  return {
    load,
    rerender() {
      render(state.dirty ? editor.value : null);
    },
    confirmDiscardChanges() {
      const confirmed = canLeaveDirtyDetail();
      if (confirmed) {
        discardDirtyDetail();
      }
      return confirmed;
    },
  };
}
