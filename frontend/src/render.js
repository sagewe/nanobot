import DOMPurify from "dompurify";
import hljs from "highlight.js/lib/core";
import json from "highlight.js/lib/languages/json";
import bash from "highlight.js/lib/languages/bash";
import yaml from "highlight.js/lib/languages/yaml";
import xml from "highlight.js/lib/languages/xml";
import { t, tChannel, tToolCount } from "./i18n.js";

hljs.registerLanguage("json", json);
hljs.registerLanguage("bash", bash);
hljs.registerLanguage("yaml", yaml);
hljs.registerLanguage("xml", xml);

const transcript = document.getElementById("transcript");
const sessionSelect = document.getElementById("session-select");
const profileSelect = document.getElementById("profile-select");
const profilePickerLabel = document.getElementById("profile-picker-label");
const profilePickerMenu = document.getElementById("profile-picker-menu");
let _currentProfileValue = "";
const statusNode = document.getElementById("status");
const weixinStatusLabel = document.getElementById("weixin-status-label");
const weixinUserLabel = document.getElementById("weixin-user-label");
const weixinStatusGroup = weixinStatusLabel?.parentElement;
const weixinQrPanel = document.getElementById("weixin-qr-panel");
const weixinQrImage = document.getElementById("weixin-qr-image");
const weixinLoginButton = document.getElementById("weixin-login-button");
const weixinLogoutButton = document.getElementById("weixin-logout-button");

// ── MCP tool name helpers ──────────────────────────────────────────────────────
// Parse "mcp_{server}_{ToolName}" → { server, shortName } or null
function parseMcpTool(name) {
  if (!name) return null;
  const m = name.match(/^mcp_([^_]+)_(.+)$/);
  return m ? { server: m[1], shortName: m[2] } : null;
}

// Server icon map: server_name → icon string (emoji, URL, or data URI)
const mcpServerIcons = new Map();

export function setMcpServerIcons(icons) {
  mcpServerIcons.clear();
  for (const [k, v] of Object.entries(icons)) mcpServerIcons.set(k, v);
}

// Build a label element for a tool name: [server-badge] ShortName
function buildToolNameEl(name) {
  const mcp = parseMcpTool(name);
  const span = document.createElement("span");
  span.className = "msg-tool-name";
  if (mcp) {
    const icon = mcpServerIcons.get(mcp.server);
    if (icon) {
      if (icon.startsWith("http") || icon.startsWith("/") || icon.startsWith("data:")) {
        const img = document.createElement("img");
        img.className = "msg-mcp-server-icon";
        img.src = icon;
        img.alt = mcp.server;
        span.appendChild(img);
      } else {
        const badge = document.createElement("span");
        badge.className = "msg-mcp-server-badge msg-mcp-server-badge--emoji";
        badge.textContent = icon;
        span.appendChild(badge);
      }
    } else {
      const badge = document.createElement("span");
      badge.className = "msg-mcp-server-badge";
      badge.textContent = mcp.server;
      span.appendChild(badge);
    }
    span.appendChild(document.createTextNode(mcp.shortName));
  } else {
    span.textContent = name;
  }
  return span;
}

const USER_AVATAR_SVG = `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>`;
const ASSISTANT_AVATAR_SVG = `<svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2L13.5 10.5L22 12L13.5 13.5L12 22L10.5 13.5L2 12L10.5 10.5Z"/></svg>`;
const TOOL_AVATAR_SVG = `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>`;
const COPY_SVG = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>`;
const CHECK_SVG = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.25" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>`;
const CHEVRON_RIGHT_SVG = `<svg viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="4,2.5 8,6 4,9.5"/></svg>`;
const TRACE_OUTPUT_COLLAPSED_LINES = 3;
const TRACE_OUTPUT_COLLAPSED_CHARS = 320;
const TRACE_ARGUMENT_PREVIEW_CHARS = 72;

