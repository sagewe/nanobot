import { t, getLang, setLang, applyI18n } from "./i18n.js";
import {
  fetchSessions,
  fetchSessionDetail,
  createSession,
  duplicateSession,
  setSessionProfile,
  sendChat,
  fetchWeixinAccount,
  startWeixinLogin,
  fetchWeixinLoginStatus,
  logoutWeixin,
  loadProfiles,
} from "./api.js";
import {
  setStatus,
  setCurrentProfile,
  renderProfiles,
  renderMessage,
  appendMessage,
  appendAssistantMessage,
  renderTranscript,
  renderSessionDetail,
  renderSessionSelect,
  renderWeixinAccount,
  normalizeWeixinQrSource,
} from "./render.js";

applyI18n();

// ── Storage keys ──────────────────────────────────────────────────────────────
const SESSION_KEY = "pikachu.sessionId";
const SELECTED_CHANNEL_KEY = "pikachu.selectedChannel";
const SELECTED_SESSION_KEY = "pikachu.selectedSessionId";
const THEME_KEY = "pikachu.theme";
const COLLAPSED_KEY = "pikachu.sidebarCollapsed";

// ── DOM references ────────────────────────────────────────────────────────────
const composer = document.getElementById("composer");
const sessionSelect = document.getElementById("session-select");
const profileSelect = document.getElementById("profile-select");
const messageInput = document.getElementById("message-input");
const sendButton = document.getElementById("send-button");
const duplicateButton = document.getElementById("duplicate-session-button");
const exportButton = document.getElementById("export-button");
const weixinQrPanel = document.getElementById("weixin-qr-panel");
const weixinQrImage = document.getElementById("weixin-qr-image");
const weixinStatusLabel = document.getElementById("weixin-status-label");
const weixinUserLabel = document.getElementById("weixin-user-label");
const weixinLoginButton = document.getElementById("weixin-login-button");
const weixinLogoutButton = document.getElementById("weixin-logout-button");
const themeToggle = document.getElementById("theme-toggle");
const themeIcon = document.getElementById("theme-icon");
const sidebarToggle = document.getElementById("sidebar-toggle");
const backdropEl = document.getElementById("sidebar-backdrop");
const mobileMenuBtn = document.getElementById("mobile-menu-btn");
const langToggleBtn = document.getElementById("lang-toggle");
const tabButtons = document.querySelectorAll(".tab-btn");
const conversationPane = document.querySelector(".conversation-pane");
const channelsPane = document.querySelector(".channels-pane");
const sessionRail = document.querySelector(".session-rail");
const transcript = document.getElementById("transcript");

const legacyStoredSessionId = localStorage.getItem(SESSION_KEY);

// ── App state ─────────────────────────────────────────────────────────────────
let currentChannel = null;
let currentSessionId = null;
let currentSessionReadOnly = false;
let currentSessionCanDuplicate = false;
let currentSessionGroups = [];
let currentMessages = [];
let pendingSelectionToken = 0;
let weixinPollTimer = null;
let isBusy = false;

// ── State helpers ─────────────────────────────────────────────────────────────
function setBusy(busy) {
  isBusy = busy;
  sendButton.disabled = busy || currentSessionReadOnly;
  sessionSelect.disabled = busy;
  duplicateButton.disabled = busy;
}

function setComposerAccess(readOnly, canDuplicate) {
  currentSessionReadOnly = readOnly;
  currentSessionCanDuplicate = canDuplicate;
  messageInput.disabled = readOnly;
  sendButton.disabled = isBusy || currentSessionReadOnly;
  duplicateButton.hidden = !canDuplicate;
  duplicateButton.disabled = isBusy;
}

function setSelectedSession(channel, sessionId) {
  currentChannel = channel;
  currentSessionId = sessionId;
  if (channel && sessionId) {
    localStorage.setItem(SELECTED_CHANNEL_KEY, channel);
    localStorage.setItem(SELECTED_SESSION_KEY, sessionId);
    localStorage.setItem(SESSION_KEY, sessionId);
  } else {
    localStorage.removeItem(SELECTED_CHANNEL_KEY);
    localStorage.removeItem(SELECTED_SESSION_KEY);
    localStorage.removeItem(SESSION_KEY);
  }
}

function findSession(groups, channel, sessionId) {
  if (!channel || !sessionId) return null;
  for (const group of groups) {
    for (const session of group.sessions || []) {
      if (session.channel === channel && session.sessionId === sessionId) {
        return session;
      }
    }
  }
  return null;
}

