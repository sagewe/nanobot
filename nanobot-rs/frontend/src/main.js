import { t, getLang, setLang, applyI18n } from "./i18n.js";
import {
  fetchSessions,
  fetchSessionDetail,
  createSession,
  duplicateSession,
  deleteSession,
  setSessionProfile,
  sendChat,
  fetchWeixinAccount,
  startWeixinLogin,
  fetchWeixinLoginStatus,
  logoutWeixin,
  loadProfiles,
  fetchCronJobs,
  addCronJob,
  deleteCronJob,
  toggleCronJob,
  runCronJob,
  fetchMcpServers,
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
  renderSessionsList,
  renderEmptyState,
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
const WIDE_KEY = "pikachu.wideLayout";
const DRAFT_KEY_PREFIX = "pikachu.draft";

// ── DOM references ────────────────────────────────────────────────────────────
const composer = document.getElementById("composer");
const sessionSelect = document.getElementById("session-select");
const profilePickerBtn = document.getElementById("profile-picker-btn");
const profilePickerMenu = document.getElementById("profile-picker-menu");
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
const wideToggle = document.getElementById("wide-toggle");
const wideIcon = document.getElementById("wide-icon");
const sidebarToggle = document.getElementById("sidebar-toggle");
const backdropEl = document.getElementById("sidebar-backdrop");
const mobileMenuBtn = document.getElementById("mobile-menu-btn");
const langToggleBtn = document.getElementById("lang-toggle");
const tabButtons = document.querySelectorAll(".tab-btn");
const conversationPane = document.querySelector(".conversation-pane");
const channelsPane = document.querySelector(".channels-pane");
const sessionsPane = document.querySelector(".sessions-pane");
const jobsPane = document.querySelector(".jobs-pane");
const mcpPane = document.querySelector(".mcp-pane");
const sessionsSearch = document.getElementById("sessions-search");
const sessionRail = document.querySelector(".session-rail");
const transcript = document.getElementById("transcript");
const slashMenu = document.getElementById("slash-menu");
const slashMenuList = document.getElementById("slash-menu-list");

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
let busyTimer = null;
let busyStart = null;
let isBusy = false;
let slashState = { open: false, items: [], selectedIndex: 0 };

const SLASH_COMMANDS = [
  { name: "/new", insertText: "/new", hintKey: "command_new_hint" },
  { name: "/help", insertText: "/help", hintKey: "command_help_hint" },
  { name: "/stop", insertText: "/stop", hintKey: "command_stop_hint" },
  { name: "/models", insertText: "/models", hintKey: "command_models_hint" },
  { name: "/model", insertText: "/model ", hintKey: "command_model_hint" },
  { name: "/btw", insertText: "/btw ", hintKey: "command_btw_hint" },
];

// ── State helpers ─────────────────────────────────────────────────────────────
function startBusyTimer() {
  busyStart = Date.now();
  clearInterval(busyTimer);
  busyTimer = setInterval(() => {
    const secs = Math.floor((Date.now() - busyStart) / 1000);
    setStatus(`${t("pikachu_working")} ${secs}s`, "loading");
  }, 1000);
}

function stopBusyTimer() {
  clearInterval(busyTimer);
  busyTimer = null;
  busyStart = null;
}

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
    renderEmptyState();
  }
}

function draftKey() {
  return currentChannel && currentSessionId
    ? `${DRAFT_KEY_PREFIX}.${currentChannel}.${currentSessionId}`
    : null;
}

function saveDraft() {
  const key = draftKey();
  if (!key) return;
  const val = messageInput.value;
  if (val) localStorage.setItem(key, val);
  else localStorage.removeItem(key);
}

function restoreDraft() {
  const key = draftKey();
  messageInput.value = (key && localStorage.getItem(key)) || "";
  autoResize();
  updateSlashMenu();
}

function clearDraft() {
  const key = draftKey();
  if (key) localStorage.removeItem(key);
}

function syncSessionsList() {
  renderSessionsList(currentSessionGroups, currentChannel, currentSessionId, sessionsSearch.value);
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
  syncSessionsList();
}