export function formatTime(date) {
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function normalizeWeixinQrSource(content) {
  const value = (content || "").trim();
  if (!value) return "";
  if (
    value.startsWith("data:") ||
    value.startsWith("blob:") ||
    value.startsWith("http://") ||
    value.startsWith("https://") ||
    value.startsWith("/")
  ) {
    return value;
  }
  const compact = value.replace(/\s+/g, "");
  if (/^[A-Za-z0-9+/=]+$/.test(compact)) {
    return `data:image/png;base64,${compact}`;
  }
  return value;
}

export function setStatus(message, variant = "idle") {
  statusNode.textContent = message;
  statusNode.dataset.variant = variant;
}

export function setCurrentProfile(profile) {
  if (!profile) return;
  _currentProfileValue = profile;
  profilePickerLabel.textContent = profile;
  if (profileSelect) profileSelect.value = profile;
  profilePickerMenu.querySelectorAll(".profile-picker-check").forEach((el) => {
    el.hidden = el.dataset.profile !== profile;
  });
}

export function renderProfiles(profiles) {
  profilePickerMenu.innerHTML = "";
  if (profileSelect) {
    profileSelect.innerHTML = "";
  }
  const CHECK_SVG = `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>`;
  for (const p of profiles) {
    const item = document.createElement("div");
    item.className = "profile-picker-item";
    item.dataset.profile = p;
    item.setAttribute("role", "option");

    const name = document.createElement("span");
    name.className = "profile-picker-item-name";
    name.textContent = p;

    const check = document.createElement("span");
    check.className = "profile-picker-check";
    check.dataset.profile = p;
    check.hidden = p !== _currentProfileValue;
    check.innerHTML = CHECK_SVG;

    item.appendChild(name);
    item.appendChild(check);
    profilePickerMenu.appendChild(item);

    if (profileSelect) {
      const option = document.createElement("option");
      option.value = p;
      option.textContent = p;
      option.selected = p === _currentProfileValue;
      profileSelect.appendChild(option);
    }
  }
}

function makeMsgGroup(role, { profile = null, timestamp = null, footer = true } = {}) {
  const group = document.createElement("div");
  group.className = "msg-group";
  group.dataset.role = role;

  const avatar = document.createElement("div");
  avatar.className = "msg-avatar";
  if (role === "user") {
    avatar.innerHTML = USER_AVATAR_SVG;
  } else if (role === "tool") {
    avatar.innerHTML = TOOL_AVATAR_SVG;
  } else {
    avatar.innerHTML = ASSISTANT_AVATAR_SVG;
  }

  const body = document.createElement("div");
  body.className = "msg-body";

  const bubble = document.createElement("div");
  bubble.className = "msg-bubble";

  body.appendChild(bubble);
  if (footer) {
    const footerEl = document.createElement("div");
    footerEl.className = "msg-footer";

    const sender = document.createElement("span");
    sender.className = "msg-sender";
    if (role === "user") {
      sender.textContent = t("sender_you");
    } else if (role === "tool") {
      sender.textContent = t("sender_tool");
    } else {
      sender.textContent = t("sender_pikachu");
    }

    const time = document.createElement("span");
    time.className = "msg-time";
    time.textContent = formatTime(timestamp ? new Date(timestamp) : new Date());

    footerEl.appendChild(sender);
    footerEl.appendChild(time);

    if (profile) {
      const badge = document.createElement("span");
      badge.className = "msg-badge";
      badge.textContent = profile;
      footerEl.appendChild(badge);
    }

    if (role === "assistant") {
      const copyBtn = document.createElement("button");
      copyBtn.className = "msg-copy";
      copyBtn.title = t("copy");
      copyBtn.innerHTML = COPY_SVG;
      copyBtn.addEventListener("click", () => {
        navigator.clipboard.writeText(bubble.innerText || "").then(() => {
          copyBtn.innerHTML = CHECK_SVG;
          setTimeout(() => { copyBtn.innerHTML = COPY_SVG; }, 1500);
        });
      });
      footerEl.appendChild(copyBtn);
    }

    body.appendChild(footerEl);
  }
  group.appendChild(avatar);
  group.appendChild(body);

  return { group, bubble };
}

function buildBtwThreadElement(message) {
  const wrapper = document.createElement("div");
  wrapper.className = "btw-thread";
  wrapper.dataset.kind = "btw-thread";
  if (message.stale) wrapper.dataset.stale = "true";
  if (message.pending) wrapper.dataset.pending = "true";

  // Header row: chevron + label + query (clickable to toggle)
  const header = document.createElement("div");
  header.className = "btw-thread-header";
  header.setAttribute("role", "button");
  header.setAttribute("tabindex", "0");

  const chevron = document.createElement("span");
  chevron.className = "btw-thread-chevron";
  chevron.innerHTML =
    '<svg viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><polyline points="3,4.5 6,7.5 9,4.5"/></svg>';

  const label = document.createElement("span");
  label.className = "btw-thread-label";
  label.textContent = "BTW";

  const query = document.createElement("span");
  query.className = "btw-thread-query";
  query.textContent = message.query || "";

  header.appendChild(chevron);
  header.appendChild(label);
  header.appendChild(query);
  wrapper.appendChild(header);

  // Answer (collapsed by default unless pending)
  const answer = document.createElement("div");
  answer.className = "btw-thread-answer";
  if (message.contentHtml) {
    answer.innerHTML = DOMPurify.sanitize(message.contentHtml);
  } else if (message.content) {
    answer.textContent = message.content;
  } else {
    answer.textContent = message.pending ? "…" : "";
  }
  wrapper.appendChild(answer);

  // Toggle on click / Enter
  const toggle = () => {
    const open = wrapper.dataset.open === "true";
    wrapper.dataset.open = String(!open);
  };
  header.addEventListener("click", toggle);
  header.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") { e.preventDefault(); toggle(); }
  });

  // Auto-expand if pending (so user sees it working)
  if (message.pending) wrapper.dataset.open = "true";

  return wrapper;
}

