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
        gap: 0.65rem;
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

      #send-button {
        justify-self: start;
        border: 0;
        border-radius: 999px;
        padding: 0.85rem 1.35rem;
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.92rem;
        color: #fff8ef;
        background: linear-gradient(135deg, #c9622f, #a4461f);
        cursor: pointer;
      }

      #new-chat-button {
        border: 1px solid var(--line);
        border-radius: 999px;
        padding: 0.85rem 1.15rem;
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.92rem;
        color: var(--ink);
        background: rgba(255, 255, 255, 0.72);
        cursor: pointer;
      }

      #send-button[disabled] {
        opacity: 0.65;
        cursor: wait;
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
          <div id="session-list" aria-live="polite"></div>
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
            </div>
          </form>
        </section>
      </section>
    </main>
    <script>
      const INITIAL_ASSISTANT_MESSAGE = "Web UI ready. Ask nanobot-rs to inspect the workspace, edit files, or research something.";
      const SESSION_KEY = "nanobot-rs.sessionId";
      const composer = document.getElementById("composer");
      const transcript = document.getElementById("transcript");
      const sessionList = document.getElementById("session-list");
      const messageInput = document.getElementById("message-input");
      const sendButton = document.getElementById("send-button");
      const newChatButton = document.getElementById("new-chat-button");
      const statusNode = document.getElementById("status");
      const currentProfileNode = document.getElementById("active-profile");
      const storedSessionId = localStorage.getItem(SESSION_KEY);
      let currentSessionId = null;
      let currentSessions = [];

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
        sendButton.disabled = busy;
        newChatButton.disabled = busy;
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

      function setSelectedSession(sessionId) {
        currentSessionId = sessionId;
        if (sessionId) {
          localStorage.setItem(SESSION_KEY, sessionId);
        } else {
          localStorage.removeItem(SESSION_KEY);
        }
      }

      function renderSessionList(sessions) {
        sessionList.innerHTML = "";
        for (const session of sessions) {
          const node = document.createElement("button");
          node.type = "button";
          node.className = "session-item";
          node.dataset.selected = String(session.sessionId === currentSessionId);

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
            await selectSession(session.sessionId);
            messageInput.focus();
          });
          sessionList.appendChild(node);
        }
      }

      function updateSessionMetadata(sessionId, activeProfile) {
        currentSessions = currentSessions.map((session) => {
          if (session.sessionId !== sessionId) {
            return session;
          }
          return {
            ...session,
            activeProfile: activeProfile || session.activeProfile,
          };
        });
        renderSessionList(currentSessions);
      }

      async function fetchSessions() {
        const response = await fetch("/api/sessions");
        const payload = await response.json();
        if (!response.ok) {
          throw new Error(payload.error || "Failed to load sessions");
        }
        return payload.sessions || [];
      }

      async function fetchSessionDetail(sessionId) {
        const safeSessionId = encodeURIComponent(sessionId);
        const response = await fetch(`/api/sessions/${sessionId}`);
        const detail = await response.json();
        if (!response.ok) {
          throw new Error(detail.error || "Failed to load session");
        }
        detail.sessionId = detail.sessionId || decodeURIComponent(safeSessionId);
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

      async function refreshSessions() {
        currentSessions = await fetchSessions();
        renderSessionList(currentSessions);
        return currentSessions;
      }

      async function selectSession(sessionId) {
        setSelectedSession(sessionId);
        const detail = await fetchSessionDetail(sessionId);
        renderSessionDetail(detail);
        setCurrentProfile(detail.activeProfile || "");
        renderSessionList(currentSessions);
      }

      async function bootstrapSessions() {
        const storedSessionId = localStorage.getItem(SESSION_KEY);
        const sessions = await fetchSessions();
        currentSessions = sessions;
        renderSessionList(currentSessions);

        const storedSession = sessions.find((session) => session.sessionId === storedSessionId);
        const initialSession = storedSession || sessions[0];
        if (!initialSession) {
          localStorage.removeItem(SESSION_KEY);
          const created = await createSession();
          await refreshSessions();
          await selectSession(created.sessionId);
          return;
        }
        await selectSession(initialSession.sessionId);
      }

      newChatButton.addEventListener("click", async () => {
        setBusy(true);
        setStatus("Starting a new session...", "loading");
        try {
          localStorage.removeItem(SESSION_KEY);
          const created = await createSession();
          await refreshSessions();
          await selectSession(created.sessionId);
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

        appendMessage("user", message);
        messageInput.value = "";
        setBusy(true);
        setStatus("nanobot-rs is working...", "loading");

        try {
          if (!currentSessionId) {
            localStorage.removeItem(SESSION_KEY);
            const created = await createSession();
            await refreshSessions();
            await selectSession(created.sessionId);
          }
          const response = await fetch("/api/chat", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ message, sessionId: currentSessionId }),
          });
          const payload = await response.json();
          if (!response.ok) {
            throw new Error(payload.error || "Request failed");
          }
          setSelectedSession(payload.sessionId);
          appendAssistantMessage(payload.replyHtml || "");
          setCurrentProfile(payload.activeProfile || "");
          updateSessionMetadata(payload.sessionId, payload.activeProfile || "");
          await refreshSessions();
          if (message.startsWith("/new") || message.startsWith("/model")) {
            await selectSession(currentSessionId);
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

      bootstrapSessions().catch((error) => {
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
