// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from "vitest";

const SKILLS_MARKUP = `
  <section class="skills-pane">
    <section class="control-panel control-panel--skills">
      <div class="control-panel-header">
        <div>
          <div class="session-kicker">Skills</div>
          <h2 class="jobs-title">Skills Library</h2>
        </div>
        <button id="skills-create-button" type="button">Create Skill</button>
      </div>
      <div class="control-form skills-layout">
        <aside class="skills-master">
          <label class="skills-search">
            <span>Search skills</span>
            <input id="skills-search" type="search" autocomplete="off" />
          </label>
          <section class="skills-group">
            <div class="skills-group-header">
              <div class="session-kicker">Workspace Skills</div>
            </div>
            <div id="skills-workspace-list" class="skills-list"></div>
          </section>
          <section class="skills-group">
            <div class="skills-group-header">
              <div class="session-kicker">Built-in Skills</div>
            </div>
            <div id="skills-builtin-list" class="skills-list"></div>
          </section>
        </aside>
        <section class="skills-detail">
          <div class="skills-detail-header">
            <div>
              <div class="session-kicker">Skills</div>
              <h3 class="settings-section-title">Skill Editor</h3>
            </div>
            <label class="checkbox-row skill-enabled-toggle">
              <input id="skill-enabled-toggle" type="checkbox" />
              <span>Enabled</span>
            </label>
          </div>
          <label class="editor-label skills-editor">
            <span>Skill definition</span>
            <textarea id="skill-editor" rows="18" spellcheck="false"></textarea>
          </label>
        </section>
      </div>
    </section>
  </section>
`;

function flush() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function findButtonByText(container, text) {
  return [...container.querySelectorAll("button")].find((button) => button.textContent.includes(text));
}

function buildSummary(overrides = {}) {
  return {
    id: "weather",
    name: "Weather",
    description: "Reports forecast context.",
    source: "workspace",
    enabled: true,
    effective: true,
    available: true,
    missingRequirements: [],
    overridesBuiltin: false,
    shadowedByWorkspace: false,
    readOnly: false,
    hasExtraFiles: false,
    ...overrides,
  };
}

function buildDetail(summary, overrides = {}) {
  return {
    ...summary,
    path: `/tmp/skills/${summary.id}/SKILL.md`,
    rawContent: `---\nname: ${summary.name}\ndescription: ${summary.description}\n---\n\nBody\n`,
    body: "Body\n",
    normalizedName: summary.id,
    metadata: {
      always: false,
      requires: { bins: [], env: [] },
      keywords: [],
      tags: [],
    },
    parseWarnings: [],
    extraFiles: [],
    ...overrides,
  };
}