function applyToolOutputContent(contentEl, raw) {
  try {
    const formatted = JSON.stringify(JSON.parse(raw), null, 2);
    contentEl.innerHTML = DOMPurify.sanitize(
      hljs.highlight(formatted, { language: "json" }).value
    );
    contentEl.classList.add("hljs");
  } catch {
    const result = hljs.highlightAuto(raw, ["bash", "yaml", "xml"]);
    if (result.relevance > 5) {
      contentEl.innerHTML = DOMPurify.sanitize(result.value);
      contentEl.classList.add("hljs");
    } else {
      contentEl.textContent = raw;
    }
  }
}

function buildToolOutputElement(message, { headerStyle = "generic", showToolName = true } = {}) {
  const wrapper = document.createElement("div");
  wrapper.className = "msg-tool-output";

  const header = document.createElement("div");
  header.className = "msg-tool-output-header";
  header.setAttribute("role", "button");
  header.setAttribute("tabindex", "0");

  if (headerStyle === "generic" || !message.toolName) {
    const label = document.createElement("span");
    label.className = "msg-tool-output-label";
    label.textContent = t("tool_output");
    header.appendChild(label);
  }

  if (showToolName && message.toolName) {
    const badge = buildToolNameEl(message.toolName);
    badge.classList.add("msg-tool-badge");
    header.appendChild(badge);
  }

  const contentEl = document.createElement("div");
  contentEl.className = "msg-tool-output-content";
  applyToolOutputContent(contentEl, message.content || "");

  const toggle = () => {
    header.classList.toggle("open");
    contentEl.classList.toggle("open");
  };
  header.addEventListener("click", toggle);
  header.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      toggle();
    }
  });

  wrapper.appendChild(header);
  wrapper.appendChild(contentEl);
  return wrapper;
}

function shouldMakeTraceArgumentsExpandable(raw) {
  const text = raw || "";
  if (!text) return false;
  const lineCount = text.split(/\r?\n/).length;
  return lineCount > 1 || text.length > TRACE_ARGUMENT_PREVIEW_CHARS;
}

