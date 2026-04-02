// @vitest-environment jsdom

import { readFileSync } from "node:fs";
import path from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";

const apiMocks = vi.hoisted(() => {
  const noop = vi.fn();
  return {
    fetchCurrentUser: vi.fn(),
    loginUser: noop,
    logoutUser: noop,
    changePassword: noop,
    fetchMyConfig: noop,
    updateMyConfig: vi.fn(),
    fetchAdminUsers: noop,
    createAdminUser: noop,
    enableAdminUser: noop,
    disableAdminUser: noop,
    setAdminUserPassword: noop,
    setAdminUserRole: noop,
    fetchSessions: noop,
    fetchSessionDetail: noop,
    createSession: noop,
    duplicateSession: noop,
    deleteSession: noop,
    setSessionProfile: noop,
    sendChat: noop,
    fetchWeixinAccount: noop,
    startWeixinLogin: noop,
    fetchWeixinLoginStatus: noop,
    logoutWeixin: noop,
    loadProfiles: noop,
    fetchCronJobs: noop,
    addCronJob: noop,
    deleteCronJob: noop,
    toggleCronJob: noop,
    runCronJob: noop,
    fetchMcpServers: noop,
    toggleMcpTool: noop,
    applyMcpServerAction: noop,
  };
});

const skillsControllerMocks = vi.hoisted(() => ({
  controller: {
    load: vi.fn().mockResolvedValue(undefined),
    confirmDiscardChanges: vi.fn().mockReturnValue(true),
    rerender: vi.fn(),
  },
  createSkillsController: vi.fn(),
}));

vi.mock("../src/api.js", () => apiMocks);
vi.mock("../src/skills.js", () => ({
  createSkillsController: (...args) => {
    skillsControllerMocks.createSkillsController(...args);
    return skillsControllerMocks.controller;
  },
}));

const BASE_MARKUP = `
  <div id="transcript"></div>
  <select id="session-select"></select>
  <div id="profile-picker-label"></div>
  <div id="profile-picker-menu"></div>
  <div id="status"></div>
  <div id="weixin-status-label"></div>
  <div id="weixin-user-label"></div>
  <div id="weixin-qr-panel"></div>
  <img id="weixin-qr-image" />
  <button id="weixin-login-button"></button>
  <button id="weixin-logout-button"></button>
`;

async function loadRenderModule() {
  vi.resetModules();
  document.body.innerHTML = BASE_MARKUP;
  return import("../src/render.js");
}

function buildDetail(output, argumentsText = "{\"q\":\"weather\"}") {
  return {
    activeProfile: "openai:gpt-4.1-mini",
    messages: [
      {
        role: "assistant",
        toolCalls: [
          {
            id: "call_1",
            name: "web.search_query",
            arguments: argumentsText,
          },
        ],
      },
      {
        role: "tool",
        toolCallId: "call_1",
        toolName: "web.search_query",
        content: output,
      },
    ],
  };
}

function readFadeCssBlock() {
  const cssPath = path.resolve(process.cwd(), "src/style.css");
  const css = readFileSync(cssPath, "utf8");
  const match = css.match(/\.msg-trace-code-fade\s*\{([\s\S]*?)\n\s*\}/);
  return match ? match[1] : "";
}

function readHtml() {
  const htmlPath = path.resolve(process.cwd(), "index.html");
  return readFileSync(htmlPath, "utf8");
}

function readHtmlBody() {
  const html = readHtml();
  const match = html.match(/<body[^>]*>([\s\S]*)<\/body>/i);
  return match ? match[1] : html;
}

function readCss() {
  const cssPath = path.resolve(process.cwd(), "src/style.css");
  return readFileSync(cssPath, "utf8");
}

function readJs() {
  const jsPath = path.resolve(process.cwd(), "src/main.js");
  return readFileSync(jsPath, "utf8");
}

function readApiJs() {
  const jsPath = path.resolve(process.cwd(), "src/api.js");
  return readFileSync(jsPath, "utf8");
}

function readPackageJson() {
  const jsonPath = path.resolve(process.cwd(), "package.json");
  return readFileSync(jsonPath, "utf8");
}

