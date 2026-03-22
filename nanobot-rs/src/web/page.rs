use axum::response::Html;

pub fn render_index_html() -> String {
    r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Pikachu control room</title>
    <style>
      :root {
        --paper: #faf9f5;
        --ink: #2c2c2c;
        --muted: #a19a94;
        --muted2: #6a6a6a;
        --accent: #C15F3C;
        --accent-dark: #A14A2F;
        --panel: rgba(253, 252, 251, 0.92);
        --sidebar-bg: #f0efed;
        --line: #e8e6e3;
        --shadow: 0 8px 32px rgba(44, 44, 44, 0.1);
        --input-bg: #ffffff;
        --error: #d73a49;
      }

      * {
        box-sizing: border-box;
      }

      [hidden] {
        display: none !important;
      }

      body {
        margin: 0;
        height: 100vh;
        display: flex;
        flex-direction: column;
        overflow: hidden;
        color: var(--ink);
        background: linear-gradient(160deg, #fdfcfb 0%, #f5f4ed 100%);
        font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
      }

      #app {
        flex: 1;
        min-height: 0;
        display: flex;
        flex-direction: column;
        width: 100%;
        padding: 0.75rem 1.25rem;
        gap: 0.75rem;
      }

      .topbar {
        display: flex;
        align-items: center;
        gap: 0.75rem;
        flex-shrink: 0;
      }

      .topbar h1 {
        margin: 0;
        font-size: 1.05rem;
        font-weight: 700;
        line-height: 1;
      }

      .topbar .eyebrow {
        color: var(--accent);
        text-transform: uppercase;
        letter-spacing: 0.15em;
        font-size: 0.72rem;
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
      }

      .topbar-sep {
        width: 1px;
        height: 1rem;
        background: var(--line);
      }

      #theme-toggle {
        margin-left: auto;
        border: 1px solid var(--line);
        border-radius: 0.5rem;
        width: 2rem;
        height: 2rem;
        display: flex;
        align-items: center;
        justify-content: center;
        color: var(--muted2);
        background: transparent;
        cursor: pointer;
        transition: color 0.15s, border-color 0.15s;
      }

      #theme-toggle:hover {
        color: var(--accent);
        border-color: var(--accent);
      }

      .shell {
        flex: 1;
        min-height: 0;
        display: grid;
        gap: 0;
        grid-template-columns: auto minmax(0, 1fr);
        border: 1px solid var(--line);
        border-radius: 1rem;
        background: var(--panel);
        backdrop-filter: blur(12px);
        box-shadow: var(--shadow);
        overflow: hidden;
      }

      .session-rail {
        display: flex;
        flex-direction: column;
        min-height: 0;
        overflow: hidden;
        border-right: 1px solid var(--line);
        width: 15rem;
        flex-shrink: 0;
        transition: width 0.2s ease;
      }

      .session-rail[data-collapsed="true"] {
        width: 2.75rem;
      }

      .session-rail[data-collapsed="true"] .tab-panel {
        display: none;
      }

      .session-rail[data-collapsed="true"] .tab-label {
        display: none;
      }

      .session-rail[data-collapsed="true"] .tab-bar {
        border-bottom: none;
        align-items: center;
      }

      .session-header {
        display: flex;
        align-items: center;
        gap: 0.75rem;
        flex-shrink: 0;
      }

      .session-header #session-select {
        width: min(22rem, 100%);
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

      #session-select {
        width: 100%;
        border: 1px solid var(--line);
        border-radius: 0.5rem;
        padding: 0.5rem 0.75rem;
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.84rem;
        color: var(--ink);
        background: var(--input-bg);
        cursor: pointer;
        appearance: auto;
      }

      #session-select:focus {
        outline: 2px solid var(--accent);
        outline-offset: 2px;
      }

      .tab-bar {
        display: flex;
        flex-direction: column;
        gap: 0.2rem;
        flex-shrink: 0;
        padding: 0.65rem 0.4rem;
        background: var(--sidebar-bg);
      }

      .tab-btn {
        border: none;
        border-radius: 0.5rem;
        padding: 0.5rem 0.55rem;
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.78rem;
        color: var(--muted);
        background: transparent;
        cursor: pointer;
        text-align: left;
        white-space: nowrap;
        display: flex;
        align-items: center;
        gap: 0.5rem;
        transition: color 0.15s, background 0.15s;
      }

      .tab-btn:hover {
        color: var(--ink);
        background: rgba(193, 95, 60, 0.08);
      }

      .tab-btn[data-active="true"] {
        color: var(--accent);
        background: #fdfcfb;
        font-weight: 600;
      }

      .tab-bar-header {
        display: flex;
        justify-content: flex-end;
        padding-bottom: 0.4rem;
        border-bottom: 1px solid var(--line);
        margin-bottom: 0.2rem;
      }

      #sidebar-toggle {
        border: none;
        border-radius: 0.4rem;
        padding: 0.35rem;
        width: 1.75rem;
        height: 1.75rem;
        display: flex;
        align-items: center;
        justify-content: center;
        color: var(--muted);
        background: transparent;
        cursor: pointer;
        align-self: center;
        transition: color 0.15s, background 0.15s;
        flex-shrink: 0;
      }

      #sidebar-toggle:hover {
        color: var(--ink);
        background: rgba(193, 95, 60, 0.08);
      }

      .tab-panel {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
        min-height: 0;
        overflow-y: auto;
        padding: 0.75rem;
        scrollbar-width: thin;
        scrollbar-color: var(--line) transparent;
      }

      .tab-panel[hidden] {
        display: none;
      }

      .account-panel {
        display: flex;
        flex-direction: column;
        gap: 1.25rem;
        padding: 1.5rem 1.75rem;
        border: 1px solid var(--line);
        border-radius: 0.75rem;
        background: #fdfcfb;
        max-width: 36rem;
      }

      .account-panel-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 1rem;
      }

      .account-panel-info {
        display: flex;
        flex-direction: column;
        gap: 1rem;
      }

      .account-status {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
      }

      .account-status strong {
        font-size: 1.05rem;
        font-weight: 600;
      }

      .account-status span {
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.82rem;
        color: var(--muted);
      }

      .account-muted {
        color: var(--muted);
        font-size: 0.85rem;
        line-height: 1.5;
        text-align: center;
      }

      .account-actions {
        display: flex;
        gap: 0.4rem;
      }

      .account-actions button {
        border: 1px solid var(--line);
        border-radius: 0.5rem;
        width: 2.1rem;
        height: 2.1rem;
        display: flex;
        align-items: center;
        justify-content: center;
        color: var(--muted2);
        background: var(--input-bg);
        cursor: pointer;
        transition: color 0.15s, background 0.15s, border-color 0.15s;
      }

      .account-actions button:hover:not([disabled]) {
        color: var(--accent);
        border-color: var(--accent);
        background: rgba(193, 95, 60, 0.06);
      }

      .account-actions button[disabled] {
        opacity: 0.35;
        cursor: not-allowed;
      }

      #weixin-qr-panel {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 0.75rem;
        padding: 1rem;
        border-radius: 0.75rem;
        border: 1px dashed var(--line);
        background: #f8f7f6;
      }

      #weixin-qr-image {
        width: min(100%, 16rem);
        border-radius: 0.75rem;
        background: white;
      }

      .conversation-pane {
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
        min-height: 0;
        padding: 0.75rem 1rem;
      }

      .channels-pane {
        display: flex;
        flex-direction: column;
        gap: 1.5rem;
        min-height: 0;
        overflow-y: auto;
        padding: 2rem 2.5rem;
        scrollbar-width: thin;
        scrollbar-color: var(--line) transparent;
      }

      #transcript {
        flex: 1;
        min-height: 0;
        overflow-y: auto;
        scrollbar-width: thin;
        scrollbar-color: var(--line) transparent;
        padding: 1rem;
        border: 1px solid var(--line);
        border-radius: 0.75rem;
        background: #f8f7f6;
        font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Georgia, serif;
        display: grid;
        gap: 0.75rem;
        align-content: start;
      }

      .msg-group {
        display: flex;
        gap: 0.6rem;
        align-items: flex-start;
        max-width: min(48rem, 100%);
      }

      .msg-group[data-role="user"] {
        justify-self: end;
        flex-direction: row-reverse;
      }

      .msg-group[data-role="assistant"] {
        justify-self: start;
      }

      .msg-avatar {
        width: 1.9rem;
        height: 1.9rem;
        border-radius: 50%;
        flex-shrink: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        margin-top: 0.1rem;
      }

      .msg-group[data-role="user"] .msg-avatar {
        background: linear-gradient(135deg, var(--accent), var(--accent-dark));
        color: #fff;
      }

      .msg-group[data-role="assistant"] .msg-avatar {
        background: var(--sidebar-bg);
        border: 1px solid var(--line);
        color: var(--accent);
      }

      .msg-body {
        display: flex;
        flex-direction: column;
        gap: 0.28rem;
        min-width: 0;
      }

      .msg-bubble {
        padding: 0.75rem 1rem;
        line-height: 1.6;
        border-radius: 1rem;
      }

      .msg-group[data-role="user"] .msg-bubble {
        color: #ffffff;
        background: linear-gradient(135deg, var(--accent), var(--accent-dark));
        white-space: pre-wrap;
        border-radius: 1rem 0.3rem 1rem 1rem;
      }

      .msg-group[data-role="assistant"] .msg-bubble {
        background: #fdfcfb;
        border: 1px solid var(--line);
        border-radius: 0.3rem 1rem 1rem 1rem;
      }

      .msg-footer {
        display: flex;
        align-items: center;
        gap: 0.45rem;
        padding: 0 0.2rem;
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.7rem;
        color: var(--muted);
      }

      .msg-group[data-role="user"] .msg-footer {
        justify-content: flex-end;
      }

      .msg-sender {
        font-weight: 600;
      }

      .msg-copy {
        border: none;
        background: transparent;
        padding: 0.15rem 0.25rem;
        border-radius: 0.3rem;
        cursor: pointer;
        color: var(--muted);
        display: flex;
        align-items: center;
        opacity: 0;
        transition: opacity 0.15s, color 0.15s;
        margin-left: auto;
      }

      .msg-group:hover .msg-copy {
        opacity: 1;
      }

      .msg-copy:hover {
        color: var(--accent);
      }

      @keyframes status-pulse {
        0%, 100% { opacity: 1; }
        50% { opacity: 0.45; }
      }

      #status {
        min-height: 1.4rem;
        color: var(--muted);
        font-size: 0.95rem;
        display: flex;
        align-items: center;
        gap: 0.5rem;
      }

      #status[data-variant="loading"] {
        color: var(--accent);
        animation: status-pulse 1.4s ease-in-out infinite;
      }

      #status[data-variant="error"] {
        color: var(--error);
      }

      #composer {
        display: grid;
        gap: 0.8rem;
      }

      .composer-actions {
        display: flex;
        align-items: center;
        gap: 0.6rem;
      }

      #message-input {
        min-height: 8rem;
        resize: vertical;
        border: 1px solid var(--line);
        border-radius: 0.5rem;
        padding: 0.75rem 1rem;
        font: inherit;
        color: var(--ink);
        background: var(--input-bg);
      }

      #message-input::placeholder {
        color: var(--muted);
      }

      #message-input:focus {
        outline: 2px solid var(--accent);
        outline-offset: -1px;
      }

      #new-chat-button,
      #duplicate-session-button {
        border: 1px solid var(--line);
        border-radius: 999px;
        padding: 0.55rem 1rem;
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.82rem;
        color: var(--muted2);
        background: var(--input-bg);
        cursor: pointer;
        transition: color 0.15s, border-color 0.15s;
      }

      #new-chat-button:hover,
      #duplicate-session-button:hover {
        color: var(--ink);
        border-color: var(--accent);
      }

      #send-button {
        margin-left: auto;
        border: 0;
        border-radius: 999px;
        width: 2.4rem;
        height: 2.4rem;
        display: flex;
        align-items: center;
        justify-content: center;
        color: #ffffff;
        background: linear-gradient(135deg, var(--accent), var(--accent-dark));
        cursor: pointer;
        flex-shrink: 0;
        transition: opacity 0.15s;
      }

      #send-button[disabled],
      #new-chat-button[disabled],
      #duplicate-session-button[disabled],
      #message-input[disabled] {
        opacity: 0.5;
        cursor: not-allowed;
      }

      @media (max-width: 720px) {
        body {
          height: 100dvh;
        }

        #app {
          padding: 0.5rem 0.75rem;
        }

        .shell {
          border-radius: 0.75rem;
        }

        .channels-pane {
          padding: 1.25rem;
        }

        .account-panel {
          flex-direction: column;
          gap: 1.25rem;
          max-width: 100%;
        }

        #weixin-qr-image {
          width: min(100%, 14rem);
        }
      }

      :root[data-theme="light"] {
        --paper: #faf9f5;
        --ink: #2c2c2c;
        --muted: #a19a94;
        --muted2: #6a6a6a;
        --accent: #C15F3C;
        --accent-dark: #A14A2F;
        --panel: rgba(253, 252, 251, 0.92);
        --sidebar-bg: #f0efed;
        --line: #e8e6e3;
        --shadow: 0 8px 32px rgba(44, 44, 44, 0.1);
        --input-bg: #ffffff;
        --error: #d73a49;
      }

      :root[data-theme="light"] body {
        background: linear-gradient(160deg, #fdfcfb 0%, #f5f4ed 100%);
      }

      .dark-vars {
        --paper: #1e1c1a;
        --ink: #e8e4de;
        --muted: #7a746e;
        --muted2: #9a9490;
        --accent: #d4724a;
        --accent-dark: #b85c38;
        --panel: rgba(30, 28, 26, 0.95);
        --sidebar-bg: #1a1815;
        --line: #333028;
        --shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
        --input-bg: #252320;
        --error: #f47067;
      }

      @media (prefers-color-scheme: dark) {
        :root:not([data-theme="light"]):not([data-theme="dark"]) {
          --paper: #1e1c1a;
          --ink: #e8e4de;
          --muted: #7a746e;
          --muted2: #9a9490;
          --accent: #d4724a;
          --accent-dark: #b85c38;
          --panel: rgba(30, 28, 26, 0.95);
          --sidebar-bg: #1a1815;
          --line: #333028;
          --shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
          --input-bg: #252320;
          --error: #f47067;
        }

        :root:not([data-theme="light"]):not([data-theme="dark"]) body {
          background: linear-gradient(160deg, #201e1b 0%, #181613 100%);
        }

        :root:not([data-theme="light"]):not([data-theme="dark"]) .account-panel { background: #252320; }
        :root:not([data-theme="light"]):not([data-theme="dark"]) #weixin-qr-panel { background: #1e1c1a; }
        :root:not([data-theme="light"]):not([data-theme="dark"]) #transcript { background: #1e1c1a; }
        :root:not([data-theme="light"]):not([data-theme="dark"]) .msg-group[data-role="assistant"] .msg-bubble { background: #252320; }
        :root:not([data-theme="light"]):not([data-theme="dark"]) .msg-group[data-role="assistant"] .msg-avatar { background: #1a1815; }
        :root:not([data-theme="light"]):not([data-theme="dark"]) .tab-btn[data-active="true"] { background: #252320; }
      }

      :root[data-theme="dark"] {
        --paper: #1e1c1a;
        --ink: #e8e4de;
        --muted: #7a746e;
        --muted2: #9a9490;
        --accent: #d4724a;
        --accent-dark: #b85c38;
        --panel: rgba(30, 28, 26, 0.95);
        --sidebar-bg: #1a1815;
        --line: #333028;
        --shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
        --input-bg: #252320;
        --error: #f47067;
      }

      :root[data-theme="dark"] body { background: linear-gradient(160deg, #201e1b 0%, #181613 100%); }
      :root[data-theme="dark"] .account-panel { background: #252320; }
      :root[data-theme="dark"] #weixin-qr-panel { background: #1e1c1a; }
      :root[data-theme="dark"] #transcript { background: #1e1c1a; }
      :root[data-theme="dark"] .msg-group[data-role="assistant"] .msg-bubble { background: #252320; }
      :root[data-theme="dark"] .msg-group[data-role="assistant"] .msg-avatar { background: #1a1815; }
      :root[data-theme="dark"] .tab-btn[data-active="true"] { background: #252320; }
    </style>
  </head>
  <body>
    <main id="app">
      <header class="topbar">
        <h1>Pikachu</h1>
        <div class="topbar-sep"></div>
        <div class="eyebrow">control room</div>
        <button id="theme-toggle" title="Toggle theme">
          <svg id="theme-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/></svg>
        </button>
      </header>
      <section class="shell">
        <aside class="session-rail">
          <div class="tab-bar" role="tablist">
            <div class="tab-bar-header">
              <button id="sidebar-toggle" title="Toggle sidebar">
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="3" y1="6" x2="21" y2="6"/><line x1="3" y1="12" x2="21" y2="12"/><line x1="3" y1="18" x2="21" y2="18"/></svg>
              </button>
            </div>
            <button class="tab-btn" data-tab="chat" data-active="true" role="tab">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
              <span class="tab-label">Chat</span>
            </button>
            <button class="tab-btn" data-tab="channels" role="tab">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/></svg>
              <span class="tab-label">Channels</span>
            </button>
          </div>
        </aside>
        <section class="channels-pane" hidden>
          <section id="weixin-account-panel" class="account-panel">
            <div class="account-panel-header">
              <div class="session-kicker">Weixin</div>
              <div class="account-actions">
                <button id="weixin-login-button" type="button" title="Login to Weixin">
                  <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4"/><polyline points="10 17 15 12 10 7"/><line x1="15" y1="12" x2="3" y2="12"/></svg>
                </button>
                <button id="weixin-logout-button" type="button" title="Logout">
                  <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" y1="12" x2="9" y2="12"/></svg>
                </button>
              </div>
            </div>
            <div class="account-panel-info">
              <div class="account-status">
                <strong id="weixin-status-label">Checking account…</strong>
                <span id="weixin-user-label">Login from the embedded console.</span>
              </div>
            </div>
            <div id="weixin-qr-panel" hidden>
              <img id="weixin-qr-image" alt="Weixin login QR code" />
              <div id="weixin-qr-note" class="account-muted">Scan in Weixin to confirm login.</div>
            </div>
          </section>
        </section>
        <section class="conversation-pane">
          <div class="session-header">
            <select id="session-select" aria-label="Select session"></select>
            <strong id="active-profile">default</strong>
          </div>
          <section id="transcript" aria-live="polite"></section>
          <div id="status" role="status"></div>
          <form id="composer">
            <textarea id="message-input" placeholder="Ask Pikachu to inspect, edit, or research. (Enter to send, Ctrl+Enter for newline)"></textarea>
            <div class="composer-actions">
              <button id="send-button" type="submit" title="Send (Enter)">
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="22" y1="2" x2="11" y2="13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/></svg>
              </button>
              <button id="new-chat-button" type="button">New chat</button>
              <button id="duplicate-session-button" type="button" hidden>Duplicate to Web</button>
            </div>
          </form>
        </section>
      </section>
    </main>
    <script>
      const INITIAL_ASSISTANT_MESSAGE = "Web UI ready. Ask Pikachu to inspect the workspace, edit files, or research something.";
      const SESSION_KEY = "pikachu.sessionId";
      const SELECTED_CHANNEL_KEY = "pikachu.selectedChannel";
      const SELECTED_SESSION_KEY = "pikachu.selectedSessionId";
      const composer = document.getElementById("composer");
      const transcript = document.getElementById("transcript");
      const sessionSelect = document.getElementById("session-select");
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

      function formatTime(date) {
        return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
      }

      const USER_AVATAR_SVG = `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>`;
      const ASSISTANT_AVATAR_SVG = `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>`;
      const COPY_SVG = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>`;
      const CHECK_SVG = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>`;

      function makeMsgGroup(role) {
        const group = document.createElement("div");
        group.className = "msg-group";
        group.dataset.role = role;

        const avatar = document.createElement("div");
        avatar.className = "msg-avatar";
        avatar.innerHTML = role === "user" ? USER_AVATAR_SVG : ASSISTANT_AVATAR_SVG;

        const body = document.createElement("div");
        body.className = "msg-body";

        const bubble = document.createElement("div");
        bubble.className = "msg-bubble";

        const footer = document.createElement("div");
        footer.className = "msg-footer";

        const sender = document.createElement("span");
        sender.className = "msg-sender";
        sender.textContent = role === "user" ? "You" : "Pikachu";

        const time = document.createElement("span");
        time.className = "msg-time";
        time.textContent = formatTime(new Date());

        footer.appendChild(sender);
        footer.appendChild(time);

        if (role === "assistant") {
          const copyBtn = document.createElement("button");
          copyBtn.className = "msg-copy";
          copyBtn.title = "Copy";
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

      function appendMessage(role, content) {
        const { group, bubble } = makeMsgGroup(role);
        bubble.textContent = content;
        transcript.appendChild(group);
        transcript.scrollTop = transcript.scrollHeight;
      }

      function appendAssistantMessage(content) {
        const { group, bubble } = makeMsgGroup("assistant");
        bubble.innerHTML = content;
        transcript.appendChild(group);
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

      function renderSessionSelect(groups) {
        const prev = sessionSelect.value;
        sessionSelect.innerHTML = "";
        for (const group of groups) {
          const optgroup = document.createElement("optgroup");
          optgroup.label = group.channel;
          for (const session of group.sessions || []) {
            const opt = document.createElement("option");
            opt.value = `${session.channel}::${session.sessionId}`;
            opt.textContent = session.preview
              ? `${session.sessionId} — ${session.preview}`
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
        renderSessionSelect(currentSessionGroups);
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
          weixinQrPanel.hidden = true;
          weixinQrImage.src = "";
          return;
        }

        weixinQrPanel.hidden = true;
        weixinQrImage.src = "";
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
        renderSessionSelect(currentSessionGroups);
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
        renderSessionSelect(currentSessionGroups);
      }

      async function bootstrapSessions() {
        const storedChannel = localStorage.getItem(SELECTED_CHANNEL_KEY);
        const storedSessionId = localStorage.getItem(SELECTED_SESSION_KEY);
        const restoredSessionId = storedSessionId || legacyStoredSessionId;
        const sessions = await fetchSessions();
        currentSessionGroups = sessions;
        renderSessionSelect(currentSessionGroups);

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

      sessionSelect.addEventListener("change", async () => {
        const [channel, sessionId] = sessionSelect.value.split("::");
        if (channel && sessionId) {
          await selectSession(channel, sessionId);
          messageInput.focus();
        }
      });

      const tabButtons = document.querySelectorAll(".tab-btn");
      const conversationPane = document.querySelector(".conversation-pane");
      const channelsPane = document.querySelector(".channels-pane");
      const sessionRail = document.querySelector(".session-rail");
      const sidebarToggle = document.getElementById("sidebar-toggle");

      const THEME_KEY = "pikachu.theme";
      const themeToggle = document.getElementById("theme-toggle");
      const themeIcon = document.getElementById("theme-icon");

      const SUN_ICON = '<circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/>';
      const MOON_ICON = '<path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/>';

      function applyTheme(theme) {
        if (theme === "dark") {
          document.documentElement.setAttribute("data-theme", "dark");
          themeIcon.innerHTML = SUN_ICON;
          themeToggle.title = "Switch to light mode";
        } else {
          document.documentElement.setAttribute("data-theme", "light");
          themeIcon.innerHTML = MOON_ICON;
          themeToggle.title = "Switch to dark mode";
        }
        localStorage.setItem(THEME_KEY, theme);
      }

      const savedTheme = localStorage.getItem(THEME_KEY) ||
        (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
      applyTheme(savedTheme);

      themeToggle.addEventListener("click", () => {
        const current = document.documentElement.getAttribute("data-theme");
        applyTheme(current === "dark" ? "light" : "dark");
      });

      const COLLAPSED_KEY = "pikachu.sidebarCollapsed";

      function setSidebarCollapsed(collapsed) {
        sessionRail.dataset.collapsed = String(collapsed);
        localStorage.setItem(COLLAPSED_KEY, String(collapsed));
      }

      setSidebarCollapsed(localStorage.getItem(COLLAPSED_KEY) === "true");

      sidebarToggle.addEventListener("click", () => {
        setSidebarCollapsed(sessionRail.dataset.collapsed !== "true");
      });

      tabButtons.forEach((btn) => {
        btn.addEventListener("click", () => {
          const tab = btn.dataset.tab;
          tabButtons.forEach((b) => { b.dataset.active = String(b.dataset.tab === tab); });
          conversationPane.hidden = tab !== "chat";
          channelsPane.hidden = tab !== "channels";
        });
      });

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
        setStatus("Pikachu is working...", "loading");

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