function appendEphemeralExchange(message, payload) {
  appendMessage("user", message);
  appendAssistantMessage(payload.replyHtml || payload.reply);
}

function closeSlashMenu() {
  slashState = { open: false, items: [], selectedIndex: 0 };
  slashMenu.hidden = true;
  slashMenuList.innerHTML = "";
}

function renderSlashMenu() {
  slashMenuList.innerHTML = "";
  for (const [index, command] of slashState.items.entries()) {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "slash-item";
    item.dataset.selected = String(index === slashState.selectedIndex);

    const name = document.createElement("span");
    name.className = "slash-name";
    name.textContent = command.name;

    const hint = document.createElement("span");
    hint.className = "slash-hint";
    hint.textContent = t(command.hintKey);

    item.appendChild(name);
    item.appendChild(hint);
    item.addEventListener("mousedown", (event) => event.preventDefault());
    item.addEventListener("click", () => applySlashCommand(command));
    slashMenuList.appendChild(item);
  }
  slashMenu.hidden = !slashState.open;
}

function applySlashCommand(command) {
  messageInput.value = command.insertText;
  saveDraft();
  syncSendState();
  autoResize();
  closeSlashMenu();
  messageInput.focus();
  messageInput.setSelectionRange(messageInput.value.length, messageInput.value.length);
}

function getSlashQuery(value) {
  const match = value.match(/^\s*\/([^\s]*)$/);
  return match ? match[1].toLowerCase() : null;
}

function filterSlashCommands(query) {
  if (!query) return [...SLASH_COMMANDS];
  const prefixMatches = SLASH_COMMANDS.filter((command) =>
    command.name.slice(1).toLowerCase().startsWith(query)
  );
  const containsMatches = SLASH_COMMANDS.filter((command) =>
    !prefixMatches.includes(command) &&
    command.name.slice(1).toLowerCase().includes(query)
  );
  return [...prefixMatches, ...containsMatches];
}

function updateSlashMenu() {
  const query = getSlashQuery(messageInput.value);
  if (query === null) {
    closeSlashMenu();
    return;
  }

  const items = filterSlashCommands(query);
  if (!items.length) {
    closeSlashMenu();
    return;
  }

  slashState = {
    open: true,
    items,
    selectedIndex: Math.min(slashState.selectedIndex, items.length - 1),
  };
  renderSlashMenu();
}

// ── Session management ────────────────────────────────────────────────────────
async function refreshSessions() {
  currentSessionGroups = await fetchSessions();
  renderSessionSelect(currentSessionGroups, currentChannel, currentSessionId);
  syncSessionsList();
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
  syncSessionsList();
  restoreDraft();
}