function getMeaningfulTraceArguments(raw) {
  const text = (raw || "").trim();
  if (!text) return "";

  try {
    const parsed = JSON.parse(text);
    if (parsed == null) return "";
    if (Array.isArray(parsed) && parsed.length === 0) return "";
    if (typeof parsed === "object" && Object.keys(parsed).length === 0) return "";
  } catch {
    // Non-JSON arguments still render as plain text.
  }

  return text;
}

function shouldMakeTraceOutputExpandable(raw) {
  const text = raw || "";
  if (!text) return false;
  const lineCount = text.split(/\r?\n/).length;
  return lineCount > TRACE_OUTPUT_COLLAPSED_LINES || text.length > TRACE_OUTPUT_COLLAPSED_CHARS;
}

function buildTraceArguments(raw) {
  const text = raw || "";
  const expandable = shouldMakeTraceArgumentsExpandable(text);
  const args = document.createElement(expandable ? "button" : "span");
  args.className = "msg-trace-args";
  args.textContent = text;

  if (!expandable) {
    return args;
  }

  args.type = "button";
  args.dataset.expanded = "false";
  args.setAttribute("aria-expanded", "false");
  args.addEventListener("click", (event) => {
    event.stopPropagation();
    const expanded = args.dataset.expanded === "true";
    args.dataset.expanded = String(!expanded);
    args.setAttribute("aria-expanded", String(!expanded));
  });
  args.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === " ") {
      event.stopPropagation();
    }
  });

  return args;
}

function buildTraceCode(raw, className, { expandable = false } = {}) {
  const wantsOutputUi = className.includes("msg-trace-code--output");
  const isExpandable = expandable && shouldMakeTraceOutputExpandable(raw);
  const wrap = document.createElement("div");
  wrap.className = wantsOutputUi
    ? "msg-trace-code-wrap msg-trace-code-wrap--output"
    : "msg-trace-code-wrap";
  if (wantsOutputUi) {
    wrap.dataset.expandable = String(isExpandable);
  }

  const block = document.createElement("div");
  block.className = className;
  applyToolOutputContent(block, raw || "");

  if (isExpandable) {
    const reveal = document.createElement("div");
    reveal.className = "msg-trace-code-reveal";

    const showMoreBtn = document.createElement("button");
    showMoreBtn.type = "button";
    showMoreBtn.className = "msg-trace-show-more";
    showMoreBtn.textContent = t("show_more");
    showMoreBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      const expanded = wrap.dataset.expanded === "true";
      wrap.dataset.expanded = String(!expanded);
      showMoreBtn.textContent = expanded ? t("show_more") : t("show_less");
    });
    reveal.appendChild(showMoreBtn);

    wrap.appendChild(block);
    wrap.appendChild(reveal);
    wrap.dataset.expanded = "false";
    return wrap;
  }

  wrap.appendChild(block);
  return wrap;
}

function buildTraceItem(toolCall, outputs = [], { collapsible = true } = {}) {
  const item = document.createElement("div");
  item.className = "msg-trace-item";

  const argumentsText = getMeaningfulTraceArguments(toolCall?.arguments);
  const hasArguments = Boolean(argumentsText);
  const outputText = outputs
    .map((output) => output.content || "")
    .filter(Boolean)
    .join("\n\n");
  const hasOutput = Boolean(outputText);
  const hasContent = hasArguments || hasOutput;

  item.dataset.open = "false";
  if (!collapsible || !hasOutput) {
    item.dataset.static = "true";
  }

  const header = document.createElement("div");
  header.className = "msg-trace-header";
  if (collapsible && hasOutput) {
    header.setAttribute("role", "button");
    header.setAttribute("tabindex", "0");
  }

  const rawName = toolCall?.name || outputs[0]?.toolName || "";
  const title = rawName
    ? buildToolNameEl(rawName)
    : (() => { const s = document.createElement("span"); s.textContent = t("tool_output"); return s; })();
  title.classList.add("msg-trace-title");
  header.appendChild(title);

  if (hasArguments) {
    header.appendChild(buildTraceArguments(argumentsText));
  }

  const body = document.createElement("div");
  body.className = "msg-trace-item-body";

  if (hasOutput) {
    body.appendChild(
      buildTraceCode(outputText, "msg-trace-code msg-trace-code--output", {
        expandable: true,
      })
    );
  }

  if (!hasContent) {
    item.dataset.empty = "true";
  }

  if (collapsible && hasOutput) {
    const toggle = () => {
      const open = item.dataset.open === "true";
      item.dataset.open = String(!open);
    };
    header.addEventListener("click", toggle);
    header.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        toggle();
      }
    });
  }

  item.appendChild(header);
  if (hasOutput) {
    item.appendChild(body);
  }
  return item;
}

