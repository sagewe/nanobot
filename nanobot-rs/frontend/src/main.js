import { t, getLang, setLang, applyI18n } from "./i18n.js";
import {
  fetchCurrentUser,
  loginUser,
  logoutUser,
  changePassword,
  fetchMyConfig,
  updateMyConfig,
  fetchAdminUsers,
  createAdminUser,
  enableAdminUser,
  disableAdminUser,
  setAdminUserPassword,
  setAdminUserRole,
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
  toggleMcpTool,
  applyMcpServerAction,
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
  setMcpServerIcons,
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
const loginShell = document.getElementById("login-shell");
const loginForm = document.getElementById("login-form");
const loginUsernameInput = document.getElementById("login-username");
const loginPasswordInput = document.getElementById("login-password");
const loginError = document.getElementById("login-error");
const composer = document.getElementById("composer");
const sessionSelect = document.getElementById("session-select");
const profileSelect = document.getElementById("profile-select");
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
const settingsPane = document.querySelector(".settings-pane");
const usersPane = document.querySelector(".users-pane");
const sessionsSearch = document.getElementById("sessions-search");
const sessionRail = document.querySelector(".session-rail");
const transcript = document.getElementById("transcript");
const slashMenu = document.getElementById("slash-menu");
const slashMenuList = document.getElementById("slash-menu-list");
const currentUserDisplay = document.getElementById("current-user-display");
const currentUserRole = document.getElementById("current-user-role");
const logoutButton = document.getElementById("logout-button");
const adminUsersTab = document.getElementById("admin-users-tab");
const settingsRefreshButton = document.getElementById("settings-refresh-button");
const settingsForm = document.getElementById("settings-form");
const settingsDefaultProfile = document.getElementById("settings-default-profile");
const settingsTelegramEnabled = document.getElementById("settings-telegram-enabled");
const settingsTelegramToken = document.getElementById("settings-telegram-token");
const settingsWeixinEnabled = document.getElementById("settings-weixin-enabled");
const settingsWeixinApiBase = document.getElementById("settings-weixin-api-base");
const settingsWecomEnabled = document.getElementById("settings-wecom-enabled");
const settingsWecomBotId = document.getElementById("settings-wecom-bot-id");
const settingsWecomSecret = document.getElementById("settings-wecom-secret");
const configEditor = document.getElementById("config-editor");
const changePasswordForm = document.getElementById("change-password-form");
const currentPasswordInput = document.getElementById("current-password-input");
const newPasswordInput = document.getElementById("new-password-input");
const usersRefreshButton = document.getElementById("users-refresh-button");
const createUserForm = document.getElementById("create-user-form");
const createUsernameInput = document.getElementById("create-username");
const createDisplayNameInput = document.getElementById("create-display-name");
const createPasswordInput = document.getElementById("create-password");
const createRoleInput = document.getElementById("create-role");
const adminUsersList = document.getElementById("admin-users-list");

const legacyStoredSessionId = localStorage.getItem(SESSION_KEY);

// ── App state ─────────────────────────────────────────────────────────────────
let currentChannel = null;
let currentSessionId = null;
let currentSessionReadOnly = false;
let currentSessionCanDuplicate = false;
let currentSessionGroups = [];
let currentMessages = [];
let currentUser = null;
let currentConfig = null;
let appBootstrapped = false;
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

function setAuthState(user) {
  currentUser = user;
  loginShell.hidden = Boolean(user);
  document.getElementById("app").hidden = !user;
  currentUserDisplay.textContent = user?.displayName || user?.username || "Guest";
  currentUserRole.textContent = user?.role || "guest";
  adminUsersTab.hidden = !user || user.role !== "admin";
  if ((!user || user.role !== "admin") && usersPane && !usersPane.hidden) {
    switchTab("chat");
  }
}

function stableStringifyConfig(config) {
  return JSON.stringify(config, null, 2);
}

function syncStructuredSettings(config) {
  settingsDefaultProfile.value = config?.agents?.defaults?.defaultProfile || "";
  settingsTelegramEnabled.checked = Boolean(config?.channels?.telegram?.enabled);
  settingsTelegramToken.value = config?.channels?.telegram?.token || "";
  settingsWeixinEnabled.checked = Boolean(config?.channels?.weixin?.enabled);
  settingsWeixinApiBase.value = config?.channels?.weixin?.apiBase || "";
  settingsWecomEnabled.checked = Boolean(config?.channels?.wecom?.enabled);
  settingsWecomBotId.value = config?.channels?.wecom?.botId || "";
  settingsWecomSecret.value = config?.channels?.wecom?.secret || "";
}

function applyStructuredSettings(config) {
  const next = structuredClone(config || {});
  next.agents ??= {};
  next.agents.defaults ??= {};
  next.channels ??= {};
  next.channels.telegram ??= {};
  next.channels.weixin ??= {};
  next.channels.wecom ??= {};

  if (settingsDefaultProfile.value.trim()) {
    next.agents.defaults.defaultProfile = settingsDefaultProfile.value.trim();
  }
  next.channels.telegram.enabled = settingsTelegramEnabled.checked;
  next.channels.telegram.token = settingsTelegramToken.value.trim();
  next.channels.weixin.enabled = settingsWeixinEnabled.checked;
  next.channels.weixin.apiBase = settingsWeixinApiBase.value.trim();
  next.channels.wecom.enabled = settingsWecomEnabled.checked;
  next.channels.wecom.botId = settingsWecomBotId.value.trim();
  next.channels.wecom.secret = settingsWecomSecret.value.trim();
  return next;
}

function tUserRole(role) {
  return role === "admin" ? t("users_role_admin") : t("users_role_user");
}

function tUserState(enabled) {
  return enabled ? t("users_state_enabled") : t("users_state_disabled");
}

function tUserRuntimeStatus(status) {
  const normalized = (status || "unknown").toLowerCase();
  const key = `users_runtime_${normalized}`;
  const translated = t(key);
  return translated === key ? (status || t("users_runtime_unknown")) : translated;
}

function tResetPasswordPrompt(username) {
  return t("users_prompt_password").replace("{username}", username || "");
}

function renderAdminUsers(users) {
  if (!users.length) {
    adminUsersList.innerHTML = `<div class="jobs-empty">${t("users_empty")}</div>`;
    return;
  }
  adminUsersList.innerHTML = users
    .map(
      (user) => `
        <article class="admin-user-card" data-user-id="${user.userId}">
          <div class="admin-user-head">
            <div class="admin-user-name">
              <strong>${user.displayName || user.username}</strong>
              <span>${user.username}</span>
            </div>
            <div class="admin-user-badges">
              <span class="admin-user-badge">${tUserRole(user.role)}</span>
              <span class="admin-user-badge" data-state="${user.enabled ? "enabled" : "disabled"}">${tUserState(user.enabled)}</span>
              <span class="admin-user-badge">${tUserRuntimeStatus(user.runtimeStatus)}</span>
            </div>
          </div>
          <div class="admin-user-actions">
            <button type="button" data-action="${user.enabled ? "disable" : "enable"}" data-user-id="${user.userId}">
              ${user.enabled ? t("users_action_disable") : t("users_action_enable")}
            </button>
            <button type="button" data-action="role" data-user-id="${user.userId}" data-role="${user.role}">
              ${user.role === "admin" ? t("users_action_make_user") : t("users_action_make_admin")}
            </button>
            <button type="button" data-action="password" data-user-id="${user.userId}" data-username="${user.username}">
              ${t("users_action_reset_password")}
            </button>
          </div>
        </article>
      `
    )
    .join("");
}

async function loadSettings() {
  currentConfig = await fetchMyConfig();
  syncStructuredSettings(currentConfig);
  configEditor.value = stableStringifyConfig(currentConfig);
}

async function refreshAdminUsers() {
  if (!currentUser || currentUser.role !== "admin") return;
  const users = await fetchAdminUsers();
  renderAdminUsers(users);
}

async function initializeAuthenticatedApp() {
  renderTranscript([]);
  setComposerAccess(false, false);
  setStatus("", "idle");
  clearWeixinPollTimer();

  fetchMcpServers()
    .then((servers) =>
      setMcpServerIcons(
        Object.fromEntries(servers.filter((server) => server.icon).map((server) => [server.name, server.icon]))
      )
    )
    .catch(() => {});

  await Promise.all([
    bootstrapSessions(),
    loadWeixinAccount(),
    loadProfiles().then(renderProfiles),
    loadSettings(),
    currentUser?.role === "admin" ? refreshAdminUsers() : Promise.resolve(),
  ]);
  appBootstrapped = true;
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
  const groups = sessions;
  currentSessionGroups = groups;
  renderSessionSelect(currentSessionGroups, currentChannel, currentSessionId);
  syncSessionsList();
  const storedSession = findSession(groups, storedChannel || "web", restoredSessionId);
  const initialSession = storedSession || findLatestWritableWebSession(groups);
  if (!initialSession) {
    setSelectedSession(null, null);
    const created = await createSession();
    await refreshSessions();
    await selectSession(created.channel || "web", created.sessionId);
    return;
  }
  await selectSession(initialSession.channel, initialSession.sessionId);
}

async function loadWeixinAccount() {
  const account = await fetchWeixinAccount();
  renderWeixinAccount(account);
  return account;
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
      await loadWeixinAccount();
      await refreshSessions();
      return;
    }

    if (payload.expired === true || payload.status === "expired") {
      weixinStatusLabel.textContent = t("login_expired");
      weixinUserLabel.textContent = t("refresh_qr");
      clearWeixinPollTimer();
      await loadWeixinAccount();
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
    loadWeixinAccount().catch(() => {});
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
  settingsPane.hidden = tab !== "settings";
  usersPane.hidden = tab !== "users";
  if (tab === "jobs") refreshJobs();
  if (tab === "mcp") refreshMcp();
  if (tab === "settings") loadSettings().catch((error) => setStatus(error?.message || t("settings_load_failed"), "error"));
  if (tab === "users" && currentUser?.role === "admin") {
    refreshAdminUsers().catch((error) => setStatus(error?.message || t("users_load_failed"), "error"));
  }
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
  if (currentUser?.role === "admin") {
    refreshAdminUsers().catch(() => {});
  }
});

loginForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  loginError.textContent = "";
  try {
    const user = await loginUser(loginUsernameInput.value.trim(), loginPasswordInput.value);
    setAuthState(user);
    try {
      await initializeAuthenticatedApp();
    } catch (error) {
      setStatus(error?.message || t("failed_load_sessions"), "error");
    }
    loginPasswordInput.value = "";
  } catch (error) {
    loginError.textContent = error?.message || "Failed to sign in";
  }
});

logoutButton.addEventListener("click", async () => {
  try {
    await logoutUser();
  } catch (_) {
    // Ignore logout transport failures and force a local reset.
  }
  window.location.reload();
});

settingsRefreshButton.addEventListener("click", async () => {
  try {
    await loadSettings();
    setStatus(t("settings_reloaded"), "idle");
  } catch (error) {
    setStatus(error?.message || t("settings_reload_failed"), "error");
  }
});

settingsForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  try {
    const parsed = JSON.parse(configEditor.value || "{}");
    const nextConfig = applyStructuredSettings(parsed);
    await updateMyConfig(stableStringifyConfig(nextConfig));
    currentConfig = nextConfig;
    syncStructuredSettings(nextConfig);
    configEditor.value = stableStringifyConfig(nextConfig);
    await loadProfiles().then(renderProfiles);
    await refreshSessions();
    setStatus(t("settings_saved"), "idle");
  } catch (error) {
    setStatus(error?.message || t("settings_save_failed"), "error");
  }
});

changePasswordForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  try {
    await changePassword(currentPasswordInput.value, newPasswordInput.value);
    currentPasswordInput.value = "";
    newPasswordInput.value = "";
    setStatus(t("settings_password_updated"), "idle");
  } catch (error) {
    setStatus(error?.message || t("settings_password_update_failed"), "error");
  }
});

usersRefreshButton.addEventListener("click", async () => {
  try {
    await refreshAdminUsers();
    setStatus(t("users_refreshed"), "idle");
  } catch (error) {
    setStatus(error?.message || t("users_load_failed"), "error");
  }
});

createUserForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  try {
    await createAdminUser({
      username: createUsernameInput.value.trim(),
      displayName: createDisplayNameInput.value.trim(),
      password: createPasswordInput.value,
      role: createRoleInput.value,
    });
    createUserForm.reset();
    await refreshAdminUsers();
    setStatus(t("users_created"), "idle");
  } catch (error) {
    setStatus(error?.message || t("users_create_failed"), "error");
  }
});

adminUsersList.addEventListener("click", async (event) => {
  const button = event.target.closest("button[data-action]");
  if (!button) return;
  const userId = button.dataset.userId;
  if (!userId) return;

  try {
    if (button.dataset.action === "enable") {
      await enableAdminUser(userId);
    } else if (button.dataset.action === "disable") {
      await disableAdminUser(userId);
    } else if (button.dataset.action === "role") {
      const nextRole = button.dataset.role === "admin" ? "user" : "admin";
      await setAdminUserRole(userId, nextRole);
    } else if (button.dataset.action === "password") {
      const nextPassword = window.prompt(tResetPasswordPrompt(button.dataset.username));
      if (!nextPassword) return;
      await setAdminUserPassword(userId, nextPassword);
    }
    await refreshAdminUsers();
    setStatus(t("users_updated"), "idle");
  } catch (error) {
    setStatus(error?.message || t("users_update_failed"), "error");
  }
});