describe("createSkillsController", () => {
  beforeEach(() => {
    vi.resetModules();
    document.body.innerHTML = SKILLS_MARKUP;
  });

  it("renders grouped workspace and builtin lists and loads the first workspace detail", async () => {
    const workspaceSummary = buildSummary({
      id: "workspace-weather",
      name: "Workspace Weather",
      description: "Workspace version.",
      source: "workspace",
      overridesBuiltin: true,
      enabled: false,
      available: false,
    });
    const builtinSummary = buildSummary({
      id: "builtin-weather",
      name: "Builtin Weather",
      description: "Builtin version.",
      source: "builtin",
      readOnly: true,
      shadowedByWorkspace: true,
    });
    const api = {
      fetchSkillsList: vi.fn().mockResolvedValue({
        workspace: [workspaceSummary],
        builtin: [builtinSummary],
      }),
      fetchSkillDetail: vi.fn().mockImplementation(async (source, id) => {
        if (source === "workspace" && id === workspaceSummary.id) {
          return buildDetail(workspaceSummary);
        }
        if (source === "builtin" && id === builtinSummary.id) {
          return buildDetail(builtinSummary);
        }
        throw new Error(`unexpected skill ${source}/${id}`);
      }),
      createWorkspaceSkill: vi.fn(),
      updateWorkspaceSkill: vi.fn(),
      updateWorkspaceSkillState: vi.fn(),
      deleteWorkspaceSkill: vi.fn(),
    };

    const { createSkillsController } = await import("../src/skills.js");
    const controller = createSkillsController({
      root: document.querySelector(".skills-pane"),
      api,
      setStatus: vi.fn(),
      t: (key) => key,
      confirmDelete: vi.fn(),
    });

    await controller.load();

    const workspaceItems = [...document.querySelectorAll("#skills-workspace-list button")];
    const builtinItems = [...document.querySelectorAll("#skills-builtin-list button")];

    expect(workspaceItems).toHaveLength(1);
    expect(builtinItems).toHaveLength(1);
    expect(workspaceItems[0].textContent).toContain("Workspace Weather");
    expect(builtinItems[0].textContent).toContain("Builtin Weather");
    expect(
      [...workspaceItems[0].querySelectorAll(".skills-list-badge")].map((node) => node.textContent)
    ).toEqual(expect.arrayContaining(["Workspace", "Overrides builtin", "Disabled", "Unavailable"]));
    expect(
      [...builtinItems[0].querySelectorAll(".skills-list-badge")].map((node) => node.textContent)
    ).toEqual(expect.arrayContaining(["Built-in", "Shadowed", "Enabled", "Available"]));
    expect(api.fetchSkillDetail).toHaveBeenCalledWith("workspace", "workspace-weather");
    expect(document.getElementById("skill-editor").value).toContain("Workspace Weather");
    expect(document.getElementById("skill-editor").readOnly).toBe(false);
  });

  it("loads builtin detail on selection and keeps the editor read-only", async () => {
    const workspaceSummary = buildSummary({
      id: "workspace-weather",
      name: "Workspace Weather",
      source: "workspace",
    });
    const builtinSummary = buildSummary({
      id: "builtin-weather",
      name: "Builtin Weather",
      source: "builtin",
      readOnly: true,
    });
    const api = {
      fetchSkillsList: vi.fn().mockResolvedValue({
        workspace: [workspaceSummary],
        builtin: [builtinSummary],
      }),
      fetchSkillDetail: vi.fn().mockImplementation(async (source, id) => {
        if (source === "workspace" && id === workspaceSummary.id) {
          return buildDetail(workspaceSummary);
        }
        if (source === "builtin" && id === builtinSummary.id) {
          return buildDetail(builtinSummary);
        }
        throw new Error(`unexpected skill ${source}/${id}`);
      }),
      createWorkspaceSkill: vi.fn(),
      updateWorkspaceSkill: vi.fn(),
      updateWorkspaceSkillState: vi.fn(),
      deleteWorkspaceSkill: vi.fn(),
    };

    const { createSkillsController } = await import("../src/skills.js");
    const controller = createSkillsController({
      root: document.querySelector(".skills-pane"),
      api,
      setStatus: vi.fn(),
      t: (key) => key,
      confirmDelete: vi.fn(),
    });

    await controller.load();

    document.querySelector('#skills-builtin-list button[data-id="builtin-weather"]').click();
    await flush();

    expect(api.fetchSkillDetail).toHaveBeenLastCalledWith("builtin", "builtin-weather");
    expect(document.getElementById("skill-editor").value).toContain("Builtin Weather");
    expect(document.getElementById("skill-editor").readOnly).toBe(true);
    expect(document.getElementById("skill-enabled-toggle").disabled).toBe(true);
  });

  it("retries detail loading when the same row is clicked again after a failed fetch", async () => {
    const workspaceSummary = buildSummary({
      id: "workspace-weather",
      name: "Workspace Weather",
      source: "workspace",
    });
    const detail = buildDetail(workspaceSummary);
    const api = {
      fetchSkillsList: vi.fn().mockResolvedValue({
        workspace: [workspaceSummary],
        builtin: [],
      }),
      fetchSkillDetail: vi.fn()
        .mockRejectedValueOnce(new Error("temporary failure"))
        .mockResolvedValueOnce(detail),
      createWorkspaceSkill: vi.fn(),
      updateWorkspaceSkill: vi.fn(),
      updateWorkspaceSkillState: vi.fn(),
      deleteWorkspaceSkill: vi.fn(),
    };
    const { createSkillsController } = await import("../src/skills.js");
    const controller = createSkillsController({
      root: document.querySelector(".skills-pane"),
      api,
      setStatus: vi.fn(),
      t: (key) => key,
      confirmDelete: vi.fn(),
    });

    await controller.load().catch(() => {});
    expect(api.fetchSkillDetail).toHaveBeenCalledTimes(1);

    document.querySelector('#skills-workspace-list button[data-id="workspace-weather"]').click();
    await flush();

    expect(api.fetchSkillDetail).toHaveBeenCalledTimes(2);
    expect(document.getElementById("skill-editor").value).toContain("Workspace Weather");
  });

  it("shows availability and missing requirements in the detail summary strip", async () => {
    const workspaceSummary = buildSummary({
      id: "workspace-weather",
      name: "Workspace Weather",
      source: "workspace",
      available: false,
      missingRequirements: ["python3", "OPENAI_API_KEY"],
    });
    const api = {
      fetchSkillsList: vi.fn().mockResolvedValue({
        workspace: [workspaceSummary],
        builtin: [],
      }),
      fetchSkillDetail: vi.fn().mockResolvedValue(buildDetail(workspaceSummary)),
      createWorkspaceSkill: vi.fn(),
      updateWorkspaceSkill: vi.fn(),
      updateWorkspaceSkillState: vi.fn(),
      deleteWorkspaceSkill: vi.fn(),
    };

    const { createSkillsController } = await import("../src/skills.js");
    const controller = createSkillsController({
      root: document.querySelector(".skills-pane"),
      api,
      setStatus: vi.fn(),
      t: (key) => key,
      confirmDelete: vi.fn(),
    });

    await controller.load();

    const meta = document.querySelector(".skills-detail-meta");

    expect(meta.textContent).toContain("Unavailable");
    expect(meta.textContent).toContain("Missing requirements");
    expect(meta.textContent).toContain("python3");
    expect(meta.textContent).toContain("OPENAI_API_KEY");
  });

  it("shows a visible source badge in the selected-skill header", async () => {
    const workspaceSummary = buildSummary({
      id: "workspace-weather",
      name: "Workspace Weather",
      source: "workspace",
    });
    const builtinSummary = buildSummary({
      id: "builtin-weather",
      name: "Builtin Weather",
      source: "builtin",
      readOnly: true,
    });
    const api = {
      fetchSkillsList: vi.fn().mockResolvedValue({
        workspace: [workspaceSummary],
        builtin: [builtinSummary],
      }),
      fetchSkillDetail: vi.fn().mockImplementation(async (source, id) => {
        if (source === "workspace" && id === workspaceSummary.id) {
          return buildDetail(workspaceSummary);
        }
        if (source === "builtin" && id === builtinSummary.id) {
          return buildDetail(builtinSummary);
        }
        throw new Error(`unexpected skill ${source}/${id}`);
      }),
      createWorkspaceSkill: vi.fn(),
      updateWorkspaceSkill: vi.fn(),
      updateWorkspaceSkillState: vi.fn(),
      deleteWorkspaceSkill: vi.fn(),
    };

    const { createSkillsController } = await import("../src/skills.js");
    const controller = createSkillsController({
      root: document.querySelector(".skills-pane"),
      api,
      setStatus: vi.fn(),
      t: (key) => key,
      confirmDelete: vi.fn(),
    });

    await controller.load();

    let badges = [...document.querySelectorAll(".skills-detail-header .skills-detail-badge")].map((node) => node.textContent);
    expect(badges).toContain("Workspace");

    document.querySelector('#skills-builtin-list button[data-id="builtin-weather"]').click();
    await flush();

    badges = [...document.querySelectorAll(".skills-detail-header .skills-detail-badge")].map((node) => node.textContent);
    expect(badges).toContain("Built-in");
  });

  it("shows workspace override context in the selected-skill detail summary", async () => {
    const workspaceSummary = buildSummary({
      id: "workspace-weather",
      name: "Workspace Weather",
      source: "workspace",
      overridesBuiltin: true,
    });
    const api = {
      fetchSkillsList: vi.fn().mockResolvedValue({
        workspace: [workspaceSummary],
        builtin: [],
      }),
      fetchSkillDetail: vi.fn().mockResolvedValue(buildDetail(workspaceSummary)),
      createWorkspaceSkill: vi.fn(),
      updateWorkspaceSkill: vi.fn(),
      updateWorkspaceSkillState: vi.fn(),
      deleteWorkspaceSkill: vi.fn(),
    };

    const { createSkillsController } = await import("../src/skills.js");
    const controller = createSkillsController({
      root: document.querySelector(".skills-pane"),
      api,
      setStatus: vi.fn(),
      t: (key) => key,
      confirmDelete: vi.fn(),
    });

    await controller.load();

    const meta = document.querySelector(".skills-detail-meta");

    expect(meta.textContent).toContain("Overrides builtin");
  });

  it("reapplies the first workspace selection whenever load runs", async () => {
    const firstWorkspace = buildSummary({
      id: "workspace-alpha",
      name: "Workspace Alpha",
      description: "First workspace skill.",
      source: "workspace",
    });
    const secondWorkspace = buildSummary({
      id: "workspace-beta",
      name: "Workspace Beta",
      description: "Second workspace skill.",
      source: "workspace",
    });
    const builtinSummary = buildSummary({
      id: "builtin-weather",
      name: "Builtin Weather",
      description: "Builtin reference.",
      source: "builtin",
      readOnly: true,
    });
    const api = {
      fetchSkillsList: vi.fn().mockResolvedValue({
        workspace: [firstWorkspace, secondWorkspace],
        builtin: [builtinSummary],
      }),
      fetchSkillDetail: vi.fn().mockImplementation(async (source, id) => {
        if (source === "workspace" && id === firstWorkspace.id) {
          return buildDetail(firstWorkspace);
        }
        if (source === "workspace" && id === secondWorkspace.id) {
          return buildDetail(secondWorkspace);
        }
        if (source === "builtin" && id === builtinSummary.id) {
          return buildDetail(builtinSummary);
        }
        throw new Error(`unexpected skill ${source}/${id}`);
      }),
      createWorkspaceSkill: vi.fn(),
      updateWorkspaceSkill: vi.fn(),
      updateWorkspaceSkillState: vi.fn(),
      deleteWorkspaceSkill: vi.fn(),
    };

    const { createSkillsController } = await import("../src/skills.js");
    const controller = createSkillsController({
      root: document.querySelector(".skills-pane"),
      api,
      setStatus: vi.fn(),
      t: (key) => key,
      confirmDelete: vi.fn(),
    });

    await controller.load();
    document.querySelector('#skills-builtin-list button[data-id="builtin-weather"]').click();
    await flush();
    await controller.load();

    expect(api.fetchSkillDetail).toHaveBeenLastCalledWith("workspace", "workspace-alpha");
    expect(document.getElementById("skill-editor").value).toContain("Workspace Alpha");
    expect(document.getElementById("skill-editor").value).not.toContain("Builtin Weather");
    expect(document.getElementById("skill-editor").readOnly).toBe(false);
  });

  it("keeps toggle-state updates separate from raw-content saves", async () => {
    const workspaceSummary = buildSummary({
      id: "workspace-weather",
      name: "Workspace Weather",
      source: "workspace",
    });
    const detail = buildDetail(workspaceSummary);
    const api = {
      fetchSkillsList: vi.fn().mockResolvedValue({
        workspace: [workspaceSummary],
        builtin: [],
      }),
      fetchSkillDetail: vi.fn().mockResolvedValue(detail),
      createWorkspaceSkill: vi.fn(),
      updateWorkspaceSkill: vi.fn(),
      updateWorkspaceSkillState: vi.fn().mockImplementation(async (id, enabled) => ({
        ...detail,
        enabled,
      })),
      deleteWorkspaceSkill: vi.fn(),
    };

    const { createSkillsController } = await import("../src/skills.js");
    const controller = createSkillsController({
      root: document.querySelector(".skills-pane"),
      api,
      setStatus: vi.fn(),
      t: (key) => key,
      confirmDelete: vi.fn(),
    });

    await controller.load();

    const editor = document.getElementById("skill-editor");
    editor.value = `${editor.value}\n# unsaved`;
    editor.dispatchEvent(new Event("input", { bubbles: true }));

    const toggle = document.getElementById("skill-enabled-toggle");
    toggle.checked = false;
    toggle.dispatchEvent(new Event("change", { bubbles: true }));
    await flush();

    expect(api.updateWorkspaceSkillState).toHaveBeenCalledWith("workspace-weather", false);
    expect(api.updateWorkspaceSkill).not.toHaveBeenCalled();
    expect(document.getElementById("skill-editor").value).toContain("# unsaved");
  });

  it("preserves the caret position when typing in the middle of the editor", async () => {
    const workspaceSummary = buildSummary({
      id: "workspace-weather",
      name: "Workspace Weather",
      source: "workspace",
    });
    const api = {
      fetchSkillsList: vi.fn().mockResolvedValue({
        workspace: [workspaceSummary],
        builtin: [],
      }),
      fetchSkillDetail: vi.fn().mockResolvedValue(buildDetail(workspaceSummary)),
      createWorkspaceSkill: vi.fn(),
      updateWorkspaceSkill: vi.fn(),
      updateWorkspaceSkillState: vi.fn(),
      deleteWorkspaceSkill: vi.fn(),
    };

    const { createSkillsController } = await import("../src/skills.js");
    const controller = createSkillsController({
      root: document.querySelector(".skills-pane"),
      api,
      setStatus: vi.fn(),
      t: (key) => key,
      confirmDelete: vi.fn(),
    });

    await controller.load();

    const editor = document.getElementById("skill-editor");
    const valueDescriptor = Object.getOwnPropertyDescriptor(
      Object.getPrototypeOf(editor),
      "value"
    );
    let currentValue = editor.value;
    Object.defineProperty(editor, "value", {
      configurable: true,
      get() {
        return currentValue;
      },
      set(nextValue) {
        currentValue = String(nextValue);
        this.selectionStart = currentValue.length;
        this.selectionEnd = currentValue.length;
      },
    });
    const insertAt = editor.value.indexOf("description:");
    const nextValue = `${editor.value.slice(0, insertAt)}X${editor.value.slice(insertAt)}`;

    editor.value = nextValue;
    editor.setSelectionRange(insertAt + 1, insertAt + 1);
    editor.dispatchEvent(new Event("input", { bubbles: true }));

    expect(editor.value).toBe(nextValue);
    expect(editor.selectionStart).toBe(insertAt + 1);
    expect(editor.selectionEnd).toBe(insertAt + 1);

    Object.defineProperty(editor, "value", valueDescriptor);
  });

  it("prompts before leaving a dirty workspace detail", async () => {
    const workspaceSummary = buildSummary({
      id: "workspace-weather",
      name: "Workspace Weather",
      source: "workspace",
    });
    const builtinSummary = buildSummary({
      id: "builtin-weather",
      name: "Builtin Weather",
      source: "builtin",
      readOnly: true,
    });
    const api = {
      fetchSkillsList: vi.fn().mockResolvedValue({
        workspace: [workspaceSummary],
        builtin: [builtinSummary],
      }),
      fetchSkillDetail: vi.fn().mockImplementation(async (source, id) => {
        if (source === "workspace" && id === workspaceSummary.id) {
          return buildDetail(workspaceSummary);
        }
        if (source === "builtin" && id === builtinSummary.id) {
          return buildDetail(builtinSummary);
        }
        throw new Error(`unexpected skill ${source}/${id}`);
      }),
      createWorkspaceSkill: vi.fn(),
      updateWorkspaceSkill: vi.fn(),
      updateWorkspaceSkillState: vi.fn(),
      deleteWorkspaceSkill: vi.fn(),
    };
    const confirmDelete = vi.fn().mockReturnValue(false);

    const { createSkillsController } = await import("../src/skills.js");
    const controller = createSkillsController({
      root: document.querySelector(".skills-pane"),
      api,
      setStatus: vi.fn(),
      t: (key) => key,
      confirmDelete,
    });

    await controller.load();

    const editor = document.getElementById("skill-editor");
    editor.value = `${editor.value}\n# dirty`;
    editor.dispatchEvent(new Event("input", { bubbles: true }));

    document.querySelector('#skills-builtin-list button[data-id="builtin-weather"]').click();
    await flush();

    expect(confirmDelete).toHaveBeenCalledTimes(1);
    expect(api.fetchSkillDetail).not.toHaveBeenLastCalledWith("builtin", "builtin-weather");
    expect(document.getElementById("skill-editor").value).toContain("# dirty");
    expect(document.getElementById("skill-editor").value).toContain("Workspace Weather");
  });

  it("discards dirty draft state when tab-leave confirmation is accepted", async () => {
    const workspaceSummary = buildSummary({
      id: "workspace-weather",
      name: "Workspace Weather",
      source: "workspace",
    });
    const api = {
      fetchSkillsList: vi.fn().mockResolvedValue({
        workspace: [workspaceSummary],
        builtin: [],
      }),
      fetchSkillDetail: vi.fn().mockResolvedValue(buildDetail(workspaceSummary)),
      createWorkspaceSkill: vi.fn(),
      updateWorkspaceSkill: vi.fn(),
      updateWorkspaceSkillState: vi.fn(),
      deleteWorkspaceSkill: vi.fn(),
    };
    const confirmDelete = vi.fn().mockReturnValue(true);

    const { createSkillsController } = await import("../src/skills.js");
    const controller = createSkillsController({
      root: document.querySelector(".skills-pane"),
      api,
      setStatus: vi.fn(),
      t: (key) => key,
      confirmDelete,
    });

    await controller.load();

    const editor = document.getElementById("skill-editor");
    editor.value = `${editor.value}\n# dirty`;
    editor.dispatchEvent(new Event("input", { bubbles: true }));

    expect(controller.confirmDiscardChanges()).toBe(true);
    expect(confirmDelete).toHaveBeenCalledTimes(1);
    expect(document.getElementById("skill-editor").value).not.toContain("# dirty");

    await controller.load();

    expect(api.fetchSkillDetail).toHaveBeenLastCalledWith("workspace", "workspace-weather");
    expect(document.getElementById("skill-editor").value).toContain("Workspace Weather");
    expect(document.getElementById("skill-editor").value).not.toContain("# dirty");
  });

  it("warns that delete removes the whole skill directory and extra files", async () => {
    const workspaceSummary = buildSummary({
      id: "workspace-weather",
      name: "Workspace Weather",
      source: "workspace",
      hasExtraFiles: true,
    });
    const api = {
      fetchSkillsList: vi.fn().mockResolvedValue({
        workspace: [workspaceSummary],
        builtin: [],
      }),
      fetchSkillDetail: vi.fn().mockResolvedValue(buildDetail(workspaceSummary, {
        extraFiles: ["notes.txt", "examples.md"],
      })),
      createWorkspaceSkill: vi.fn(),
      updateWorkspaceSkill: vi.fn(),
      updateWorkspaceSkillState: vi.fn(),
      deleteWorkspaceSkill: vi.fn(),
    };
    const confirmDelete = vi.fn().mockReturnValue(false);

    const { createSkillsController } = await import("../src/skills.js");
    const controller = createSkillsController({
      root: document.querySelector(".skills-pane"),
      api,
      setStatus: vi.fn(),
      t: (key) => key,
      confirmDelete,
    });

    await controller.load();

    findButtonByText(document.querySelector(".skills-actions"), "Delete").click();

    expect(confirmDelete).toHaveBeenCalledTimes(1);
    expect(confirmDelete.mock.calls[0][0]).toContain("entire skill directory");
    expect(confirmDelete.mock.calls[0][0]).toContain("notes.txt");
    expect(api.deleteWorkspaceSkill).not.toHaveBeenCalled();
  });
});