function findLatestWritableWebSession(groups) {
  const webSessions = groups
    .flatMap((group) =>
      (group.sessions || []).map((session) => ({
        ...session,
        channel: session.channel || group.channel,
      }))
    )
    .filter((session) => session.channel === "web" && session.canSend);
  return webSessions[0] || null;
}

function updateSessionMetadata(channel, sessionId, activeProfile) {
  currentSessionGroups = currentSessionGroups.map((group) => ({
    ...group,
    sessions: (group.sessions || []).map((session) => {
      if (session.channel !== channel || session.sessionId !== sessionId) return session;
      return { ...session, activeProfile: activeProfile || session.activeProfile };
    }),
  }));
  renderSessionSelect(currentSessionGroups, currentChannel, currentSessionId);
}

// ── Session management ────────────────────────────────────────────────────────
async function refreshSessions() {
  currentSessionGroups = await fetchSessions();
  renderSessionSelect(currentSessionGroups, currentChannel, currentSessionId);
  if (currentChannel && currentSessionId && !findSession(currentSessionGroups, currentChannel, currentSessionId)) {
    setSelectedSession(null, null);
  }
  return currentSessionGroups;
}

async function selectSession(channel, sessionId) {
  const selectionToken = ++pendingSelectionToken;
  const detail = await fetchSessionDetail(channel, sessionId);
  if (selectionToken !== pendingSelectionToken) return;
  setSelectedSession(channel, sessionId);
  currentMessages = renderSessionDetail(detail);
  setCurrentProfile(detail.activeProfile || "");
  setComposerAccess(detail.readOnly === true, detail.canDuplicate === true);
  renderSessionSelect(currentSessionGroups, currentChannel, currentSessionId);
}

async function bootstrapSessions() {
  const storedChannel = localStorage.getItem(SELECTED_CHANNEL_KEY);
  const storedSessionId = localStorage.getItem(SELECTED_SESSION_KEY);
  const restoredSessionId = storedSessionId || legacyStoredSessionId;
  const sessions = await fetchSessions();
  currentSessionGroups = sessions;
  renderSessionSelect(currentSessionGroups, currentChannel, currentSessionId);
  const storedSession = findSession(sessions, storedChannel || "web", restoredSessionId);
  const initialSession = storedSession || findLatestWritableWebSession(sessions);
  if (!initialSession) {
    setSelectedSession(null, null);
    const created = await createSession();
    await refreshSessions();
    await selectSession(created.channel || "web", created.sessionId);
    return;
  }
  await selectSession(initialSession.channel, initialSession.sessionId);
}

// ── Weixin polling state machine ──────────────────────────────────────────────
function clearWeixinPollTimer() {
  if (weixinPollTimer) {
    clearTimeout(weixinPollTimer);
    weixinPollTimer = null;
  }
}

function scheduleWeixinPoll() {
  clearWeixinPollTimer();
  weixinPollTimer = setTimeout(() => pollWeixinLoginStatus(), 1500);
}

async function pollWeixinLoginStatus() {
  try {
    const payload = await fetchWeixinLoginStatus();

    if (payload.status === "confirmed") {
      weixinStatusLabel.textContent = t("connected");
      weixinQrPanel.hidden = true;
      clearWeixinPollTimer();
      renderWeixinAccount(await fetchWeixinAccount());
      await refreshSessions();
      return;
    }

    if (payload.expired === true || payload.status === "expired") {
      weixinStatusLabel.textContent = t("login_expired");
      weixinUserLabel.textContent = t("refresh_qr");
      clearWeixinPollTimer();
      renderWeixinAccount(await fetchWeixinAccount());
      return;
    }

    if (payload.status === "scaned") {
      weixinStatusLabel.textContent = t("qr_scanned");
      weixinUserLabel.textContent = t("confirm_login_weixin");
    } else {
      weixinStatusLabel.textContent = t("waiting_for_scan");
    }

    scheduleWeixinPoll();
  } catch (error) {
    clearWeixinPollTimer();
    setStatus(error?.message || t("failed_poll_weixin"), "error");
    fetchWeixinAccount().then(renderWeixinAccount).catch(() => {});
  }
}

