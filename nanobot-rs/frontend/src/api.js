import { t } from "./i18n.js";

async function parseJson(response) {
  return response.json().catch(() => ({}));
}

export async function fetchCurrentUser() {
  const response = await fetch("/api/auth/me");
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || "Authentication required");
  }
  return payload;
}

export async function loginUser(username, password) {
  const response = await fetch("/api/auth/login", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ username, password }),
  });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || "Failed to sign in");
  }
  return payload;
}

export async function logoutUser() {
  const response = await fetch("/api/auth/logout", { method: "POST" });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || "Failed to sign out");
  }
  return payload;
}

export async function changePassword(currentPassword, newPassword) {
  const response = await fetch("/api/auth/change-password", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ currentPassword, newPassword }),
  });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || "Failed to update password");
  }
  return payload;
}

export async function fetchMyConfig() {
  const response = await fetch("/api/me/config");
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || "Failed to load config");
  }
  return payload;
}

export async function updateMyConfig(rawConfig) {
  const response = await fetch("/api/me/config", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: rawConfig,
  });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || "Failed to save config");
  }
  return payload;
}

export async function fetchAdminUsers() {
  const response = await fetch("/api/admin/users");
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || "Failed to load users");
  }
  return payload.users || [];
}

export async function createAdminUser(input) {
  const response = await fetch("/api/admin/users", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || "Failed to create user");
  }
  return payload.user;
}

export async function enableAdminUser(userId) {
  const response = await fetch(`/api/admin/users/${userId}/enable`, { method: "POST" });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || "Failed to enable user");
  }
  return payload.user;
}

export async function disableAdminUser(userId) {
  const response = await fetch(`/api/admin/users/${userId}/disable`, { method: "POST" });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || "Failed to disable user");
  }
  return payload.user;
}

export async function setAdminUserPassword(userId, password) {
  const response = await fetch(`/api/admin/users/${userId}/password`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ password }),
  });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || "Failed to reset password");
  }
  return payload.user;
}

export async function setAdminUserRole(userId, role) {
  const response = await fetch(`/api/admin/users/${userId}/role`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ role }),
  });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || "Failed to update role");
  }
  return payload.user;
}

export async function fetchSessions() {
  const response = await fetch("/api/sessions");
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || t("failed_load_sessions"));
  }
  return payload.groups || [];
}

export async function fetchSessionDetail(channel, sessionId) {
  const response = await fetch(`/api/sessions/${channel}/${sessionId}`);
  const detail = await parseJson(response);
  if (!response.ok) {
    throw new Error(detail.error || t("failed_load_session"));
  }
  detail.channel = detail.channel || channel;
  detail.sessionId = detail.sessionId || sessionId;
  return detail;
}

export async function createSession() {
  const response = await fetch("/api/sessions", { method: "POST" });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || t("failed_create_session"));
  }
  return payload;
}

export async function duplicateSession(channel, sessionId) {
  const response = await fetch("/api/sessions/duplicate", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ channel, sessionId }),
  });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || t("failed_duplicate_session"));
  }
  return payload;
}

export async function deleteSession(channel, sessionId) {
  const response = await fetch(`/api/sessions/${channel}/${sessionId}`, {
    method: "DELETE",
  });
  if (!response.ok && response.status !== 404) {
    const payload = await response.json().catch(() => ({}));
    throw new Error(payload.error || t("failed_delete_session"));
  }
}

export async function setSessionProfile(channel, sessionId, profile) {
  await fetch(`/api/sessions/${channel}/${sessionId}/profile`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ profile }),
  });
}

export async function sendChat(message, channel, sessionId) {
  const response = await fetch("/api/chat", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ message, channel, sessionId }),
  });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || t("request_failed"));
  }
  return payload;
}

export async function fetchWeixinAccount() {
  const response = await fetch("/api/weixin/account");
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || t("failed_load_weixin_account"));
  }
  return payload;
}

export async function startWeixinLogin() {
  const response = await fetch("/api/weixin/login/start", { method: "POST" });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || t("failed_start_weixin"));
  }
  return payload;
}

export async function fetchWeixinLoginStatus() {
  const response = await fetch("/api/weixin/login/status");
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || t("failed_poll_weixin"));
  }
  return payload;
}

export async function logoutWeixin() {
  const response = await fetch("/api/weixin/logout", { method: "POST" });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || t("failed_logout_weixin"));
  }
  return payload;
}

export async function fetchCronJobs() {
  const response = await fetch("/api/cron/jobs");
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || t("jobs_load_failed"));
  }
  return payload.jobs || [];
}

export async function addCronJob(params) {
  const response = await fetch("/api/cron/jobs", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(params),
  });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || t("jobs_add_failed"));
  }
  return payload;
}

export async function deleteCronJob(id) {
  const response = await fetch(`/api/cron/jobs/${id}`, { method: "DELETE" });
  if (!response.ok) {
    const payload = await response.json().catch(() => ({}));
    throw new Error(payload.error || t("jobs_delete_failed"));
  }
}

export async function toggleCronJob(id) {
  const response = await fetch(`/api/cron/jobs/${id}/toggle`, { method: "POST" });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || t("jobs_toggle_failed"));
  }
  return payload;
}

export async function runCronJob(id) {
  const response = await fetch(`/api/cron/jobs/${id}/run`, { method: "POST" });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || t("jobs_run_failed"));
  }
  return payload;
}

export async function fetchMcpServers() {
  const response = await fetch("/api/mcp/servers");
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || "Failed to load MCP servers");
  }
  return payload.servers || [];
}

export async function toggleMcpTool(name, enabled) {
  const response = await fetch(`/api/mcp/tools/${encodeURIComponent(name)}/toggle`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ enabled }),
  });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || "Failed to toggle tool");
  }
  return payload;
}

export async function applyMcpServerAction(name, action) {
  const response = await fetch(`/api/mcp/servers/${encodeURIComponent(name)}/tools/bulk`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ action }),
  });
  const payload = await parseJson(response);
  if (!response.ok) {
    throw new Error(payload.error || t("mcp_action_failed"));
  }
  return payload;
}

export async function loadProfiles() {
  try {
    const response = await fetch("/api/profiles");
    const payload = await parseJson(response);
    if (!response.ok) return [];
    return payload.profiles || [];
  } catch (_) {
    return [];
  }
}
