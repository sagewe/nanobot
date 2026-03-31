// @vitest-environment jsdom

import { readFileSync } from "node:fs";
import path from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";

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

function readCollapsedOutputCssBlock() {
  const cssPath = path.resolve(process.cwd(), "src/style.css");
  const css = readFileSync(cssPath, "utf8");
  const match = css.match(/\.msg-trace-code-wrap--output\[data-expandable="true"\] \.msg-trace-code--output\s*\{([\s\S]*?)\n\s*\}/);
  return match ? match[1] : "";
}

function readCssBlock(selectorPattern) {
  const cssPath = path.resolve(process.cwd(), "src/style.css");
  const css = readFileSync(cssPath, "utf8");
  const match = css.match(new RegExp(`${selectorPattern}\\s*\\{([\\s\\S]*?)\\n\\s*\\}`));
  return match ? match[1] : "";
}

describe("tool trace output", () => {
  beforeEach(() => {
    document.body.innerHTML = BASE_MARKUP;
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