function readCollapsedOutputCssBlock() {
  const css = readCss();
  const match = css.match(/\.msg-trace-code-wrap--output\[data-expandable="true"\] \.msg-trace-code--output\s*\{([\s\S]*?)\n\s*\}/);
  return match ? match[1] : "";
}

function readCssBlock(selectorPattern) {
  const css = readCss();
  const match = css.match(new RegExp(`${selectorPattern}\\s*\\{([\\s\\S]*?)\\n\\s*\\}`));
  return match ? match[1] : "";
}

describe("tool trace output", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    document.body.innerHTML = BASE_MARKUP;
    skillsControllerMocks.controller.load.mockReset().mockResolvedValue(undefined);
    skillsControllerMocks.controller.confirmDiscardChanges.mockReset().mockReturnValue(true);
    skillsControllerMocks.controller.rerender.mockReset();
    skillsControllerMocks.createSkillsController.mockReset();
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      writable: true,
      value: vi.fn(() => ({
        matches: false,
        media: "",
        onchange: null,
        addListener: vi.fn(),
        removeListener: vi.fn(),
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    });
  });

  it("renders short arguments inline in the trace header", async () => {
    const { renderSessionDetail } = await loadRenderModule();

    renderSessionDetail(buildDetail("ok"));

    const args = document.querySelector(".msg-trace-args");

    expect(args).not.toBeNull();
    expect(args.textContent).toContain("\"q\":\"weather\"");
    expect(document.querySelector(".msg-trace-code--arguments")).toBeNull();
    expect(document.querySelector(".msg-trace-show-more")).toBeNull();
  });

  it("does not render empty arguments in the trace header", async () => {
    const { renderSessionDetail } = await loadRenderModule();

    renderSessionDetail(buildDetail("ok", "{}"));

    expect(document.querySelector(".msg-trace-args")).toBeNull();
  });

  it("expands long arguments without opening the trace body", async () => {
    const { renderSessionDetail } = await loadRenderModule();
    const longArgs = JSON.stringify({
      q: "weather in shanghai tomorrow morning with humidity and hourly forecast",
      locale: "zh-CN",
      units: "metric",
    }, null, 2);

    renderSessionDetail(buildDetail("ok", longArgs));

    const item = document.querySelector(".msg-trace-item");
    const argsButton = document.querySelector("button.msg-trace-args");

    expect(item).not.toBeNull();
    expect(argsButton).not.toBeNull();
    expect(item.dataset.open).toBe("false");
    expect(argsButton.dataset.expanded).toBe("false");

    argsButton.click();

    expect(item.dataset.open).toBe("false");
    expect(argsButton.dataset.expanded).toBe("true");
  });

  it("renders long output with a dedicated reveal hotspot", async () => {
    const { renderSessionDetail } = await loadRenderModule();
    const longOutput = Array.from({ length: 8 }, (_, index) => `line ${index + 1}`).join("\n");

    renderSessionDetail(buildDetail(longOutput));

    const wrap = document.querySelector(".msg-trace-code-wrap--output");
    const hotspot = document.querySelector(".msg-trace-code-reveal");
    const button = document.querySelector(".msg-trace-show-more");
    const icon = document.querySelector(".msg-trace-item-icon");

    expect(wrap).not.toBeNull();
    expect(hotspot).not.toBeNull();
    expect(button).not.toBeNull();
    expect(icon).toBeNull();
    expect(wrap.dataset.expanded).toBe("false");
    expect(button.textContent).toContain("Show more");
    expect(button.closest(".msg-trace-code-reveal")).toBe(hotspot);

    button.click();

    expect(wrap.dataset.expanded).toBe("true");
    expect(document.querySelector(".msg-trace-show-more")).toBe(button);
    expect(button.textContent).toContain("Show less");
  });

  it("fades the collapsed output with a mask instead of blur overlay", () => {
    const fadeCss = readFadeCssBlock();
    const collapsedCss = readCollapsedOutputCssBlock();

    expect(collapsedCss).toContain("mask-image:");
    expect(collapsedCss).toContain("-webkit-mask-image:");
    expect(fadeCss).not.toContain("backdrop-filter:");
    expect(fadeCss).not.toContain("-webkit-backdrop-filter:");
  });

  it("keeps message rows constrained to the transcript width instead of the viewport", () => {
    const groupCss = readCssBlock("\\.msg-group");
    const bodyCss = readCssBlock("\\.msg-body");

    expect(groupCss).toContain("width: 100%;");
    expect(bodyCss).not.toContain("100vw");
  });

  it("renders tool output without box chrome", () => {
    const outputCss = readCssBlock("\\.msg-trace-code--output");
    const darkOutputCss = readCssBlock(":root\\[data-theme=\"dark\"\\] \\.msg-trace-code--output");
    const autoDarkOutputCss = readCssBlock(":root:not\\(\\[data-theme=\"light\"\\]\\):not\\(\\[data-theme=\"dark\"\\]\\) \\.msg-trace-code--output");
    const showMoreCss = readCssBlock("\\.msg-trace-show-more");

    expect(outputCss).toContain("border: none;");
    expect(outputCss).toContain("background: transparent;");
    expect(darkOutputCss).toContain("background: transparent;");
    expect(autoDarkOutputCss).toContain("background: transparent;");
    expect(showMoreCss).toContain("left:");
    expect(showMoreCss).not.toContain("right:");
  });

  it("uses warm dark surfaces for the sidebar identity area in dark themes", () => {
    const darkUserChipCss = readCssBlock(":root\\[data-theme=\"dark\"\\] \\.user-chip");
    const autoDarkUserChipCss = readCssBlock(":root:not\\(\\[data-theme=\"light\"\\]\\):not\\(\\[data-theme=\"dark\"\\]\\) \\.user-chip");
    const darkFooterButtonCss = readCssBlock(":root\\[data-theme=\"dark\"\\] \\.sidebar-footer button");
    const autoDarkFooterButtonCss = readCssBlock(":root:not\\(\\[data-theme=\"light\"\\]\\):not\\(\\[data-theme=\"dark\"\\]\\) \\.sidebar-footer button");

    expect(darkUserChipCss).toContain("background:");
    expect(darkUserChipCss).not.toContain("background: rgba(255, 255, 255");
    expect(autoDarkUserChipCss).toContain("background:");
    expect(autoDarkUserChipCss).not.toContain("background: rgba(255, 255, 255");
    expect(darkFooterButtonCss).toContain("background:");
    expect(darkFooterButtonCss).not.toContain("transparent");
    expect(autoDarkFooterButtonCss).toContain("background:");
    expect(autoDarkFooterButtonCss).not.toContain("transparent");
  });

  it("reuses the jobs panel palette for settings and users surfaces", () => {
    const controlPanelCss = readCssBlock("\\.control-panel");
    const adminUserCardCss = readCssBlock("\\.admin-user-card");
    const controlPanelButtonCss = readCssBlock("\\.control-panel-header button");
    const adminUserActionCss = readCssBlock("\\.admin-user-actions button");

    expect(controlPanelCss).toContain("background: var(--panel);");
    expect(controlPanelCss).not.toContain("255, 255, 255");
    expect(adminUserCardCss).toContain("background: var(--panel);");
    expect(adminUserCardCss).not.toContain("253, 252, 251");
    expect(controlPanelButtonCss).toContain("background: transparent;");
    expect(controlPanelButtonCss).not.toContain("rgba(193, 95, 60, 0.08)");
    expect(adminUserActionCss).toContain("background: transparent;");
    expect(adminUserActionCss).not.toContain("rgba(193, 95, 60, 0.08)");
  });

  it("splits settings into primary controls and a dedicated advanced editor column", () => {
    const html = readHtml();
    const css = readCss();
    const settingsLayoutCss = readCssBlock("\\.settings-layout");
    const settingsMainCss = readCssBlock("\\.settings-main");
    const settingsAdvancedCss = readCssBlock("\\.settings-advanced");

    expect(html).toContain('id="settings-form" class="control-form settings-layout"');
    expect(html).toContain('class="settings-main"');
    expect(html).toContain('class="settings-advanced"');
    expect(html).toContain('class="settings-advanced-header"');
    expect(settingsLayoutCss).toContain("grid-template-columns:");
    expect(settingsMainCss).toContain("display: grid;");
    expect(settingsAdvancedCss).toContain("grid-template-rows:");
    expect(css).toContain("@media (max-width: 1100px)");
    expect(css).toContain(".settings-layout");
    expect(css).toContain("grid-template-columns: 1fr;");
  });

  it("adds a static skills shell with responsive master-detail scaffolding", async () => {
    const html = readHtml();
    const css = readCss();
    const skillsLayoutCss = readCssBlock("\\.skills-layout");
    const skillsListCss = readCssBlock("\\.skills-list");
    const skillsDetailCss = readCssBlock("\\.skills-detail");
    const { TRANSLATIONS } = await import("../src/i18n.js");

    expect(html).toContain('data-tab="skills"');
    expect(html).toContain('class="skills-pane"');
    expect(html).toContain('id="skills-search"');
    expect(html).toContain('id="skills-workspace-list"');
    expect(html).toContain('id="skills-builtin-list"');
    expect(html).toContain('id="skill-editor"');
    expect(html).toContain('id="skill-enabled-toggle"');
    expect(html).toContain('data-i18n="tab_skills"');
    expect(html).toContain('data-i18n-placeholder="skills_search_placeholder"');
    expect(html).toContain('data-i18n="skills_create"');
    expect(html).toContain('data-i18n="skill_enabled"');
    expect(html).toContain('data-i18n="skills_builtin_title"');
    expect(html).toContain('data-i18n="skills_workspace_title"');
    expect(html).toContain('data-i18n="skill_editor_title"');

    expect(skillsLayoutCss).toContain("display: grid;");
    expect(skillsLayoutCss).toContain("grid-template-columns:");
    expect(skillsListCss).toContain("display: grid;");
    expect(skillsDetailCss).toContain("display: grid;");
    expect(css).toContain("@media (max-width: 1100px)");
    expect(css).toContain(".skills-layout");
    expect(css).toContain("grid-template-columns: 1fr;");

    expect(TRANSLATIONS.en.tab_skills).toBe("Skills");
    expect(TRANSLATIONS.zh.tab_skills).toBe("\u6280\u80fd");
  });

  it("wires the skills pane into the main tab switcher", () => {
    const js = readJs();

    expect(js).toContain('const skillsPane = document.querySelector(".skills-pane");');
    expect(js).toContain('skillsPane.hidden = tab !== "skills";');
  });

  it("groups channels by provider and wires settings/users labels through i18n", async () => {
    const html = readHtml();
    const js = readJs();
    const apiJs = readApiJs();
    const packageJson = readPackageJson();
    const { TRANSLATIONS } = await import("../src/i18n.js");

    expect(html).toContain('class="settings-channel-groups"');
    expect(html).toContain('class="settings-channel-group"');
    expect(html).toContain('data-i18n="settings_channels_telegram"');
    expect(html).toContain('data-i18n="settings_channels_weixin"');
    expect(html).toContain('data-i18n="settings_channels_wecom"');
    expect(html).toContain('data-i18n="settings_channels_feishu"');
    expect(html).toContain('data-i18n="settings_workspace_title"');
    expect(html).toContain('data-i18n="settings_advanced_title"');
    expect(html).toContain('data-i18n="settings_password_title"');
    expect(html).toContain('data-i18n="users_title"');
    expect(html).toContain('data-i18n="users_create_submit"');
    expect(html).toContain('data-i18n="tab_settings"');
    expect(html).toContain('data-i18n="tab_users"');
    expect(html).toContain("Advanced TOML");

    expect(TRANSLATIONS.en.settings_workspace_title).toBeTruthy();
    expect(TRANSLATIONS.zh.settings_workspace_title).toBeTruthy();
    expect(TRANSLATIONS.en.settings_advanced_title).toBe("Advanced TOML");
    expect(TRANSLATIONS.zh.settings_advanced_title).toContain("TOML");
    expect(TRANSLATIONS.en.settings_channels_feishu).toBe("Feishu");
    expect(TRANSLATIONS.zh.settings_channels_feishu).toBeTruthy();
    expect(TRANSLATIONS.en.users_action_reset_password).toBeTruthy();
    expect(TRANSLATIONS.zh.users_action_reset_password).toBeTruthy();
    expect(js).toContain('t("users_empty")');
    expect(js).toContain('t("users_action_reset_password")');
    expect(js).toContain('t("settings_saved")');
    expect(js).toContain('t("users_updated")');
    expect(js).toContain('import TOML from "@iarna/toml"');
    expect(js).toContain("TOML.stringify");
    expect(js).toContain("TOML.parse");
    expect(apiJs).toContain("export async function updateMyConfig(nextConfig)");
    expect(apiJs).toContain("body: JSON.stringify(nextConfig)");
    expect(packageJson).toContain('"@iarna/toml"');
  });

  it("surfaces Feishu structured settings in the workspace form", () => {
    const html = readHtml();
    const js = readJs();

    expect(html).toContain('id="settings-feishu-enabled"');
    expect(html).toContain('id="settings-feishu-app-id"');
    expect(html).toContain('id="settings-feishu-app-secret"');
    expect(html).toContain('id="settings-feishu-api-base"');
    expect(html).toContain('id="settings-feishu-ws-base"');
    expect(js).toContain('const settingsFeishuEnabled = document.getElementById("settings-feishu-enabled")');
    expect(js).toContain('const settingsFeishuAppId = document.getElementById("settings-feishu-app-id")');
    expect(js).toContain('const settingsFeishuAppSecret = document.getElementById("settings-feishu-app-secret")');
    expect(js).toContain('const settingsFeishuApiBase = document.getElementById("settings-feishu-api-base")');
    expect(js).toContain('const settingsFeishuWsBase = document.getElementById("settings-feishu-ws-base")');
    expect(js).toContain("next.channels.feishu ??= {}");
  });

  it("submits parsed TOML editor text as a structured config object", async () => {
    document.body.innerHTML = readHtmlBody();
    vi.resetModules();
    apiMocks.fetchCurrentUser.mockRejectedValueOnce(new Error("login required"));
    apiMocks.updateMyConfig.mockResolvedValueOnce({ ok: true });

    const { default: TOML } = await import("@iarna/toml");
    await import("../src/main.js");
    await new Promise((resolve) => setTimeout(resolve, 0));

    const configEditor = document.getElementById("config-editor");
    const settingsForm = document.getElementById("settings-form");
    const settingsDefaultProfile = document.getElementById("settings-default-profile");
    const settingsTelegramEnabled = document.getElementById("settings-telegram-enabled");

    settingsDefaultProfile.value = "openai:gpt-4.1-mini";
    settingsTelegramEnabled.checked = true;
    configEditor.value = TOML.stringify({
      providers: {
        codex: {
          apiBase: "https://api.example.test/v1",
        },
      },
      channels: {
        telegram: {
          enabled: false,
          token: "",
        },
      },
    });

    settingsForm.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(apiMocks.updateMyConfig).toHaveBeenCalledTimes(1);
    const [payload] = apiMocks.updateMyConfig.mock.calls[0];

    expect(typeof payload).toBe("object");
    expect(payload.providers.codex.apiBase).toBe("https://api.example.test/v1");
    expect(payload.channels.telegram.enabled).toBe(true);
    expect(payload.agents.defaults.defaultProfile).toBe("openai:gpt-4.1-mini");
    expect(payload).not.toBe(configEditor.value);
  });

  it("merges Feishu structured settings into the submitted config", async () => {
    document.body.innerHTML = readHtmlBody();
    vi.resetModules();
    apiMocks.fetchCurrentUser.mockRejectedValueOnce(new Error("login required"));
    apiMocks.updateMyConfig.mockResolvedValueOnce({ ok: true });

    const { default: TOML } = await import("@iarna/toml");
    await import("../src/main.js");
    await new Promise((resolve) => setTimeout(resolve, 0));

    const configEditor = document.getElementById("config-editor");
    const settingsForm = document.getElementById("settings-form");
    const settingsFeishuEnabled = document.getElementById("settings-feishu-enabled");
    const settingsFeishuAppId = document.getElementById("settings-feishu-app-id");
    const settingsFeishuAppSecret = document.getElementById("settings-feishu-app-secret");
    const settingsFeishuApiBase = document.getElementById("settings-feishu-api-base");
    const settingsFeishuWsBase = document.getElementById("settings-feishu-ws-base");

    settingsFeishuEnabled.checked = true;
    settingsFeishuAppId.value = "cli_test_app";
    settingsFeishuAppSecret.value = "secret-value";
    settingsFeishuApiBase.value = "https://open.feishu.cn/open-apis";
    settingsFeishuWsBase.value = "wss://open.feishu.cn/ws";
    configEditor.value = TOML.stringify({
      channels: {
        feishu: {
          enabled: false,
          appId: "",
          appSecret: "",
          apiBase: "",
          wsBase: "",
        },
      },
    });

    settingsForm.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(apiMocks.updateMyConfig).toHaveBeenCalledTimes(1);
    const [payload] = apiMocks.updateMyConfig.mock.calls[0];

    expect(payload.channels.feishu.enabled).toBe(true);
    expect(payload.channels.feishu.appId).toBe("cli_test_app");
    expect(payload.channels.feishu.appSecret).toBe("secret-value");
    expect(payload.channels.feishu.apiBase).toBe("https://open.feishu.cn/open-apis");
    expect(payload.channels.feishu.wsBase).toBe("wss://open.feishu.cn/ws");
  });

  it("does not reload skills again when clicking the already-active skills tab", async () => {
    vi.resetModules();
    document.body.innerHTML = readHtmlBody();
    apiMocks.fetchCurrentUser.mockRejectedValueOnce(new Error("login required"));

    await import("../src/main.js");
    await new Promise((resolve) => setTimeout(resolve, 0));

    const skillsTab = document.querySelector('.tab-btn[data-tab="skills"]');

    skillsTab.click();
    await new Promise((resolve) => setTimeout(resolve, 0));
    skillsTab.click();
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(skillsControllerMocks.controller.load).toHaveBeenCalledTimes(1);
  });

  it("separates the users page into a create card and a directory card", () => {
    const html = readHtml();
    const css = readCss();
    const usersLayoutCss = readCssBlock("\\.users-layout");
    const usersCreateFormCss = readCssBlock("\\.users-create-form");

    expect(html).toContain('class="users-pane-header"');
    expect(html).toContain('class="users-layout"');
    expect(html).toContain('class="control-panel users-create-card"');
    expect(html).toContain('class="control-panel users-list-card"');
    expect(html).toContain('id="create-user-form" class="control-form compact-form users-create-form"');
    expect(usersLayoutCss).toContain("display: grid;");
    expect(usersCreateFormCss).toContain("max-width:");
    expect(css).toContain(".users-list-card");
  });

  it("aligns the intent text without reserving chevron width", () => {
    const intentHeaderCss = readCssBlock("\\.msg-intent-header");
    const intentIconCss = readCssBlock("\\.msg-intent-icon");
    const intentDetailsCss = readCssBlock("\\.msg-intent-details");
    const intentDetailsLineCss = readCssBlock("\\.msg-intent-details::before");

    expect(intentHeaderCss).not.toContain("grid-template-columns:");
    expect(intentIconCss).toContain("position: absolute;");
    expect(intentIconCss).toContain("left:");
    expect(intentDetailsCss).toContain("padding-left: 0;");
    expect(intentDetailsLineCss).toContain("left: calc(-1 * (var(--msg-intent-prefix-width) + var(--msg-intent-prefix-gap)) + (var(--msg-intent-prefix-width) * 0.5));");
  });

  it("hides the assistant footer for messages that include tool traces", async () => {
    const { renderSessionDetail } = await loadRenderModule();

    renderSessionDetail(buildDetail("ok"));

    expect(document.querySelector(".msg-group[data-activity=\"true\"] .msg-footer")).toBeNull();
  });

  it("keeps the assistant footer for plain assistant messages", async () => {
    const { renderSessionDetail } = await loadRenderModule();

    renderSessionDetail({
      activeProfile: "codex:gpt-5.4",
      messages: [
        {
          role: "assistant",
          content: "hello",
          timestamp: "2026-04-01T00:03:00.000Z",
        },
      ],
    });

    expect(document.querySelector(".msg-group[data-role=\"assistant\"] .msg-footer")).not.toBeNull();
  });
});
