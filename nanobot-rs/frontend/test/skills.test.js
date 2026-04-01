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
    });
    const builtinSummary = buildSummary({
      id: "builtin-weather",
      name: "Builtin Weather",
      description: "Builtin version.",
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

    const workspaceItems = [...document.querySelectorAll("#skills-workspace-list button")];
    const builtinItems = [...document.querySelectorAll("#skills-builtin-list button")];

    expect(workspaceItems).toHaveLength(1);
    expect(builtinItems).toHaveLength(1);
    expect(workspaceItems[0].textContent).toContain("Workspace Weather");
    expect(builtinItems[0].textContent).toContain("Builtin Weather");
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
});