profileSelect.addEventListener("change", async () => {
  const profile = profileSelect.value;
  setCurrentProfile(profile);
  if (!profile || !currentChannel || !currentSessionId) return;
  try {
    await setSessionProfile(currentChannel, currentSessionId, profile);
  } catch (_) {}
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
    // const duplicated = await duplicateSession();
    // body: JSON.stringify({ channel: currentChannel, sessionId: currentSessionId })
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
    loadWeixinAccount().catch(() => {});
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
    await loadWeixinAccount();
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
setAuthState(null);

(async () => {
  try {
    const user = await fetchCurrentUser();
    setAuthState(user);
    try {
      await initializeAuthenticatedApp();
    } catch (error) {
      clearWeixinPollTimer();
      setStatus(error?.message || t("failed_load_sessions"), "error");
    }
  } catch (_) {
    setAuthState(null);
    loginUsernameInput.focus();
  }
})();

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

function formatMcpToolCount(enabledCount, totalCount) {
  return enabledCount === totalCount
    ? `${totalCount} ${t("mcp_tools")}`
    : `${enabledCount}/${totalCount} ${t("mcp_tools")}`;
}

function setMcpCardEnabled(card, enabled) {
  card.dataset.enabled = enabled ? "true" : "false";
  card.classList.toggle("mcp-tool-card--disabled", !enabled);
}

function updateMcpServerCount(serverCard) {
  if (!serverCard) return;
  const cards = [...serverCard.querySelectorAll(".mcp-tool-card")];
  const enabledCount = cards.filter((card) => card.dataset.enabled !== "false").length;
  const countNode = serverCard.querySelector(".mcp-server-count");
  if (countNode) {
    countNode.textContent = formatMcpToolCount(enabledCount, cards.length);
  }
}

async function runMcpServerAction(serverName, action) {
  mcpPopover.hide();
  try {
    await applyMcpServerAction(serverName, action);
    await refreshMcp();
  } catch (error) {
    setStatus(error?.message || t("mcp_action_failed"), "error");
  }
}

// ── MCP tool popover ──────────────────────────────────────────────────────────
const mcpPopover = (() => {
  const el = document.createElement("div");
  el.className = "mcp-popover";
  el.hidden = true;
  document.body.appendChild(el);

  function show(card, fullName, shortName, desc, enabled) {
    document.querySelectorAll(".mcp-tool-card--active").forEach(c => c.classList.remove("mcp-tool-card--active"));
    el.innerHTML = `
      <div class="mcp-popover-header">
        <strong class="mcp-popover-name">${escapeHtml(shortName)}</strong>
        <label class="mcp-toggle" title="${enabled ? t("mcp_disable_tool") : t("mcp_enable_tool")}">
          <input type="checkbox" class="mcp-toggle-input" ${enabled ? "checked" : ""}>
          <span class="mcp-toggle-track"></span>
        </label>
      </div>
      <p class="mcp-popover-desc">${escapeHtml(desc)}</p>`;
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

    el.querySelector(".mcp-toggle-input").addEventListener("change", async (e) => {
      e.stopPropagation();
      const previousEnabled = card.dataset.enabled !== "false";
      const nowEnabled = e.target.checked;
      const serverCard = card.closest(".mcp-server-card");
      e.target.disabled = true;
      setMcpCardEnabled(card, nowEnabled);
      updateMcpServerCount(serverCard);
      el.querySelector(".mcp-toggle").title = nowEnabled ? t("mcp_disable_tool") : t("mcp_enable_tool");
      try {
        await toggleMcpTool(fullName, nowEnabled);
      } catch (error) {
        e.target.checked = previousEnabled;
        setMcpCardEnabled(card, previousEnabled);
        updateMcpServerCount(serverCard);
        el.querySelector(".mcp-toggle").title = previousEnabled ? t("mcp_disable_tool") : t("mcp_enable_tool");
        setStatus(error?.message || t("mcp_toggle_failed"), "error");
      } finally {
        e.target.disabled = false;
      }
    });
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

function mcpToolIcon(name) {
  const n = name.toLowerCase();
  const svg = (p) => `<svg class="mcp-tool-icon" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">${p}</svg>`;
  if (/turnon|turn_on/.test(n))             return svg(`<polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>`);
  if (/turnoff|turn_off/.test(n))           return svg(`<path d="M18.36 6.64A9 9 0 0 1 20.77 15"/><path d="M6.16 6.16a9 9 0 1 0 12.68 12.68"/><line x1="12" y1="2" x2="12" y2="12"/><line x1="2" y1="2" x2="22" y2="22"/>`);
  if (/light/.test(n))                      return svg(`<line x1="9" y1="18" x2="15" y2="18"/><line x1="10" y1="22" x2="14" y2="22"/><path d="M15.09 14c.18-.98.65-1.74 1.41-2.5A4.65 4.65 0 0 0 18 8 6 6 0 0 0 6 8c0 1 .23 2.23 1.5 3.5A4.61 4.61 0 0 1 8.91 14"/>`);
  if (/broadcast/.test(n))                  return svg(`<path d="M4.9 19.1C1 15.2 1 8.8 4.9 4.9"/><path d="M7.8 16.2c-2.3-2.3-2.3-6.1 0-8.5"/><circle cx="12" cy="12" r="2"/><path d="M16.2 7.8c2.3 2.3 2.3 6.1 0 8.5"/><path d="M19.1 4.9C23 8.8 23 15.2 19.1 19.1"/>`);
  if (/fan/.test(n))                        return svg(`<path d="M9.59 4.59A2 2 0 1 1 11 8H2m10.59 11.41A2 2 0 1 0 14 16H2m15.73-8.27A2 2 0 1 1 19.41 10H2"/>`);
  if (/unmute/.test(n))                     return svg(`<polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/>`);
  if (/mute/.test(n))                       return svg(`<polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><line x1="23" y1="9" x2="17" y2="15"/><line x1="17" y1="9" x2="23" y2="15"/>`);
  if (/volumerelative|volume_relative/.test(n)) return svg(`<polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><path d="M15.54 8.46a5 5 0 0 1 0 7.07"/>`);
  if (/volume/.test(n))                     return svg(`<polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/><path d="M19.07 4.93a10 10 0 0 1 0 14.14M15.54 8.46a5 5 0 0 1 0 7.07"/>`);
  if (/unpause|resume/.test(n))             return svg(`<polygon points="5 3 19 12 5 21 5 3"/>`);
  if (/pause/.test(n))                      return svg(`<rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/>`);
  if (/next/.test(n))                       return svg(`<polygon points="5 4 15 12 5 20 5 4"/><line x1="19" y1="5" x2="19" y2="19"/>`);
  if (/previous|prev/.test(n))              return svg(`<polygon points="19 20 9 12 19 4 19 20"/><line x1="5" y1="19" x2="5" y2="5"/>`);
  if (/search/.test(n))                     return svg(`<circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/>`);
  if (/timer|cancel/.test(n))               return svg(`<circle cx="12" cy="12" r="10"/><line x1="15" y1="9" x2="9" y2="15"/><line x1="9" y1="9" x2="15" y2="15"/>`);
  if (/datetime|time|date/.test(n))         return svg(`<circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/>`);
  if (/context|live|state/.test(n))         return svg(`<polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/>`);
  return svg(`<path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/>`);
}

function renderMcpList(servers) {
  setMcpServerIcons(Object.fromEntries(servers.filter(s => s.icon).map(s => [s.name, s.icon])));
  if (!servers.length) {
    mcpList.innerHTML = `<div class="jobs-empty">${t("mcp_empty")}<br><span class="jobs-empty-hint">${t("mcp_empty_hint")}</span></div>`;
    return;
  }
  mcpList.innerHTML = servers.map((server) => {
    const toolsHtml = server.tools.map((tool) => {
      const shortName = tool.original_name || tool.name;
      const enabled = tool.enabled !== false;
      return `
      <div class="mcp-tool-card${enabled ? "" : " mcp-tool-card--disabled"}"
           data-name="${escapeHtml(tool.name)}"
           data-short="${escapeHtml(shortName)}"
           data-desc="${escapeHtml(tool.description)}"
           data-enabled="${enabled}">
        <div class="mcp-tool-card-header">
          ${mcpToolIcon(shortName)}
          <span class="mcp-tool-card-name">${escapeHtml(shortName)}</span>
        </div>
        <span class="mcp-tool-card-desc">${escapeHtml(tool.description)}</span>
      </div>`;
    }).join("");
    const enabledCount = server.tools.filter(t => t.enabled !== false).length;
    const iconHtml = server.icon
      ? (server.icon.startsWith("http") || server.icon.startsWith("/") || server.icon.startsWith("data:")
          ? `<img class="mcp-server-icon" src="${escapeHtml(server.icon)}" alt="${escapeHtml(server.name)}">`
          : `<span class="mcp-server-icon-emoji">${escapeHtml(server.icon)}</span>`)
      : "";
    return `
<div class="mcp-server-card">
  <div class="mcp-server-header">
    <div class="mcp-server-info">
      <span class="mcp-status-dot"></span>
      ${iconHtml}<strong class="mcp-server-name">${escapeHtml(server.name)}</strong>
      <span class="mcp-server-count">${formatMcpToolCount(enabledCount, server.tool_count)}</span>
    </div>
    <div class="mcp-server-actions">
      <button type="button" class="mcp-server-action" data-server-action="enableAll" data-server-name="${escapeHtml(server.name)}">${t("mcp_enable_all")}</button>
      <button type="button" class="mcp-server-action" data-server-action="disableAll" data-server-name="${escapeHtml(server.name)}">${t("mcp_disable_all")}</button>
      <button type="button" class="mcp-server-action" data-server-action="reset" data-server-name="${escapeHtml(server.name)}">${t("mcp_reset")}</button>
    </div>
  </div>
  ${server.tools.length ? `<div class="mcp-tool-grid">${toolsHtml}</div>` : ""}
</div>`;
  }).join("");

  mcpList.querySelectorAll(".mcp-server-action").forEach(button => {
    button.addEventListener("click", async (e) => {
      e.stopPropagation();
      const serverName = button.dataset.serverName;
      const action = button.dataset.serverAction;
      button.disabled = true;
      try {
        await runMcpServerAction(serverName, action);
      } finally {
        button.disabled = false;
      }
    });
  });

  mcpList.querySelectorAll(".mcp-tool-card").forEach(card => {
    card.addEventListener("click", (e) => {
      e.stopPropagation();
      if (card.classList.contains("mcp-tool-card--active")) {
        mcpPopover.hide();
      } else {
        const enabled = card.dataset.enabled !== "false";
        mcpPopover.show(card, card.dataset.name, card.dataset.short, card.dataset.desc, enabled);
      }
    });
  });
}

async function refreshMcp() {
  try {
    const servers = await fetchMcpServers();
    setMcpServerIcons(Object.fromEntries(servers.filter(s => s.icon).map(s => [s.name, s.icon])));
    renderMcpList(servers);
  } catch (_) {
    mcpList.innerHTML = `<div class="jobs-empty">${t("mcp_empty")}</div>`;
  }
}

document.getElementById("mcp-refresh-btn").addEventListener("click", refreshMcp);