function pairToolCallsWithOutputs(toolCalls, toolOutputs) {
  const usedOutputs = new Set();
  const pairs = (toolCalls || []).map((toolCall, index) => {
    const matchedOutputs = [];
    for (const [outputIndex, toolOutput] of toolOutputs.entries()) {
      if (usedOutputs.has(outputIndex)) continue;
      const idsMatch =
        toolCall.id &&
        toolOutput.toolCallId &&
        toolCall.id === toolOutput.toolCallId;
      const fallbackMatch = !toolCall.id && outputIndex === index;
      if (idsMatch || fallbackMatch) {
        matchedOutputs.push(toolOutput);
        usedOutputs.add(outputIndex);
        if (!toolCall.id) break;
      }
    }
    return { toolCall, outputs: matchedOutputs };
  });

  for (const [outputIndex, toolOutput] of toolOutputs.entries()) {
    if (usedOutputs.has(outputIndex)) continue;
    pairs.push({ toolCall: null, outputs: [toolOutput] });
  }

  return pairs;
}

function buildAssistantTrace(message, toolOutputs) {
  const trace = document.createElement("div");
  trace.className = "msg-trace";

  const pairs = pairToolCallsWithOutputs(message.toolCalls || [], toolOutputs);
  for (const pair of pairs) {
    trace.appendChild(buildTraceItem(pair.toolCall, pair.outputs, { collapsible: true }));
  }

  return trace;
}

function buildAssistantIntent(message, trace) {
  const intent = document.createElement("div");
  intent.className = "msg-intent";
  intent.dataset.open = "true";

  const header = document.createElement("div");
  header.className = "msg-intent-header";
  header.setAttribute("role", "button");
  header.setAttribute("tabindex", "0");

  const icon = document.createElement("span");
  icon.className = "msg-intent-icon";
  icon.innerHTML = CHEVRON_RIGHT_SVG;

  const text = document.createElement("div");
  text.className = "msg-intent-text";
  if (message.contentHtml) {
    text.innerHTML = DOMPurify.sanitize(message.contentHtml);
  } else {
    text.textContent = message.content || "";
  }
  header.appendChild(icon);
  header.appendChild(text);

  const details = document.createElement("div");
  details.className = "msg-intent-details";
  details.appendChild(trace);

  const toggle = () => {
    const open = intent.dataset.open === "true";
    intent.dataset.open = String(!open);
  };
  header.addEventListener("click", toggle);
  header.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      toggle();
    }
  });

  intent.appendChild(header);
  intent.appendChild(details);
  return intent;
}