// ── Export ────────────────────────────────────────────────────────────────────
function exportConversation() {
  if (!currentMessages.length) return;
  const lines = [];
  for (const msg of currentMessages) {
    if (msg.role === "user") {
      lines.push(`**User**\n\n${msg.content || ""}\n`);
    } else if (msg.role === "assistant") {
      lines.push(`**Assistant**\n\n${msg.content || ""}\n`);
    } else if (msg.role === "tool") {
      const name = msg.toolName ? ` (${msg.toolName})` : "";
      lines.push(`**Tool output${name}**\n\n\`\`\`\n${msg.content || ""}\n\`\`\`\n`);
    }
  }
  const md = lines.join("\n---\n\n");
  const blob = new Blob([md], { type: "text/markdown;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `${t("export_filename")}-${currentSessionId || t("export_filename")}.md`;
  a.click();
  URL.revokeObjectURL(url);
}

// ── Theme ─────────────────────────────────────────────────────────────────────
const SUN_ICON = '<circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/>';
const MOON_ICON = '<path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/>';

function applyTheme(theme) {
  if (theme === "dark") {
    document.documentElement.setAttribute("data-theme", "dark");
    themeIcon.innerHTML = SUN_ICON;
    themeToggle.title = t("switch_light");
  } else {
    document.documentElement.setAttribute("data-theme", "light");
    themeIcon.innerHTML = MOON_ICON;
    themeToggle.title = t("switch_dark");
  }
  localStorage.setItem(THEME_KEY, theme);
}

const savedTheme =
  localStorage.getItem(THEME_KEY) ||
  (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
applyTheme(savedTheme);

// ── Sidebar ───────────────────────────────────────────────────────────────────
function isMobile() {
  return window.matchMedia("(max-width: 640px)").matches;
}

function setSidebarCollapsed(collapsed) {
  sessionRail.dataset.collapsed = String(collapsed);
  localStorage.setItem(COLLAPSED_KEY, String(collapsed));
}

function openMobileSidebar() {
  sessionRail.classList.add("mobile-open");
  backdropEl.classList.add("visible");
}

function closeMobileSidebar() {
  sessionRail.classList.remove("mobile-open");
  backdropEl.classList.remove("visible");
}

setSidebarCollapsed(localStorage.getItem(COLLAPSED_KEY) === "true");

// ── Event listeners ───────────────────────────────────────────────────────────
themeToggle.addEventListener("click", () => {
  const current = document.documentElement.getAttribute("data-theme");
  applyTheme(current === "dark" ? "light" : "dark");
});

sidebarToggle.addEventListener("click", () => {
  if (isMobile()) {
    closeMobileSidebar();
  } else {
    setSidebarCollapsed(sessionRail.dataset.collapsed !== "true");
  }
});

mobileMenuBtn.addEventListener("click", openMobileSidebar);
backdropEl.addEventListener("click", closeMobileSidebar);

tabButtons.forEach((btn) => {
  btn.addEventListener("click", () => {
    const tab = btn.dataset.tab;
    tabButtons.forEach((b) => { b.dataset.active = String(b.dataset.tab === tab); });
    conversationPane.hidden = tab !== "chat";
    channelsPane.hidden = tab !== "channels";
  });
});

langToggleBtn.addEventListener("click", () => {
  setLang(getLang() === "en" ? "zh" : "en");
  applyI18n();
  applyTheme(document.documentElement.getAttribute("data-theme") || "light");
  renderSessionSelect(currentSessionGroups, currentChannel, currentSessionId);
});

profileSelect.addEventListener("change", async () => {
  const profile = profileSelect.value;
  if (!profile || !currentChannel || !currentSessionId) return;
  try {
    await setSessionProfile(currentChannel, currentSessionId, profile);
  } catch (_) {}
});

sessionSelect.addEventListener("change", async () => {
  if (sessionSelect.value === "__new__") {
    setBusy(true);
    setStatus(t("starting_session"), "loading");
    try {
      setSelectedSession(null, null);
      const created = await createSession();
      await refreshSessions();
      await selectSession(created.channel || "web", created.sessionId);
      setStatus(t("session_started"), "idle");
    } catch (error) {
      setStatus(error?.message || t("failed_create_session"), "error");
    } finally {
      setBusy(false);
      messageInput.focus();
    }
    return;
  }
  const [channel, sessionId] = sessionSelect.value.split("::");
  if (channel && sessionId) {
    await selectSession(channel, sessionId);
    if (isMobile()) closeMobileSidebar();
    messageInput.focus();
  }
});

duplicateButton.addEventListener("click", async () => {
  if (!currentSessionId || !currentSessionCanDuplicate) return;
  setBusy(true);
  setStatus(t("duplicating_session"), "loading");
  try {
    const duplicated = await duplicateSession(currentChannel, currentSessionId);
    await refreshSessions();
    await selectSession(duplicated.channel, duplicated.sessionId);
    setStatus(t("session_duplicated"), "idle");
  } catch (error) {
    setStatus(error?.message || t("failed_duplicate_session"), "error");
  } finally {
    setBusy(false);
    messageInput.focus();
  }
});

exportButton.addEventListener("click", exportConversation);

weixinLoginButton.addEventListener("click", async () => {
  clearWeixinPollTimer();
  weixinQrPanel.hidden = true;
  try {
    setStatus(t("starting_weixin_login"), "loading");
    const payload = await startWeixinLogin();
    weixinQrPanel.hidden = false;
    weixinQrImage.src = normalizeWeixinQrSource(payload.qrcodeImgContent || "");
    weixinStatusLabel.textContent = t("waiting_for_scan");
    weixinUserLabel.textContent = t("scan_qr_weixin");
    scheduleWeixinPoll();
    setStatus(t("scan_weixin_qr_continue"), "idle");
  } catch (error) {
    weixinQrPanel.hidden = true;
    setStatus(error?.message || t("failed_start_weixin"), "error");
    fetchWeixinAccount().then(renderWeixinAccount).catch(() => {});
  }
});

weixinLogoutButton.addEventListener("click", async () => {
  clearWeixinPollTimer();
  try {
    setStatus(t("disconnecting_weixin"), "loading");
    const payload = await logoutWeixin();
    weixinQrPanel.hidden = true;
    weixinQrImage.src = "";
    renderWeixinAccount(payload);
    renderWeixinAccount(await fetchWeixinAccount());
    await refreshSessions();
    setStatus(t("weixin_disconnected"), "idle");
  } catch (error) {
    setStatus(error?.message || t("failed_logout_weixin"), "error");
  }
});

messageInput.addEventListener("focus", () => {
  if (isMobile()) {
    setTimeout(() => messageInput.scrollIntoView({ behavior: "smooth", block: "nearest" }), 300);
  }
});

(function () {
  const sessionHeader = document.querySelector(".session-header");
  const composerEl = document.getElementById("composer");
  function hideChrome() {
    sessionHeader.classList.add("scroll-hidden");
    composerEl.classList.add("scroll-hidden");
  }
  transcript.addEventListener("touchmove", () => { if (isMobile()) hideChrome(); }, { passive: true });
  transcript.addEventListener("touchend", () => {
    if (!isMobile()) return;
    sessionHeader.classList.remove("scroll-hidden");
    composerEl.classList.remove("scroll-hidden");
  }, { passive: true });
  transcript.addEventListener("touchcancel", () => {
    if (!isMobile()) return;
    sessionHeader.classList.remove("scroll-hidden");
    composerEl.classList.remove("scroll-hidden");
  }, { passive: true });
})();

messageInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && !event.ctrlKey && !event.metaKey && !event.shiftKey) {
    event.preventDefault();
    composer.requestSubmit();
  }
});