async function bootstrapSessions() {
  const storedChannel = localStorage.getItem(SELECTED_CHANNEL_KEY);
  const storedSessionId = localStorage.getItem(SELECTED_SESSION_KEY);
  const restoredSessionId = storedSessionId || legacyStoredSessionId;
  const sessions = await fetchSessions();
  currentSessionGroups = sessions;
  renderSessionSelect(currentSessionGroups, currentChannel, currentSessionId);
  syncSessionsList();
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

// ── Wide layout ───────────────────────────────────────────────────────────────
const WIDE_ICON = '<polyline points="15 3 21 3 21 9"/><polyline points="9 21 3 21 3 15"/><line x1="21" y1="3" x2="14" y2="10"/><line x1="3" y1="21" x2="10" y2="14"/>';
const NARROW_ICON = '<polyline points="4 14 10 14 10 20"/><polyline points="20 10 14 10 14 4"/><line x1="10" y1="14" x2="3" y2="21"/><line x1="21" y1="3" x2="14" y2="10"/>';

function applyWide(wide) {
  document.body.setAttribute("data-wide", String(wide));
  wideIcon.innerHTML = wide ? WIDE_ICON : NARROW_ICON;
  wideToggle.title = wide ? t("switch_narrow") : t("switch_wide");
  localStorage.setItem(WIDE_KEY, String(wide));
}

const savedWide = localStorage.getItem(WIDE_KEY) !== "false";
applyWide(savedWide);

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

wideToggle.addEventListener("click", () => {
  applyWide(document.body.getAttribute("data-wide") !== "true");
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

function switchTab(tab) {
  tabButtons.forEach((b) => { b.dataset.active = String(b.dataset.tab === tab); });
  conversationPane.hidden = tab !== "chat";
  sessionsPane.hidden = tab !== "sessions";
  channelsPane.hidden = tab !== "channels";
  jobsPane.hidden = tab !== "jobs";
  mcpPane.hidden = tab !== "mcp";
  if (tab === "jobs") refreshJobs();
  if (tab === "mcp") refreshMcp();
}

tabButtons.forEach((btn) => {
  btn.addEventListener("click", () => switchTab(btn.dataset.tab));
});

sessionsSearch.addEventListener("input", syncSessionsList);

document.getElementById("sessions-list").addEventListener("click", async (e) => {
  // Delete button
  const deleteBtn = e.target.closest(".session-delete-btn");
  if (deleteBtn) {
    e.stopPropagation();
    const { channel, sessionId } = deleteBtn.dataset;
    if (!confirm(t("session_delete_confirm"))) return;
    try {
      await deleteSession(channel, sessionId);
      await refreshSessions();
    } catch (err) {
      alert(err.message);
    }
    return;
  }

  const item = e.target.closest(".session-item");
  if (!item) return;
  const { channel, sessionId } = item.dataset;
  if (!channel || !sessionId) return;
  switchTab("chat");
  await selectSession(channel, sessionId);
  if (isMobile()) closeMobileSidebar();
  messageInput.focus();
});

document.getElementById("sessions-list").addEventListener("keydown", (e) => {
  if (e.key === "Enter" || e.key === " ") {
    e.preventDefault();
    e.target.closest(".session-item")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  }
});

langToggleBtn.addEventListener("click", () => {
  setLang(getLang() === "en" ? "zh" : "en");
  applyI18n();
  applyTheme(document.documentElement.getAttribute("data-theme") || "light");
  applyWide(document.body.getAttribute("data-wide") !== "false");
  renderSessionSelect(currentSessionGroups, currentChannel, currentSessionId);
  syncSessionsList();
});

profilePickerBtn.addEventListener("click", (e) => {
  e.stopPropagation();
  const rect = profilePickerBtn.getBoundingClientRect();
  profilePickerMenu.style.bottom = `${window.innerHeight - rect.top + 6}px`;
  profilePickerMenu.style.right = `${window.innerWidth - rect.right}px`;
  profilePickerMenu.hidden = !profilePickerMenu.hidden;
});

profilePickerMenu.addEventListener("click", async (e) => {
  const item = e.target.closest(".profile-picker-item");
  if (!item) return;
  const profile = item.dataset.profile;
  profilePickerMenu.hidden = true;
  setCurrentProfile(profile);
  if (!profile || !currentChannel || !currentSessionId) return;
  try {
    await setSessionProfile(currentChannel, currentSessionId, profile);
  } catch (_) {}
});

document.addEventListener("click", (event) => {
  profilePickerMenu.hidden = true;
  if (event.target !== messageInput && !slashMenu.contains(event.target)) {
    closeSlashMenu();
  }
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

function syncSendState() {
  sendButton.dataset.empty = messageInput.value.trim() ? "false" : "true";
}

function autoResize() {
  messageInput.style.height = "auto";
  messageInput.style.height = messageInput.scrollHeight + "px";
}

messageInput.addEventListener("input", () => {
  saveDraft();
  syncSendState();
  autoResize();
  updateSlashMenu();
});
syncSendState();

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
  if (slashState.open && slashState.items.length) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      slashState.selectedIndex = (slashState.selectedIndex + 1) % slashState.items.length;
      renderSlashMenu();
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      slashState.selectedIndex =
        (slashState.selectedIndex - 1 + slashState.items.length) % slashState.items.length;
      renderSlashMenu();
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      closeSlashMenu();
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      applySlashCommand(slashState.items[slashState.selectedIndex]);
      return;
    }
  }

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
  autoResize();
  closeSlashMenu();
  clearDraft();
  setBusy(true);
  setStatus(t("pikachu_working"), "loading");
  startBusyTimer();

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
    if (payload.persisted === false) {
      appendEphemeralExchange(message, payload);
    }
    setStatus("", "idle");
  } catch (error) {
    if (!messageInput.value.trim()) {
      messageInput.value = draft;
      saveDraft();
    }
    setStatus(error?.message || t("request_failed"), "error");
  } finally {
    stopBusyTimer();
    setBusy(false);
    messageInput.focus();
  }
});