function buildAssistantElement(message, activeProfile, toolOutputs = []) {
  const ts = message.timestamp || null;
  const hasToolCalls = message.toolCalls && message.toolCalls.length > 0;
  const hasContent = Boolean(message.contentHtml || message.content);
  const hasTrace = hasToolCalls && (toolOutputs.length > 0 || message.toolCalls.length > 0);
  const { group, bubble } = makeMsgGroup("assistant", {
    profile: activeProfile || null,
    timestamp: ts,
    footer: !hasTrace,
  });

  if (hasTrace && hasContent) {
    group.dataset.activity = "true";
    bubble.appendChild(buildAssistantIntent(message, buildAssistantTrace(message, toolOutputs)));
    return group;
  }

  if (message.contentHtml) {
    const contentDiv = document.createElement("div");
    contentDiv.className = "msg-content";
    contentDiv.innerHTML = message.contentHtml;
    contentDiv.innerHTML = DOMPurify.sanitize(contentDiv.innerHTML);
    bubble.appendChild(contentDiv);
  } else if (message.content) {
    const contentDiv = document.createElement("div");
    contentDiv.className = "msg-content";
    contentDiv.textContent = message.content;
    bubble.appendChild(contentDiv);
  } else if (hasToolCalls && message.toolCalls.length > 1) {
    const summaryDiv = document.createElement("div");
    summaryDiv.className = "msg-content msg-content--trace-summary";
    summaryDiv.textContent = tToolCount(message.toolCalls.length);
    bubble.appendChild(summaryDiv);
  }

  if (hasTrace) {
    group.dataset.activity = "true";
    bubble.appendChild(buildAssistantTrace(message, toolOutputs));
  }

  return group;
}

function buildMessageElement(message, activeProfile) {
  if (message.kind === "btw_thread") {
    return buildBtwThreadElement(message);
  }
  const ts = message.timestamp || null;
  if (message.role === "user") {
    const { group, bubble } = makeMsgGroup("user", { timestamp: ts });
    bubble.textContent = message.content || "";
    return group;
  } else if (message.role === "assistant") {
    return buildAssistantElement(message, activeProfile);
  } else if (message.role === "tool") {
    const { group, bubble } = makeMsgGroup("tool", { timestamp: ts });
    group.dataset.trace = "true";
    bubble.classList.add("msg-bubble--trace");
    bubble.appendChild(buildToolOutputElement(message));
    return group;
  }
  return null;
}

// Appends a single message to transcript (used for live/incremental updates).
export function renderMessage(message, activeProfile) {
  const el = buildMessageElement(message, activeProfile);
  if (el) {
    transcript.appendChild(el);
    transcript.scrollTop = transcript.scrollHeight;
  }
}

export function appendMessage(role, content) {
  const { group, bubble } = makeMsgGroup(role);
  bubble.textContent = content;
  transcript.appendChild(group);
  transcript.scrollTop = transcript.scrollHeight;
}

export function appendAssistantMessage(content) {
  const { group, bubble } = makeMsgGroup("assistant");
  bubble.innerHTML = content;
  bubble.innerHTML = DOMPurify.sanitize(bubble.innerHTML);
  transcript.appendChild(group);
  transcript.scrollTop = transcript.scrollHeight;
}

function batchRender(messages, activeProfile) {
  const frag = document.createDocumentFragment();
  for (let index = 0; index < messages.length; index += 1) {
    const message = messages[index];
    if (message.role === "assistant" && message.toolCalls && message.toolCalls.length > 0) {
      const toolOutputs = [];
      let nextIndex = index + 1;
      while (nextIndex < messages.length && messages[nextIndex].role === "tool") {
        toolOutputs.push(messages[nextIndex]);
        nextIndex += 1;
      }
      const el = buildAssistantElement(message, activeProfile || "", toolOutputs);
      if (el) frag.appendChild(el);
      index = nextIndex - 1;
      continue;
    }

    const el = buildMessageElement(message, activeProfile || "");
    if (el) frag.appendChild(el);
  }
  transcript.innerHTML = "";
  transcript.appendChild(frag);
  transcript.scrollTop = transcript.scrollHeight;
}

