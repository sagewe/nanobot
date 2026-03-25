import { t } from "./i18n.js";

export async function fetchSessions() {
  const response = await fetch("/api/sessions");
  const payload = await response.json();
  if (!response.ok) {
    throw new Error(payload.error || t("failed_load_sessions"));
  }
  return payload.groups || [];
}

export async function fetchSessionDetail(channel, sessionId) {
  const response = await fetch(`/api/sessions/${channel}/${sessionId}`);
  const detail = await response.json();
  if (!response.ok) {
    throw new Error(detail.error || t("failed_load_session"));
  }
  detail.channel = detail.channel || channel;
  detail.sessionId = detail.sessionId || sessionId;
  return detail;
}

export async function createSession() {
  const response = await fetch("/api/sessions", { method: "POST" });
  const payload = await response.json();
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
  const payload = await response.json();
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
  const payload = await response.json();
  if (!response.ok) {
    throw new Error(payload.error || t("request_failed"));
  }
  return payload;
}

export async function fetchWeixinAccount() {
  const response = await fetch("/api/weixin/account");
  const payload = await response.json();
  if (!response.ok) {
    throw new Error(payload.error || t("failed_load_weixin_account"));
  }
  return payload;
}

export async function startWeixinLogin() {
  const response = await fetch("/api/weixin/login/start", { method: "POST" });
  const payload = await response.json();
  if (!response.ok) {
    throw new Error(payload.error || t("failed_start_weixin"));
  }
  return payload;
}

export async function fetchWeixinLoginStatus() {
  const response = await fetch("/api/weixin/login/status");
  const payload = await response.json();
  if (!response.ok) {
    throw new Error(payload.error || t("failed_poll_weixin"));
  }
  return payload;
}

export async function logoutWeixin() {
  const response = await fetch("/api/weixin/logout", { method: "POST" });
  const payload = await response.json();
  if (!response.ok) {
    throw new Error(payload.error || t("failed_logout_weixin"));
  }
  return payload;
}

export async function loadProfiles() {
  try {
    const response = await fetch("/api/profiles");
    const payload = await response.json();
    if (!response.ok) return [];
    return payload.profiles || [];
  } catch (_) {
    return [];
  }
}
