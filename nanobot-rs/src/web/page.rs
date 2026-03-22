use axum::response::Html;

pub fn render_index_html() -> String {
    r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>nanobot-rs control room</title>
    <style>
      :root {
        --paper: #f6efe2;
        --ink: #21313d;
        --muted: #6d6a63;
        --accent: #c9622f;
        --panel: rgba(255, 251, 245, 0.82);
        --line: rgba(33, 49, 61, 0.14);
        --shadow: 0 22px 60px rgba(75, 50, 28, 0.15);
      }

      * {
        box-sizing: border-box;
      }

      body {
        margin: 0;
        min-height: 100vh;
        color: var(--ink);
        background:
          radial-gradient(circle at top left, rgba(201, 98, 47, 0.16), transparent 28rem),
          linear-gradient(180deg, #f8f2e7 0%, #efe4d1 100%);
        font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
      }

      body::before {
        content: "";
        position: fixed;
        inset: 0;
        pointer-events: none;
        background-image:
          linear-gradient(rgba(33, 49, 61, 0.03) 1px, transparent 1px),
          linear-gradient(90deg, rgba(33, 49, 61, 0.03) 1px, transparent 1px);
        background-size: 2rem 2rem;
        mask-image: linear-gradient(180deg, rgba(0, 0, 0, 0.32), transparent 85%);
      }

      #app {
        width: min(92vw, 72rem);
        margin: 0 auto;
        padding: 3rem 0 4rem;
      }

      .hero {
        display: grid;
        gap: 0.75rem;
        margin-bottom: 1.75rem;
      }

      .eyebrow {
        color: var(--accent);
        text-transform: uppercase;
        letter-spacing: 0.18em;
        font-size: 0.78rem;
      }

      h1 {
        margin: 0;
        font-size: clamp(2.5rem, 5vw, 4.8rem);
        line-height: 0.92;
        font-weight: 700;
      }

      .deck {
        width: min(44rem, 100%);
        margin: 0;
        color: var(--muted);
        font-size: 1.05rem;
        line-height: 1.6;
      }

      .shell {
        display: grid;
        gap: 1rem;
        grid-template-columns: minmax(14rem, 18rem) minmax(0, 1fr);
        padding: 1rem;
        border: 1px solid var(--line);
        border-radius: 1.5rem;
        background: var(--panel);
        backdrop-filter: blur(12px);
        box-shadow: var(--shadow);
      }

      .session-rail {
        display: grid;
        gap: 0.85rem;
        align-content: start;
      }

      .session-header {
        display: grid;
        gap: 0.35rem;
      }

      .session-kicker {
        color: var(--accent);
        text-transform: uppercase;
        letter-spacing: 0.14em;
        font-size: 0.72rem;
      }

      #active-profile {
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.86rem;
      }

      #session-list {
        display: grid;
        gap: 0.85rem;
      }

      .session-group {
        display: grid;
        gap: 0.65rem;
      }

      .session-group-title {
        color: var(--accent);
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.78rem;
        letter-spacing: 0.08em;
        text-transform: uppercase;
      }

      .session-item {
        width: 100%;
        display: grid;
        gap: 0.3rem;
        text-align: left;
        border: 1px solid var(--line);
        border-radius: 1rem;
        padding: 0.8rem 0.9rem;
        background: rgba(255, 255, 255, 0.58);
        color: var(--ink);
        cursor: pointer;
      }

      .session-item[data-selected="true"] {
        border-color: rgba(201, 98, 47, 0.45);
        box-shadow: inset 0 0 0 1px rgba(201, 98, 47, 0.24);
      }

      .session-item-title {
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.84rem;
      }

      .session-item-preview {
        color: var(--muted);
        font-size: 0.9rem;
        line-height: 1.4;
      }

      .session-item-meta {
        color: var(--accent);
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.78rem;
      }

      .account-panel {
        display: grid;
        gap: 0.7rem;
        padding: 0.95rem;
        border: 1px solid var(--line);
        border-radius: 1rem;
        background: rgba(255, 255, 255, 0.58);
      }

      .account-status {
        display: grid;
        gap: 0.3rem;
      }

      .account-status strong,
      .account-status span {
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.84rem;
      }

      .account-muted {
        color: var(--muted);
        font-size: 0.9rem;
        line-height: 1.4;
      }

      .account-actions {
        display: flex;
        flex-wrap: wrap;
        gap: 0.6rem;
      }

      #weixin-qr-panel {
        display: grid;
        gap: 0.5rem;
        padding: 0.75rem;
        border-radius: 0.9rem;
        border: 1px dashed var(--line);
        background: rgba(255, 255, 255, 0.52);
      }

      #weixin-qr-image {
        width: min(100%, 12rem);
        border-radius: 0.8rem;
        background: white;
      }

      .conversation-pane {
        display: grid;
        gap: 1rem;
      }

      #transcript {
        min-height: 22rem;
        padding: 1rem;
        border: 1px dashed var(--line);
        border-radius: 1rem;
        background: rgba(255, 255, 255, 0.48);
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        display: grid;
        gap: 0.75rem;
        align-content: start;
      }

      .message {
        max-width: min(46rem, 100%);
        padding: 0.9rem 1rem;
        border-radius: 1rem;
        white-space: pre-wrap;
        line-height: 1.55;
      }

      .message[data-role="user"] {
        justify-self: end;
        color: #fff8ef;
        background: linear-gradient(135deg, #3a5263, #2a3945);
      }

      .message[data-role="assistant"] {
        justify-self: start;
        background: rgba(255, 255, 255, 0.72);
        border: 1px solid rgba(33, 49, 61, 0.1);
      }

      #status {
        min-height: 1.4rem;
        color: var(--muted);
        font-size: 0.95rem;
      }

      #status[data-variant="error"] {
        color: #9a2f1f;
      }

      #composer {
        display: grid;
        gap: 0.8rem;
      }

      .composer-actions {
        display: flex;
        flex-wrap: wrap;
        gap: 0.75rem;
      }

      #message-input {
        min-height: 8rem;
        resize: vertical;
        border: 1px solid var(--line);
        border-radius: 1rem;
        padding: 1rem;
        font: inherit;
        color: var(--ink);
        background: rgba(255, 255, 255, 0.72);
      }

      #send-button,
      #new-chat-button,
      #duplicate-session-button {
        border-radius: 999px;
        padding: 0.85rem 1.15rem;
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.92rem;
        cursor: pointer;
      }

      #send-button {
        justify-self: start;
        border: 0;
        padding-inline: 1.35rem;
        color: #fff8ef;
        background: linear-gradient(135deg, #c9622f, #a4461f);
      }

      #new-chat-button,
      #duplicate-session-button {
        border: 1px solid var(--line);
        color: var(--ink);
        background: rgba(255, 255, 255, 0.72);
      }

      #send-button[disabled],
      #new-chat-button[disabled],
      #duplicate-session-button[disabled],
      #message-input[disabled] {
        opacity: 0.65;
      }

      @media (max-width: 720px) {
        #app {
          width: min(94vw, 40rem);
          padding-top: 2rem;
        }

        .shell {
          grid-template-columns: 1fr;
          padding: 0.85rem;
          border-radius: 1.1rem;
        }
      }
    </style>
  </head>
  <body>
    <main id="app">
      <header class="hero">
        <div class="eyebrow">Local Operator Console</div>
        <h1>nanobot-rs control room</h1>
        <p class="deck">A minimal browser surface for the Rust agent. Text in, text out, same workspace brain underneath.</p>
      </header>
      <section class="shell">
        <aside class="session-rail">
          <div class="session-header">
            <div class="session-kicker">Sessions</div>
            <strong id="active-profile">default</strong>
          </div>
          <div id="session-list" aria-live="polite"><section class="session-group" hidden></section></div>
          <section id="weixin-account-panel" class="account-panel">
            <div class="session-kicker">Weixin</div>
            <div class="account-status">
              <strong id="weixin-status-label">Checking account…</strong>
              <span id="weixin-user-label" class="account-muted">Login from the embedded console.</span>
            </div>
            <div id="weixin-qr-panel" hidden>
              <img id="weixin-qr-image" alt="Weixin login QR code" />
              <div id="weixin-qr-note" class="account-muted">Scan this QR code in Weixin to confirm login.</div>
            </div>
            <div class="account-actions">
              <button id="weixin-login-button" type="button">Login to Weixin</button>
              <button id="weixin-logout-button" type="button">Logout</button>
            </div>
          </section>
        </aside>
        <section class="conversation-pane">
          <section id="transcript" aria-live="polite">
          </section>
          <div id="status" role="status"></div>
          <form id="composer">
            <textarea id="message-input" placeholder="Ask nanobot-rs to inspect, edit, or research."></textarea>
            <div class="composer-actions">
              <button id="send-button" type="submit">Send</button>
              <button id="new-chat-button" type="button">New chat</button>
              <button id="duplicate-session-button" type="button" hidden>Duplicate to Web</button>
            </div>
          </form>
        </section>
      </section>
    </main>
    <script>
      const INITIAL_ASSISTANT_MESSAGE = "Web UI ready. Ask nanobot-rs to inspect the workspace, edit files, or research something.";
      const SESSION_KEY = "nanobot-rs.sessionId";
      const SELECTED_CHANNEL_KEY = "nanobot-rs.selectedChannel";
      const SELECTED_SESSION_KEY = "nanobot-rs.selectedSessionId";
      const composer = document.getElementById("composer");
      const transcript = document.getElementById("transcript");
      const sessionList = document.getElementById("session-list");
      const messageInput = document.getElementById("message-input");
      const sendButton = document.getElementById("send-button");
      const newChatButton = document.getElementById("new-chat-button");
      const duplicateButton = document.getElementById("duplicate-session-button");
      const statusNode = document.getElementById("status");
      const currentProfileNode = document.getElementById("active-profile");
      const weixinAccountPanel = document.getElementById("weixin-account-panel");
      const weixinStatusLabel = document.getElementById("weixin-status-label");
      const weixinUserLabel = document.getElementById("weixin-user-label");
      const weixinQrPanel = document.getElementById("weixin-qr-panel");
      const weixinQrImage = document.getElementById("weixin-qr-image");
      const weixinLoginButton = document.getElementById("weixin-login-button");
      const weixinLogoutButton = document.getElementById("weixin-logout-button");
      const legacyStoredSessionId = localStorage.getItem(SESSION_KEY);
      let currentChannel = null;
      let currentSessionId = null;
      let currentSessionReadOnly = false;
      let currentSessionCanDuplicate = false;
      let currentSessionGroups = [];
      let pendingSelectionToken = 0;
      let weixinPollTimer = null;
      let isBusy = false;

      function appendMessage(role, content) {
        const node = document.createElement("article");
        node.className = "message";
        node.dataset.role = role;
        node.textContent = content;
        transcript.appendChild(node);
        transcript.scrollTop = transcript.scrollHeight;
      }

      function appendAssistantMessage(content) {
        const node = document.createElement("article");
        node.className = "message";
        node.dataset.role = "assistant";
        node.innerHTML = content;
        transcript.appendChild(node);
        transcript.scrollTop = transcript.scrollHeight;
      }

      function setCurrentProfile(profile) {
        currentProfileNode.textContent = profile || "default";
      }

      function setStatus(message, variant = "idle") {
        statusNode.textContent = message;
        statusNode.dataset.variant = variant;
      }

      function setBusy(busy) {
        isBusy = busy;
        sendButton.disabled = busy || currentSessionReadOnly;
        newChatButton.disabled = busy;
        duplicateButton.disabled = busy;
        sendButton.textContent = busy ? "Working..." : "Send";
      }

      function renderTranscript(messages) {
        transcript.innerHTML = "";
        if (!messages.length) {
          appendAssistantMessage(INITIAL_ASSISTANT_MESSAGE);
          return;
        }
        for (const message of messages || []) {
          if (message.role === "assistant") {
            appendAssistantMessage(message.contentHtml || message.content || "");
          } else if (message.role === "user") {
            appendMessage("user", message.content || "");
          }
        }
      }

      function renderSessionDetail(detail) {
        transcript.innerHTML = "";
        for (const message of detail.messages || []) {
          if (message.role === "assistant") {
            appendAssistantMessage(message.contentHtml || message.content || "");
          } else if (message.role === "user") {
            appendMessage("user", message.content || "");
          }
        }
        if (!(detail.messages || []).length) {
          appendAssistantMessage(INITIAL_ASSISTANT_MESSAGE);
        }
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

      function renderSessionList(groups) {
        sessionList.innerHTML = "";
        for (const group of groups) {
          const groupNode = document.createElement("section");
          groupNode.className = "session-group";

          const heading = document.createElement("div");
          heading.className = "session-group-title";
          heading.textContent = group.channel;
          groupNode.appendChild(heading);

          for (const session of group.sessions || []) {
            const node = document.createElement("button");
            node.type = "button";
            node.className = "session-item";
            node.dataset.selected = String(
              session.channel === currentChannel && session.sessionId === currentSessionId
            );

            const title = document.createElement("div");
            title.className = "session-item-title";
            title.textContent = session.sessionId;

            const preview = document.createElement("div");
            preview.className = "session-item-preview";
            preview.textContent = session.preview || "New session";

            const meta = document.createElement("div");
            meta.className = "session-item-meta";
            meta.textContent = session.activeProfile || "default";

            node.appendChild(title);
            node.appendChild(preview);
            node.appendChild(meta);
            node.addEventListener("click", async () => {
              await selectSession(session.channel, session.sessionId);
              messageInput.focus();
            });
            groupNode.appendChild(node);
          }

          sessionList.appendChild(groupNode);
        }
      }

      function updateSessionMetadata(channel, sessionId, activeProfile) {
        currentSessionGroups = currentSessionGroups.map((group) => ({
          ...group,
          sessions: (group.sessions || []).map((session) => {
            if (session.channel !== channel || session.sessionId !== sessionId) {
              return session;
            }
            return {
              ...session,
              activeProfile: activeProfile || session.activeProfile,
            };
          }),
        }));
        renderSessionList(currentSessionGroups);
      }

      function findSession(groups, channel, sessionId) {
        if (!channel || !sessionId) {
          return null;
        }
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

      function normalizeWeixinQrSource(content) {
        const value = (content || "").trim();
        if (!value) {
          return "";
        }
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

      function renderWeixinAccount(account) {
        const enabled = account?.enabled === true;
        const loggedIn = account?.loggedIn === true;
        const expired = account?.expired === true;
        const userId = account?.userId || account?.botId || "Login from the embedded console.";

        weixinAccountPanel.hidden = false;
        weixinLoginButton.disabled = !enabled || loggedIn;
        weixinLogoutButton.disabled = !enabled || !loggedIn;

        if (!enabled) {
          clearWeixinPollTimer();
          weixinStatusLabel.textContent = "Weixin channel disabled";
          weixinUserLabel.textContent = "Enable channels.weixin to use QR login.";
          weixinQrPanel.hidden = true;
          weixinQrImage.src = "";
          return;
        }

        if (loggedIn && !expired) {
          clearWeixinPollTimer();
          weixinStatusLabel.textContent = "Connected";
          weixinUserLabel.textContent = userId;
          weixinQrPanel.hidden = true;
          weixinQrImage.src = "";
          return;
        }

        if (expired) {
          clearWeixinPollTimer();
          weixinStatusLabel.textContent = "Login expired";
          weixinUserLabel.textContent = userId;
          return;
        }

        weixinStatusLabel.textContent = "Not connected";
        weixinUserLabel.textContent = userId;
      }

      async function fetchSessions() {
        const response = await fetch("/api/sessions");
        const payload = await response.json();
        if (!response.ok) {
          throw new Error(payload.error || "Failed to load sessions");
        }
        return payload.groups || [];
      }

      async function fetchSessionDetail(channel, sessionId) {
        const response = await fetch(`/api/sessions/${channel}/${sessionId}`);
        const detail = await response.json();
        if (!response.ok) {
          throw new Error(detail.error || "Failed to load session");
        }
        detail.channel = detail.channel || channel;
        detail.sessionId = detail.sessionId || sessionId;
        return detail;
      }

      async function createSession() {
        const response = await fetch("/api/sessions", {
          method: "POST",
        });
        const payload = await response.json();
        if (!response.ok) {
          throw new Error(payload.error || "Failed to create session");
        }
        return payload;
      }

      async function duplicateSession() {
        const response = await fetch("/api/sessions/duplicate", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ channel: currentChannel, sessionId: currentSessionId }),
        });
        const payload = await response.json();
        if (!response.ok) {
          throw new Error(payload.error || "Failed to duplicate session");
        }
        return payload;
      }

      async function refreshSessions() {
        currentSessionGroups = await fetchSessions();
        renderSessionList(currentSessionGroups);
        if (
          currentChannel &&
          currentSessionId &&
          !findSession(currentSessionGroups, currentChannel, currentSessionId)
        ) {
          setSelectedSession(null, null);
        }
        return currentSessionGroups;
      }

      async function loadWeixinAccount() {
        const response = await fetch("/api/weixin/account");
        const payload = await response.json();
        if (!response.ok) {
          throw new Error(payload.error || "Failed to load Weixin account");
        }
        renderWeixinAccount(payload);
        return payload;
      }

      async function startWeixinLogin() {
        const response = await fetch("/api/weixin/login/start", {
          method: "POST",
        });
        const payload = await response.json();
        if (!response.ok) {
          throw new Error(payload.error || "Failed to start Weixin login");
        }
        weixinQrPanel.hidden = false;
        weixinQrImage.src = normalizeWeixinQrSource(payload.qrcodeImgContent || "");
        weixinStatusLabel.textContent = "Waiting for scan";
        weixinUserLabel.textContent = "Scan the QR code in Weixin.";
        scheduleWeixinPoll();
      }

      async function pollWeixinLoginStatus() {
        try {
          const response = await fetch("/api/weixin/login/status");
          const payload = await response.json();
          if (!response.ok) {
            throw new Error(payload.error || "Failed to poll Weixin login");
          }

          if (payload.status === "confirmed") {
            weixinStatusLabel.textContent = "Connected";
            weixinQrPanel.hidden = true;
            clearWeixinPollTimer();
            await loadWeixinAccount();
            await refreshSessions();
            return;
          }

          if (payload.expired === true || payload.status === "expired") {
            weixinStatusLabel.textContent = "Login expired";
            weixinUserLabel.textContent = "Refresh the QR code to try again.";
            clearWeixinPollTimer();
            await loadWeixinAccount();
            return;
          }

          if (payload.status === "scaned") {
            weixinStatusLabel.textContent = "QR scanned";
            weixinUserLabel.textContent = "Confirm login in Weixin.";
          } else {
            weixinStatusLabel.textContent = "Waiting for scan";
          }

          scheduleWeixinPoll();
        } catch (error) {
          clearWeixinPollTimer();
          setStatus(error?.message || "Failed to poll Weixin login", "error");
          await loadWeixinAccount().catch(() => {});
        }
      }

      async function selectSession(channel, sessionId) {
        const selectionToken = ++pendingSelectionToken;
        const detail = await fetchSessionDetail(channel, sessionId);
        if (selectionToken !== pendingSelectionToken) {
          return;
        }
        setSelectedSession(channel, sessionId);
        renderSessionDetail(detail);
        setCurrentProfile(detail.activeProfile || "");
        setComposerAccess(detail.readOnly === true, detail.canDuplicate === true);
        renderSessionList(currentSessionGroups);
      }

      async function bootstrapSessions() {
        const storedChannel = localStorage.getItem(SELECTED_CHANNEL_KEY);
        const storedSessionId = localStorage.getItem(SELECTED_SESSION_KEY);
        const restoredSessionId = storedSessionId || legacyStoredSessionId;
        const sessions = await fetchSessions();
        currentSessionGroups = sessions;
        renderSessionList(currentSessionGroups);

        const groups = sessions;
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

      newChatButton.addEventListener("click", async () => {
        setBusy(true);
        setStatus("Starting a new session...", "loading");
        try {
          setSelectedSession(null, null);
          const created = await createSession();
          await refreshSessions();
          await selectSession(created.channel || "web", created.sessionId);
          setStatus("New session started.", "idle");
        } catch (error) {
          setStatus(error?.message || "Failed to create session", "error");
        } finally {
          setBusy(false);
          messageInput.focus();
        }
      });

      renderTranscript([]);
      setCurrentProfile("");
      setComposerAccess(false, false);

      duplicateButton.addEventListener("click", async () => {
        if (!currentSessionId || !currentSessionCanDuplicate) {
          return;
        }
        setBusy(true);
        setStatus("Duplicating session to Web...", "loading");
        try {
          const duplicated = await duplicateSession();
          await refreshSessions();
          await selectSession(duplicated.channel, duplicated.sessionId);
          setStatus("Session duplicated to Web.", "idle");
        } catch (error) {
          setStatus(error?.message || "Failed to duplicate session", "error");
        } finally {
          setBusy(false);
          messageInput.focus();
        }
      });

      weixinLoginButton.addEventListener("click", async () => {
        clearWeixinPollTimer();
        weixinQrPanel.hidden = true;
        try {
          setStatus("Starting Weixin login...", "loading");
          await startWeixinLogin();
          setStatus("Scan the Weixin QR code to continue.", "idle");
        } catch (error) {
          weixinQrPanel.hidden = true;
          setStatus(error?.message || "Failed to start Weixin login", "error");
          await loadWeixinAccount().catch(() => {});
        }
      });

      weixinLogoutButton.addEventListener("click", async () => {
        clearWeixinPollTimer();
        try {
          setStatus("Disconnecting Weixin...", "loading");
          const response = await fetch("/api/weixin/logout", {
            method: "POST",
          });
          const payload = await response.json();
          if (!response.ok) {
            throw new Error(payload.error || "Failed to logout Weixin");
          }
          weixinQrPanel.hidden = true;
          weixinQrImage.src = "";
          renderWeixinAccount(payload);
          await loadWeixinAccount();
          await refreshSessions();
          setStatus("Weixin disconnected.", "idle");
        } catch (error) {
          setStatus(error?.message || "Failed to logout Weixin", "error");
        }
      });

      messageInput.addEventListener("keydown", (event) => {
        if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
          event.preventDefault();
          composer.requestSubmit();
        }
      });

      composer.addEventListener("submit", async (event) => {
        event.preventDefault();
        const draft = messageInput.value;
        const message = draft.trim();
        if (!message) {
          setStatus("Enter a message before sending.", "error");
          messageInput.focus();
          return;
        }
        if (currentSessionReadOnly) {
          setStatus("This session is read-only. Duplicate it to Web to continue.", "error");
          return;
        }

        appendMessage("user", message);
        messageInput.value = "";
        setBusy(true);
        setStatus("nanobot-rs is working...", "loading");

        try {
          if (!currentSessionId) {
            setSelectedSession(null, null);
            const created = await createSession();
            await refreshSessions();
            await selectSession(created.channel || "web", created.sessionId);
          }
          const response = await fetch("/api/chat", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ message, channel: currentChannel, sessionId: currentSessionId }),
          });
          const payload = await response.json();
          if (!response.ok) {
            throw new Error(payload.error || "Request failed");
          }
          setSelectedSession(payload.channel || currentChannel, payload.sessionId);
          appendAssistantMessage(payload.replyHtml || "");
          setCurrentProfile(payload.activeProfile || "");
          updateSessionMetadata(
            payload.channel || currentChannel,
            payload.sessionId,
            payload.activeProfile || ""
          );
          await refreshSessions();
          if (message.startsWith("/new") || message.startsWith("/model")) {
            await selectSession(currentChannel, currentSessionId);
          }
          setStatus("", "idle");
        } catch (error) {
          if (!messageInput.value.trim()) {
            messageInput.value = draft;
          }
          setStatus(error?.message || "Request failed", "error");
        } finally {
          setBusy(false);
          messageInput.focus();
        }
      });

      Promise.all([bootstrapSessions(), loadWeixinAccount()]).catch((error) => {
        clearWeixinPollTimer();
        setStatus(error?.message || "Failed to load sessions", "error");
      });
    </script>
  </body>
</html>"#
        .to_string()
}

pub async fn index() -> Html<String> {
    Html(render_index_html())
}