export function renderEmptyState() {
  transcript.innerHTML = "";
  const wrap = document.createElement("div");
  wrap.className = "transcript-empty";
  const icon = document.createElement("div");
  icon.className = "transcript-empty-icon";
  icon.innerHTML = `<svg viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M6 34V10a4 4 0 0 1 4-4h28a4 4 0 0 1 4 4v16a4 4 0 0 1-4 4H14L6 34z"/><line x1="16" y1="19" x2="32" y2="19"/><line x1="16" y1="25" x2="24" y2="25"/></svg>`;
  const title = document.createElement("div");
  title.className = "transcript-empty-title";
  title.textContent = t("no_session_title");
  const hint = document.createElement("div");
  hint.className = "transcript-empty-hint";
  hint.textContent = t("no_session_hint");
  wrap.appendChild(icon);
  wrap.appendChild(title);
  wrap.appendChild(hint);
  transcript.appendChild(wrap);
}

export function renderTranscript(messages, activeProfile) {
  if (!messages.length) {
    transcript.innerHTML = "";
    return;
  }
  batchRender(messages, activeProfile);
}

// Returns the messages array so callers can store it.
export function renderSessionDetail(detail) {
  const activeProfile = detail.activeProfile || "";
  const messages = detail.messages || [];
  if (!messages.length) {
    transcript.innerHTML = "";
    return messages;
  }
  // for (const message of messages) renderMessage(message, activeProfile);
  batchRender(messages, activeProfile);
  return messages;
}

export function renderSessionSelect(groups, currentChannel, currentSessionId) {
  const prev = sessionSelect.value;
  sessionSelect.innerHTML = "";
  const newOpt = document.createElement("option");
  newOpt.value = "__new__";
  newOpt.textContent = t("new_chat");
  sessionSelect.appendChild(newOpt);
  for (const group of groups) {
    const optgroup = document.createElement("optgroup");
    optgroup.label = tChannel(group.channel);
    for (const session of group.sessions || []) {
      const opt = document.createElement("option");
      opt.value = `${session.channel}::${session.sessionId}`;
      opt.textContent = session.preview
        ? `${session.sessionId} \u2014 ${session.preview}`
        : session.sessionId;
      if (session.channel === currentChannel && session.sessionId === currentSessionId) {
        opt.selected = true;
      }
      optgroup.appendChild(opt);
    }
    sessionSelect.appendChild(optgroup);
  }
  if (!sessionSelect.value && prev) {
    sessionSelect.value = prev;
  }
}

export function renderWeixinAccount(account) {
  const enabled = account?.enabled === true;
  const loggedIn = account?.loggedIn === true;
  const expired = account?.expired === true;
  const userId = account?.userId || account?.botId || t("login_from_console");
  weixinLoginButton.disabled = !enabled || loggedIn;
  weixinLogoutButton.disabled = !enabled || !loggedIn;

  if (!enabled) {
    if (weixinStatusGroup) weixinStatusGroup.dataset.state = "disabled";
    weixinStatusLabel.textContent = t("weixin_disabled");
    weixinUserLabel.textContent = t("enable_weixin");
    if (weixinStatusGroup) {
      weixinStatusGroup.title = t("weixin_disabled");
      weixinStatusGroup.setAttribute("aria-label", t("weixin_disabled"));
    }
    weixinQrPanel.hidden = true;
    weixinQrImage.src = "";
    return;
  }

  if (loggedIn && !expired) {
    if (weixinStatusGroup) weixinStatusGroup.dataset.state = "connected";
    weixinStatusLabel.textContent = t("connected");
    weixinUserLabel.textContent = userId;
    if (weixinStatusGroup) {
      weixinStatusGroup.title = t("connected");
      weixinStatusGroup.setAttribute("aria-label", t("connected"));
    }
    weixinQrPanel.hidden = true;
    weixinQrImage.src = "";
    return;
  }

  if (expired) {
    if (weixinStatusGroup) weixinStatusGroup.dataset.state = "warning";
    weixinStatusLabel.textContent = t("login_expired");
    weixinUserLabel.textContent = userId;
    if (weixinStatusGroup) {
      weixinStatusGroup.title = t("login_expired");
      weixinStatusGroup.setAttribute("aria-label", t("login_expired"));
    }
    weixinQrPanel.hidden = true;
    weixinQrImage.src = "";
    return;
  }

  if (weixinStatusGroup) weixinStatusGroup.dataset.state = "disabled";
  weixinQrPanel.hidden = true;
  weixinQrImage.src = "";
  weixinStatusLabel.textContent = t("not_connected");
  weixinUserLabel.textContent = userId;
  if (weixinStatusGroup) {
    weixinStatusGroup.title = t("not_connected");
    weixinStatusGroup.setAttribute("aria-label", t("not_connected"));
  }
}

