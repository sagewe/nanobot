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
const statusNode = document.getElementById("status");
const weixinStatusLabel = document.getElementById("weixin-status-label");
const weixinUserLabel = document.getElementById("weixin-user-label");
const weixinQrPanel = document.getElementById("weixin-qr-panel");
const weixinQrImage = document.getElementById("weixin-qr-image");
const weixinLoginButton = document.getElementById("weixin-login-button");
const weixinLogoutButton = document.getElementById("weixin-logout-button");

const USER_AVATAR_SVG = `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>`;
const ASSISTANT_AVATAR_SVG = `<svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2L13.5 10.5L22 12L13.5 13.5L12 22L10.5 13.5L2 12L10.5 10.5Z"/></svg>`;
const TOOL_AVATAR_SVG = `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>`;
const COPY_SVG = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>`;
const CHECK_SVG = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.25" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>`;

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
  if (profile && profileSelect.querySelector(`option[value="${CSS.escape(profile)}"]`)) {
    profileSelect.value = profile;
  }
}

export function renderProfiles(profiles) {
  profileSelect.innerHTML = "";
  for (const p of profiles) {
    const opt = document.createElement("option");
    opt.value = p;
    opt.textContent = p;
    profileSelect.appendChild(opt);
  }
}

function makeMsgGroup(role, { profile = null, timestamp = null } = {}) {
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

  const footer = document.createElement("div");
  footer.className = "msg-footer";

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

  footer.appendChild(sender);
  footer.appendChild(time);

  if (profile) {
    const badge = document.createElement("span");
    badge.className = "msg-badge";
    badge.textContent = profile;
    footer.appendChild(badge);
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
    footer.appendChild(copyBtn);
  }

  body.appendChild(bubble);
  body.appendChild(footer);
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
    const { group, bubble } = makeMsgGroup("assistant", { profile: activeProfile || null, timestamp: ts });
    if (message.toolCalls && message.toolCalls.length > 0) {
      const toolsDiv = document.createElement("div");
      toolsDiv.className = "msg-tool-calls";
      const summary = document.createElement("div");
      summary.className = "msg-tool-summary";
      summary.appendChild(document.createTextNode(tToolCount(message.toolCalls.length)));
      for (const tc of message.toolCalls) {
        const badge = document.createElement("span");
        badge.className = "msg-tool-badge";
        badge.textContent = tc.name;
        summary.appendChild(badge);
      }
      toolsDiv.appendChild(summary);
      bubble.appendChild(toolsDiv);
    }
    if (message.contentHtml) {
      const contentDiv = document.createElement("div");
      if (message.toolCalls && message.toolCalls.length > 0) {
        contentDiv.style.marginTop = "0.6rem";
      }
      contentDiv.innerHTML = DOMPurify.sanitize(message.contentHtml);
      bubble.appendChild(contentDiv);
    } else if (message.content) {
      const contentDiv = document.createElement("div");
      contentDiv.textContent = message.content;
      bubble.appendChild(contentDiv);
    }
    return group;
  } else if (message.role === "tool") {
    const { group, bubble } = makeMsgGroup("tool", { timestamp: ts });
    const header = document.createElement("div");
    header.className = "msg-tool-output-header";
    header.appendChild(document.createTextNode(t("tool_output") + "\u00a0"));
    if (message.toolName) {
      const badge = document.createElement("span");
      badge.className = "msg-tool-badge";
      badge.textContent = message.toolName;
      header.appendChild(badge);
    }
    const contentEl = document.createElement("div");
    contentEl.className = "msg-tool-output-content";
    const raw = message.content || "";
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
    header.addEventListener("click", () => {
      header.classList.toggle("open");
      contentEl.classList.toggle("open");
    });
    bubble.appendChild(header);
    bubble.appendChild(contentEl);
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
  bubble.innerHTML = DOMPurify.sanitize(content);
  transcript.appendChild(group);
  transcript.scrollTop = transcript.scrollHeight;
}

function batchRender(messages, activeProfile) {
  const frag = document.createDocumentFragment();
  for (const message of messages) {
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
    appendAssistantMessage(t("initial_message"));
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
    appendAssistantMessage(t("initial_message"));
    return messages;
  }
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
    weixinStatusLabel.textContent = t("weixin_disabled");
    weixinUserLabel.textContent = t("enable_weixin");
    weixinQrPanel.hidden = true;
    weixinQrImage.src = "";
    return;
  }

  if (loggedIn && !expired) {
    weixinStatusLabel.textContent = t("connected");
    weixinUserLabel.textContent = userId;
    weixinQrPanel.hidden = true;
    weixinQrImage.src = "";
    return;
  }

  if (expired) {
    weixinStatusLabel.textContent = t("login_expired");
    weixinUserLabel.textContent = userId;
    weixinQrPanel.hidden = true;
    weixinQrImage.src = "";
    return;
  }

  weixinQrPanel.hidden = true;
  weixinQrImage.src = "";
  weixinStatusLabel.textContent = t("not_connected");
  weixinUserLabel.textContent = userId;
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