// ── In-transcript search ───────────────────────────────────────────────────────
const transcriptSearch = document.getElementById("transcript-search");
const searchInput = document.getElementById("search-input");
const searchCount = document.getElementById("search-count");

let searchMatches = [];
let searchIndex = -1;

function openSearch() {
  transcriptSearch.hidden = false;
  searchInput.focus();
  searchInput.select();
}

function closeSearch() {
  transcriptSearch.hidden = true;
  clearSearchHighlights();
  searchMatches = [];
  searchIndex = -1;
  searchCount.textContent = "";
}

function clearSearchHighlights() {
  transcript.querySelectorAll("mark.search-highlight").forEach((mark) => {
    mark.replaceWith(...mark.childNodes);
  });
  transcript.normalize();
}

function performSearch(query) {
  clearSearchHighlights();
  searchMatches = [];
  searchIndex = -1;
  if (!query.trim()) { searchCount.textContent = ""; return; }

  const lower = query.toLowerCase();
  const walker = document.createTreeWalker(transcript, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      if (node.parentElement?.closest("script,style,mark")) return NodeFilter.FILTER_REJECT;
      return node.textContent.toLowerCase().includes(lower)
        ? NodeFilter.FILTER_ACCEPT
        : NodeFilter.FILTER_REJECT;
    },
  });

  const textNodes = [];
  let node;
  while ((node = walker.nextNode())) textNodes.push(node);

  for (const textNode of textNodes) {
    const text = textNode.textContent;
    const textLower = text.toLowerCase();
    let offset = 0;
    const frag = document.createDocumentFragment();
    let pos;
    while ((pos = textLower.indexOf(lower, offset)) !== -1) {
      if (pos > offset) frag.appendChild(document.createTextNode(text.slice(offset, pos)));
      const mark = document.createElement("mark");
      mark.className = "search-highlight";
      mark.textContent = text.slice(pos, pos + query.length);
      frag.appendChild(mark);
      searchMatches.push(mark);
      offset = pos + query.length;
    }
    if (offset < text.length) frag.appendChild(document.createTextNode(text.slice(offset)));
    textNode.replaceWith(frag);
  }

  if (searchMatches.length > 0) {
    navigateSearch(0);
  } else {
    searchCount.textContent = t("search_no_results");
  }
}

function navigateSearch(index) {
  if (!searchMatches.length) return;
  if (searchIndex >= 0) searchMatches[searchIndex]?.classList.remove("search-current");
  searchIndex = ((index % searchMatches.length) + searchMatches.length) % searchMatches.length;
  const current = searchMatches[searchIndex];
  current.classList.add("search-current");
  current.scrollIntoView({ block: "center", behavior: "smooth" });
  searchCount.textContent = `${searchIndex + 1} / ${searchMatches.length}`;
}

searchInput.addEventListener("input", () => performSearch(searchInput.value));
searchInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") { e.preventDefault(); navigateSearch(e.shiftKey ? searchIndex - 1 : searchIndex + 1); }
  else if (e.key === "Escape") closeSearch();
});
document.getElementById("search-prev").addEventListener("click", () => navigateSearch(searchIndex - 1));
document.getElementById("search-next").addEventListener("click", () => navigateSearch(searchIndex + 1));
document.getElementById("search-close").addEventListener("click", closeSearch);