composer.addEventListener("submit", async (event) => {
  event.preventDefault();
  const draft = messageInput.value;
  const message = draft.trim();
  if (!message) {
    setStatus(t("enter_message"), "error");
    messageInput.focus();
    return;
  }
  if (currentSessionReadOnly) {
    setStatus(t("readonly_session"), "error");
    return;
  }

  appendMessage("user", message);
  messageInput.value = "";
  setBusy(true);
  setStatus(t("pikachu_working"), "loading");

  try {
    if (!currentSessionId) {
      setSelectedSession(null, null);
      const created = await createSession();
      await refreshSessions();
      await selectSession(created.channel || "web", created.sessionId);
    }
    const payload = await sendChat(message, currentChannel, currentSessionId);
    setSelectedSession(payload.channel || currentChannel, payload.sessionId);
    await refreshSessions();
    await selectSession(payload.channel || currentChannel, payload.sessionId || currentSessionId);
    setStatus("", "idle");
  } catch (error) {
    if (!messageInput.value.trim()) {
      messageInput.value = draft;
    }
    setStatus(error?.message || t("request_failed"), "error");
  } finally {
    setBusy(false);
    messageInput.focus();
  }
});

// ── Bootstrap ─────────────────────────────────────────────────────────────────
renderTranscript([]);
setComposerAccess(false, false);

Promise.all([
  bootstrapSessions(),
  fetchWeixinAccount().then(renderWeixinAccount),
  loadProfiles().then(renderProfiles),
]).catch((error) => {
  clearWeixinPollTimer();
  setStatus(error?.message || t("failed_load_sessions"), "error");
});