function relativeTime(dateStr) {
  if (!dateStr) return "";
  const diff = Date.now() - new Date(dateStr).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return t("time_just_now");
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d`;
  return new Date(dateStr).toLocaleDateString([], { month: "short", day: "numeric" });
}

export function renderSessionsList(groups, currentChannel, currentSessionId, filterText) {
  const listEl = document.getElementById("sessions-list");
  const query = (filterText || "").toLowerCase().trim();
  const frag = document.createDocumentFragment();

  // Flatten all sessions, attach channel, sort by updatedAt desc
  const allSessions = [];
  for (const group of groups) {
    for (const session of group.sessions || []) {
      allSessions.push({ ...session, channel: session.channel || group.channel });
    }
  }
  allSessions.sort((a, b) => {
    const ta = a.updatedAt ? new Date(a.updatedAt).getTime() : 0;
    const tb = b.updatedAt ? new Date(b.updatedAt).getTime() : 0;
    return tb - ta;
  });

  const filtered = query
    ? allSessions.filter(
        (s) =>
          (s.sessionId || "").toLowerCase().includes(query) ||
          (s.preview || "").toLowerCase().includes(query) ||
          (s.channel || "").toLowerCase().includes(query)
      )
    : allSessions;

  for (const session of filtered) {
    const item = document.createElement("div");
    item.className = "session-item";
    item.dataset.channel = session.channel;
    item.dataset.sessionId = session.sessionId;
    item.dataset.active = String(
      session.channel === currentChannel && session.sessionId === currentSessionId
    );
    item.setAttribute("role", "button");
    item.setAttribute("tabindex", "0");

    // Top row: preview + channel badge
    const topRow = document.createElement("div");
    topRow.className = "session-item-top";

    const previewEl = document.createElement("div");
    previewEl.className = "session-item-preview";
    previewEl.textContent = session.preview || session.sessionId;
    topRow.appendChild(previewEl);

    const badge = document.createElement("span");
    badge.className = "session-channel-badge";
    badge.dataset.channel = session.channel;
    badge.textContent = tChannel(session.channel);
    topRow.appendChild(badge);

    // Bottom row: session ID + relative time
    const bottomRow = document.createElement("div");
    bottomRow.className = "session-item-bottom";

    const idEl = document.createElement("div");
    idEl.className = "session-item-id";
    idEl.textContent = session.sessionId;
    bottomRow.appendChild(idEl);

    const timeEl = document.createElement("span");
    timeEl.className = "session-item-time";
    timeEl.textContent = relativeTime(session.updatedAt);
    bottomRow.appendChild(timeEl);

    const deleteBtn = document.createElement("button");
    deleteBtn.className = "session-delete-btn";
    deleteBtn.dataset.channel = session.channel;
    deleteBtn.dataset.sessionId = session.sessionId;
    deleteBtn.setAttribute("aria-label", t("session_delete_confirm"));
    deleteBtn.innerHTML =
      '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><polyline points="3,4 13,4"/><path d="M5 4V3a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v1"/><path d="M6 7v5M10 7v5"/><rect x="3" y="4" width="10" height="9" rx="1"/></svg>';

    item.appendChild(topRow);
    item.appendChild(bottomRow);
    item.appendChild(deleteBtn);
    frag.appendChild(item);
  }

  if (filtered.length === 0) {
    const empty = document.createElement("div");
    empty.className = "sessions-empty";
    empty.textContent = t("sessions_empty");
    frag.appendChild(empty);
  }

  listEl.innerHTML = "";
  listEl.appendChild(frag);
}