// ── Global keyboard shortcuts ──────────────────────────────────────────────────
document.addEventListener("keydown", (e) => {
  const mod = e.metaKey || e.ctrlKey;
  if (!mod) return;
  switch (e.key.toLowerCase()) {
    case "k":
      e.preventDefault();
      switchTab("sessions");
      sessionsSearch.focus();
      sessionsSearch.select();
      break;
    case "n":
      if (document.activeElement !== messageInput) {
        e.preventDefault();
        sessionSelect.value = "__new__";
        sessionSelect.dispatchEvent(new Event("change"));
      }
      break;
    case "f":
      if (document.activeElement !== messageInput && document.activeElement !== searchInput) {
        e.preventDefault();
        openSearch();
      }
      break;
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

// ── Jobs tab ──────────────────────────────────────────────────────────────────

function escapeHtml(str) {
  return String(str).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

function formatMs(ms) {
  if (!ms) return "—";
  return new Date(ms).toLocaleString();
}

function formatSchedule(schedule) {
  if (!schedule) return "—";
  if (schedule.kind === "Every" && schedule.everyMs) {
    const ms = schedule.everyMs;
    if (ms % 3600000 === 0) return `every ${ms / 3600000}h`;
    if (ms % 60000 === 0) return `every ${ms / 60000}m`;
    if (ms % 1000 === 0) return `every ${ms / 1000}s`;
    return `every ${ms}ms`;
  }
  if (schedule.kind === "Cron" && schedule.expr) {
    return schedule.tz ? `cron: ${schedule.expr} (${schedule.tz})` : `cron: ${schedule.expr}`;
  }
  if (schedule.kind === "At" && schedule.atMs) {
    return `at ${new Date(schedule.atMs).toLocaleString()}`;
  }
  return schedule.kind || "—";
}

function showToast(message, variant = "success") {
  const el = document.createElement("div");
  el.className = `toast toast--${variant}`;
  el.textContent = message;
  document.body.appendChild(el);
  requestAnimationFrame(() => el.classList.add("toast--visible"));
  setTimeout(() => {
    el.classList.remove("toast--visible");
    el.addEventListener("transitionend", () => el.remove());
    setTimeout(() => el.remove(), 500); // fallback
  }, 2400);
}

function renderJobsList(jobs) {
  const addDetails = document.getElementById("jobs-add-details");
  if (!jobs.length) {
    jobsList.innerHTML = `<div class="jobs-empty">${t("jobs_empty")}<br><span class="jobs-empty-hint">${t("jobs_empty_hint")}</span></div>`;
    if (addDetails) addDetails.open = true;
    return;
  }
  if (addDetails && addDetails.open && jobs.length) {
    // keep it open if user just added, otherwise close
  }
  jobsList.innerHTML = jobs.map((job) => {
    const timing = escapeHtml(formatSchedule(job.schedule));
    const nextRun = job.state?.nextRunAtMs ? formatMs(job.state.nextRunAtMs) : "—";
    const lastRun = job.state?.lastRunAtMs ? formatMs(job.state.lastRunAtMs) : "—";
    const enabledLabel = job.enabled ? t("jobs_toggle_disable") : t("jobs_toggle_enable");
    const statusBadge = job.enabled
      ? `<span class="job-badge job-badge--active">on</span>`
      : `<span class="job-badge job-badge--inactive">off</span>`;
    const msgPreview = job.payload?.message
      ? `<div class="job-item-preview">${escapeHtml(job.payload.message)}</div>` : "";
    return `<div class="job-item" data-id="${job.id}">
  <div class="job-item-header">
    <span class="job-name">${escapeHtml(job.name)}</span>
    ${statusBadge}
    <span class="job-timing">${timing}</span>
  </div>
  ${msgPreview}
  <div class="job-item-meta">
    <span>${t("jobs_next_run")}: ${nextRun}</span>
    <span>${t("jobs_last_run")}: ${lastRun}</span>
  </div>
  <div class="job-item-actions">
    <button class="job-run-btn" data-id="${job.id}" title="${t("jobs_run")}">${t("jobs_run")}</button>
    <button class="job-toggle-btn" data-id="${job.id}">${enabledLabel}</button>
    <button class="job-delete-btn" data-id="${job.id}">${t("jobs_delete")}</button>
  </div>
</div>`;
  }).join("");
}

async function refreshJobs() {
  try {
    const jobs = await fetchCronJobs();
    renderJobsList(jobs);
  } catch (_) {
    jobsList.innerHTML = `<div class="jobs-empty">${t("jobs_empty")}</div>`;
  }
}

const jobsRefreshBtn = document.getElementById("jobs-refresh-btn");
jobsRefreshBtn.addEventListener("click", refreshJobs);

const jobsList = document.getElementById("jobs-list");
jobsList.addEventListener("click", async (e) => {
  const runBtn = e.target.closest(".job-run-btn");
  if (runBtn) {
    try {
      await runCronJob(runBtn.dataset.id);
      showToast(t("jobs_run_success"));
      await refreshJobs();
    } catch (err) { showToast(err.message, "error"); }
    return;
  }
  const toggleBtn = e.target.closest(".job-toggle-btn");
  if (toggleBtn) {
    try {
      await toggleCronJob(toggleBtn.dataset.id);
      showToast(t("jobs_toggle_success"));
      await refreshJobs();
    } catch (err) { showToast(err.message, "error"); }
    return;
  }
  const deleteBtn = e.target.closest(".job-delete-btn");
  if (deleteBtn) {
    if (!confirm(t("jobs_delete_confirm"))) return;
    try {
      await deleteCronJob(deleteBtn.dataset.id);
      showToast(t("jobs_delete_success"));
      await refreshJobs();
    } catch (err) { showToast(err.message, "error"); }
  }
});

const addJobForm = document.getElementById("add-job-form");
const jobScheduleType = document.getElementById("job-schedule-type");

// Populate timezone dropdown with search filter
const allTzOptions = [];
{
  const tzSelect = document.getElementById("job-tz");
  const tzSearch = document.getElementById("job-tz-search");
  try {
    const zones = Intl.supportedValuesOf("timeZone");
    const localTz = Intl.DateTimeFormat().resolvedOptions().timeZone;
    const now = new Date();
    for (const tz of zones) {
      const fmt = new Intl.DateTimeFormat("en-US", { timeZone: tz, timeZoneName: "shortOffset" });
      const parts = fmt.formatToParts(now);
      const offsetPart = parts.find(p => p.type === "timeZoneName");
      const offset = offsetPart ? offsetPart.value : "";
      const label = `${tz.replace(/_/g, " ")}  (${offset})`;
      allTzOptions.push({ value: tz, label, isLocal: tz === localTz });
    }
    function renderTzOptions(filter) {
      // keep the auto option
      while (tzSelect.options.length > 1) tzSelect.options[tzSelect.options.length - 1] = null;
      const lc = (filter || "").toLowerCase();
      for (const o of allTzOptions) {
        if (lc && !o.label.toLowerCase().includes(lc) && !o.value.toLowerCase().includes(lc)) continue;
        const opt = document.createElement("option");
        opt.value = o.value;
        opt.textContent = o.label;
        if (!lc && o.isLocal) opt.selected = true;
        tzSelect.appendChild(opt);
      }
    }
    renderTzOptions("");
    tzSearch.addEventListener("input", () => renderTzOptions(tzSearch.value.trim()));
  } catch (_) {
    // Fallback: keep the empty auto option
  }
}

jobScheduleType.addEventListener("change", () => {
  const val = jobScheduleType.value;
  document.getElementById("job-every-label").hidden = val !== "every";
  document.getElementById("job-cron-label").hidden = val !== "cron";
  document.getElementById("job-tz-label").hidden = val !== "cron";
  document.getElementById("job-at-label").hidden = val !== "at";
});

addJobForm.addEventListener("submit", async (e) => {
  e.preventDefault();
  const message = document.getElementById("job-message").value.trim();
  if (!message) return;
  const name = document.getElementById("job-name").value.trim();
  const schedType = document.getElementById("job-schedule-type").value;
  const params = { message };
  if (name) params.name = name;
  if (schedType === "every") {
    const value = parseInt(document.getElementById("job-every-value").value, 10) || 1;
    const unit = parseInt(document.getElementById("job-every-unit").value, 10) || 3600;
    params.every_seconds = value * unit;
  } else if (schedType === "cron") {
    params.cron_expr = document.getElementById("job-cron-expr").value.trim();
    const tz = document.getElementById("job-tz").value;
    if (tz) params.tz = tz;
  } else if (schedType === "at") {
    const dtVal = document.getElementById("job-at").value;
    if (dtVal) {
      params.at = new Date(dtVal).toISOString();
    }
  }
  try {
    await addCronJob(params);
    addJobForm.reset();
    showToast(t("jobs_add_success"));
    await refreshJobs();
  } catch (err) {
    showToast(err.message, "error");
  }
});

// ── MCP tab ───────────────────────────────────────────────────────────────────

const mcpList = document.getElementById("mcp-list");

// ── MCP tool popover ──────────────────────────────────────────────────────────
const mcpPopover = (() => {
  const el = document.createElement("div");
  el.className = "mcp-popover";
  el.hidden = true;
  document.body.appendChild(el);

  function show(card, name, desc) {
    el.innerHTML = `<strong class="mcp-popover-name">${escapeHtml(name)}</strong><p class="mcp-popover-desc">${escapeHtml(desc)}</p>`;
    el.hidden = false;
    const r = card.getBoundingClientRect();
    const pw = el.offsetWidth, ph = el.offsetHeight;
    let top = r.bottom + 6;
    let left = r.left;
    if (left + pw > window.innerWidth - 8) left = window.innerWidth - pw - 8;
    if (top + ph > window.innerHeight - 8) top = r.top - ph - 6;
    el.style.top = `${top}px`;
    el.style.left = `${left}px`;
    card.classList.add("mcp-tool-card--active");
  }

  function hide() {
    el.hidden = true;
    document.querySelectorAll(".mcp-tool-card--active").forEach(c => c.classList.remove("mcp-tool-card--active"));
  }

  document.addEventListener("click", (e) => {
    if (!el.contains(e.target) && !e.target.closest(".mcp-tool-card")) hide();
  });
  document.addEventListener("keydown", (e) => { if (e.key === "Escape") hide(); });

  return { show, hide };
})();

function renderMcpList(servers) {
  if (!servers.length) {
    mcpList.innerHTML = `<div class="jobs-empty">${t("mcp_empty")}<br><span class="jobs-empty-hint">${t("mcp_empty_hint")}</span></div>`;
    return;
  }
  mcpList.innerHTML = servers.map((server) => {
    const toolsHtml = server.tools.map((tool) => {
      const shortName = tool.original_name || tool.name;
      return `
      <div class="mcp-tool-card" data-name="${escapeHtml(shortName)}" data-desc="${escapeHtml(tool.description)}">
        <span class="mcp-tool-card-name">${escapeHtml(shortName)}</span>
        <span class="mcp-tool-card-desc">${escapeHtml(tool.description)}</span>
      </div>`;
    }).join("");
    return `
<div class="mcp-server-card">
  <div class="mcp-server-header">
    <div class="mcp-server-info">
      <span class="mcp-status-dot"></span>
      <strong class="mcp-server-name">${escapeHtml(server.name)}</strong>
      <span class="mcp-server-count">${server.tool_count} ${t("mcp_tools")}</span>
    </div>
  </div>
  ${server.tools.length ? `<div class="mcp-tool-grid">${toolsHtml}</div>` : ""}
</div>`;
  }).join("");

  mcpList.querySelectorAll(".mcp-tool-card").forEach(card => {
    card.addEventListener("click", (e) => {
      e.stopPropagation();
      if (card.classList.contains("mcp-tool-card--active")) {
        mcpPopover.hide();
      } else {
        mcpPopover.show(card, card.dataset.name, card.dataset.desc);
      }
    });
  });
}

async function refreshMcp() {
  try {
    const servers = await fetchMcpServers();
    renderMcpList(servers);
  } catch (_) {
    mcpList.innerHTML = `<div class="jobs-empty">${t("mcp_empty")}</div>`;
  }
}

document.getElementById("mcp-refresh-btn").addEventListener("click", refreshMcp);
