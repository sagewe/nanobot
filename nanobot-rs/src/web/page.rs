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
        height: 100dvh;
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
        flex-direction: row;
        width: 100%;
      }

      #theme-toggle {
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

      #lang-toggle {
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
        font-size: 0.78rem;
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-weight: 600;
        transition: color 0.15s, border-color 0.15s;
      }

      #lang-toggle:hover {
        color: var(--accent);
        border-color: var(--accent);
      }

      .shell {
        flex: 1;
        min-height: 0;
        display: flex;
        background: var(--panel);
        overflow: hidden;
        padding: 0.75rem 1.25rem;
        gap: 0.75rem;
      }

      .session-rail {
        display: flex;
        flex-direction: column;
        min-height: 0;
        overflow: hidden;
        border-right: 1px solid var(--line);
        box-shadow: 2px 0 8px rgba(44, 44, 44, 0.08);
        background: var(--sidebar-bg);
        width: 15rem;
        flex-shrink: 0;
        transition: width 0.2s ease;
        z-index: 1;
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

      .session-rail[data-collapsed="true"] .sidebar-title {
        display: none;
      }

      .sidebar-footer {
        margin-top: auto;
        display: flex;
        align-items: center;
        gap: 0.4rem;
        padding: 0.65rem 0.4rem;
        border-top: 1px solid var(--line);
        flex-shrink: 0;
      }

      .session-rail[data-collapsed="true"] .sidebar-footer {
        flex-direction: column;
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

      #profile-select {
        flex-shrink: 0;
        border: 1px solid var(--line);
        border-radius: 0.5rem;
        padding: 0.5rem 0.75rem;
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.84rem;
        color: var(--ink);
        background: var(--input-bg);
        cursor: pointer;
        appearance: auto;
        max-width: 14rem;
      }

      #profile-select:focus {
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

      .sidebar-title {
        margin: 0;
        font-size: 1.05rem;
        font-weight: 700;
        line-height: 1;
        white-space: nowrap;
        overflow: hidden;
      }

      .tab-bar-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
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

      .msg-tool-calls {
        display: flex;
        flex-direction: column;
        gap: 0.3rem;
      }

      .msg-tool-summary {
        display: flex;
        align-items: center;
        gap: 0.4rem;
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.78rem;
        color: var(--muted2);
        flex-wrap: wrap;
      }

      .msg-tool-badge {
        display: inline-flex;
        align-items: center;
        padding: 0.1rem 0.4rem;
        border-radius: 0.25rem;
        background: rgba(193, 95, 60, 0.12);
        color: var(--accent);
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.68rem;
        font-weight: 600;
      }

      .msg-badge {
        display: inline-flex;
        align-items: center;
        padding: 0.05rem 0.35rem;
        border-radius: 0.25rem;
        background: var(--line);
        color: var(--muted2);
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.65rem;
      }

      .msg-group[data-role="tool"] .msg-avatar {
        background: var(--sidebar-bg);
        border: 1px solid var(--line);
        color: var(--muted2);
      }

      .msg-group[data-role="tool"] .msg-bubble {
        background: var(--sidebar-bg);
        border: 1px solid var(--line);
        border-radius: 0.3rem 1rem 1rem 1rem;
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.8rem;
      }

      .msg-tool-output-header {
        display: flex;
        align-items: center;
        gap: 0.4rem;
        cursor: pointer;
        user-select: none;
        color: var(--muted2);
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.78rem;
      }

      .msg-tool-output-header::before {
        content: "▶";
        font-size: 0.55rem;
        transition: transform 0.15s;
        flex-shrink: 0;
      }

      .msg-tool-output-header.open::before {
        transform: rotate(90deg);
      }

      .msg-tool-output-content {
        margin-top: 0.5rem;
        padding: 0.5rem 0.65rem;
        background: var(--input-bg);
        border-radius: 0.4rem;
        font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
        font-size: 0.75rem;
        white-space: pre-wrap;
        word-break: break-all;
        max-height: 14rem;
        overflow-y: auto;
        scrollbar-width: thin;
        scrollbar-color: var(--line) transparent;
        display: none;
      }

      .msg-tool-output-content.open {
        display: block;
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
        margin-top: 0.5rem;
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
      }

      .composer-actions {
        display: flex;
        align-items: center;
        gap: 0.4rem;
        justify-content: flex-end;
      }

      #message-input {
        display: block;
        width: 100%;
        min-height: 4.5rem;
        resize: vertical;
        border: 1px solid var(--line);
        border-radius: 0.5rem;
        padding: 0.65rem 1rem;
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

      #duplicate-session-button:hover {
        color: var(--ink);
        border-color: var(--accent);
      }

      #send-button {
        border: 0;
        border-radius: 999px;
        width: 2rem;
        height: 2rem;
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
        :root:not([data-theme="light"]):not([data-theme="dark"]) .msg-group[data-role="assistant"] .msg-bubble { background: #252320; }
        :root:not([data-theme="light"]):not([data-theme="dark"]) .msg-group[data-role="assistant"] .msg-avatar { background: #1a1815; }
        :root:not([data-theme="light"]):not([data-theme="dark"]) .msg-group[data-role="tool"] .msg-bubble { background: #1a1815; }
        :root:not([data-theme="light"]):not([data-theme="dark"]) .msg-group[data-role="tool"] .msg-avatar { background: #1a1815; }
        :root:not([data-theme="light"]):not([data-theme="dark"]) .msg-tool-output-content { background: #252320; }
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
      :root[data-theme="dark"] .msg-group[data-role="assistant"] .msg-bubble { background: #252320; }
      :root[data-theme="dark"] .msg-group[data-role="assistant"] .msg-avatar { background: #1a1815; }
      :root[data-theme="dark"] .msg-group[data-role="tool"] .msg-bubble { background: #1a1815; }
      :root[data-theme="dark"] .msg-group[data-role="tool"] .msg-avatar { background: #1a1815; }
      :root[data-theme="dark"] .msg-tool-output-content { background: #252320; }
      :root[data-theme="dark"] .tab-btn[data-active="true"] { background: #252320; }

      #sidebar-backdrop {
        display: none;
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.45);
        z-index: 199;
      }

      #sidebar-backdrop.visible {
        display: block;
      }

      #mobile-menu-btn {
        display: none;
        border: 1px solid var(--line);
        border-radius: 0.5rem;
        width: 2.25rem;
        height: 2.25rem;
        align-items: center;
        justify-content: center;
        color: var(--muted2);
        background: transparent;
        cursor: pointer;
        flex-shrink: 0;
      }

      @media (max-width: 640px) {
        body {
          height: 100dvh;
        }

        .session-rail {
          position: fixed;
          top: 0;
          left: 0;
          height: 100dvh;
          width: min(80vw, 18rem);
          transform: translateX(-100%);
          transition: transform 0.25s ease;
          z-index: 200;
          box-shadow: 4px 0 16px rgba(0, 0, 0, 0.15);
        }

        .session-rail.mobile-open {
          transform: translateX(0);
        }

        .session-rail[data-collapsed="true"] {
          width: min(80vw, 18rem);
        }

        .session-rail[data-collapsed="true"] .sidebar-title,
        .session-rail[data-collapsed="true"] .tab-label {
          display: initial;
        }

        .session-rail[data-collapsed="true"] .tab-bar {
          align-items: initial;
          border-bottom: 1px solid var(--line);
        }

        #mobile-menu-btn {
          display: flex;
        }

        .session-header {
          flex-wrap: wrap;
        }

        .session-header #session-select {
          flex: 1;
          min-width: 0;
        }

        #session-select,
        #profile-select,
        #message-input {
          font-size: 16px;
        }

        #send-button,
        #theme-toggle,
        #lang-toggle {
          min-width: 2.75rem;
          min-height: 2.75rem;
        }
      }
    </style>
  </head>
  <body>
    <div id="sidebar-backdrop"></div>
    <main id="app">
      <aside class="session-rail">
          <div class="tab-bar" role="tablist">
            <div class="tab-bar-header">
              <h1 class="sidebar-title" data-i18n="app_name">Pikachu</h1>
              <button id="sidebar-toggle" data-i18n-title="toggle_sidebar" title="Toggle sidebar">
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="3" y1="6" x2="21" y2="6"/><line x1="3" y1="12" x2="21" y2="12"/><line x1="3" y1="18" x2="21" y2="18"/></svg>
              </button>
            </div>
            <button class="tab-btn" data-tab="chat" data-active="true" role="tab">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
              <span class="tab-label" data-i18n="tab_chat">Chat</span>
            </button>
            <button class="tab-btn" data-tab="channels" role="tab">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/></svg>
              <span class="tab-label" data-i18n="tab_channels">Channels</span>
            </button>
          </div>
          <div class="sidebar-footer">
            <button id="theme-toggle" data-i18n-title="toggle_theme" title="Toggle theme">
              <svg id="theme-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/></svg>
            </button>
            <button id="lang-toggle">EN</button>
          </div>
        </aside>
        <section class="shell">
        <section class="channels-pane" hidden>
          <section id="weixin-account-panel" class="account-panel">
            <div class="account-panel-header">
              <div class="session-kicker" data-i18n="weixin">Weixin</div>
              <div class="account-actions">
                <button id="weixin-login-button" type="button" data-i18n-title="login_to_weixin" title="Login to Weixin">
                  <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4"/><polyline points="10 17 15 12 10 7"/><line x1="15" y1="12" x2="3" y2="12"/></svg>
                </button>
                <button id="weixin-logout-button" type="button" data-i18n-title="logout" title="Logout">
                  <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" y1="12" x2="9" y2="12"/></svg>
                </button>
              </div>
            </div>
            <div class="account-panel-info">
              <div class="account-status">
                <strong id="weixin-status-label" data-i18n="checking_account">Checking account…</strong>
                <span id="weixin-user-label" data-i18n="login_from_console">Login from the embedded console.</span>
              </div>
            </div>
            <div id="weixin-qr-panel" hidden>
              <img id="weixin-qr-image" data-i18n-alt="weixin_qr_alt" alt="Weixin login QR code" />
              <div id="weixin-qr-note" class="account-muted" data-i18n="scan_to_confirm">Scan in Weixin to confirm login.</div>
            </div>
          </section>
        </section>
        <section class="conversation-pane">
          <div class="session-header">
            <button id="mobile-menu-btn" aria-label="Open sidebar">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="3" y1="6" x2="21" y2="6"/><line x1="3" y1="12" x2="21" y2="12"/><line x1="3" y1="18" x2="21" y2="18"/></svg>
            </button>
            <select id="session-select" data-i18n-aria-label="select_session" aria-label="Select session"></select>
            <select id="profile-select" data-i18n-aria-label="select_model" aria-label="Select model"></select>
          </div>
          <section id="transcript" aria-live="polite"></section>
          <div id="status" role="status"></div>
          <form id="composer">
            <textarea id="message-input" data-i18n-placeholder="input_placeholder" placeholder="Ask Pikachu to inspect, edit, or research. (Enter to send, Ctrl+Enter for newline)"></textarea>
            <div class="composer-actions">
              <button id="duplicate-session-button" type="button" data-i18n="duplicate_to_web" hidden>Duplicate to Web</button>
              <button id="send-button" type="submit" data-i18n-title="send_button" title="Send (Enter)">
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="22" y1="2" x2="11" y2="13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/></svg>
              </button>
            </div>
          </form>
        </section>
      </section>
    </main>
    <script>
      const TRANSLATIONS = {
        en: {
          app_name: "Pikachu",
          page_title: "Pikachu control room",
          control_room: "control room",
          toggle_theme: "Toggle theme",
          toggle_sidebar: "Toggle sidebar",
          tab_chat: "Chat",
          tab_channels: "Channels",
          channel_web: "Web",
          channel_telegram: "Telegram",
          channel_wecom: "WeCom",
          channel_weixin: "Weixin",
          weixin: "Weixin",
          login_to_weixin: "Login to Weixin",
          logout: "Logout",
          checking_account: "Checking account\u2026",
          login_from_console: "Login from the embedded console.",
          weixin_disabled: "Weixin channel disabled",
          enable_weixin: "Enable channels.weixin to use QR login.",
          connected: "Connected",
          login_expired: "Login expired",
          not_connected: "Not connected",
          waiting_for_scan: "Waiting for scan",
          scan_qr_weixin: "Scan the QR code in Weixin.",
          qr_scanned: "QR scanned",
          confirm_login_weixin: "Confirm login in Weixin.",
          refresh_qr: "Refresh the QR code to try again.",
          weixin_qr_alt: "Weixin login QR code",
          scan_to_confirm: "Scan in Weixin to confirm login.",
          select_session: "Select session",
          select_model: "Select model",
          input_placeholder: "Ask Pikachu to inspect, edit, or research. (Enter to send, Ctrl+Enter for newline)",
          send_button: "Send (Enter)",
          new_chat: "New chat",
          duplicate_to_web: "Duplicate to Web",
          initial_message: "Web UI ready. Ask Pikachu to inspect the workspace, edit files, or research something.",
          sender_you: "You",
          sender_tool: "Tool",
          sender_pikachu: "Pikachu",
          copy: "Copy",
          tool_output: "Tool output",
          starting_session: "Starting a new session...",
          session_started: "New session started.",
          failed_create_session: "Failed to create session",
          duplicating_session: "Duplicating session to Web...",
          session_duplicated: "Session duplicated to Web.",
          failed_duplicate_session: "Failed to duplicate session",
          starting_weixin_login: "Starting Weixin login...",
          scan_weixin_qr_continue: "Scan the Weixin QR code to continue.",
          failed_start_weixin: "Failed to start Weixin login",
          disconnecting_weixin: "Disconnecting Weixin...",
          weixin_disconnected: "Weixin disconnected.",
          failed_logout_weixin: "Failed to logout Weixin",
          enter_message: "Enter a message before sending.",
          readonly_session: "This session is read-only. Duplicate it to Web to continue.",
          pikachu_working: "Pikachu is working...",
          failed_load_sessions: "Failed to load sessions",
          failed_load_session: "Failed to load session",
          failed_load_weixin_account: "Failed to load Weixin account",
          failed_poll_weixin: "Failed to poll Weixin login",
          request_failed: "Request failed",
          switch_light: "Switch to light mode",
          switch_dark: "Switch to dark mode",
          lang_toggle_label: "中",
        },
        zh: {
          app_name: "皮卡丘",
          page_title: "皮卡丘控制台",
          control_room: "控制台",
          toggle_theme: "切换主题",
          toggle_sidebar: "切换侧栏",
          tab_chat: "对话",
          tab_channels: "频道",
          channel_web: "Web",
          channel_telegram: "Telegram",
          channel_wecom: "企业微信",
          channel_weixin: "微信",
          weixin: "微信",
          login_to_weixin: "登录微信",
          logout: "退出",
          checking_account: "账号检测中…",
          login_from_console: "请从嵌入控制台登录。",
          weixin_disabled: "微信频道未启用",
          enable_weixin: "启用 channels.weixin 以使用二维码登录。",
          connected: "已连接",
          login_expired: "登录已过期",
          not_connected: "未连接",
          waiting_for_scan: "等待扫码",
          scan_qr_weixin: "请在微信中扫描二维码。",
          qr_scanned: "已扫码",
          confirm_login_weixin: "请在微信中确认登录。",
          refresh_qr: "请刷新二维码重试。",
          weixin_qr_alt: "微信登录二维码",
          scan_to_confirm: "在微信中扫描以确认登录。",
          select_session: "选择会话",
          select_model: "选择模型",
          input_placeholder: "让皮卡丘检查、编辑或研究。（Enter 发送，Ctrl+Enter 换行）",
          send_button: "发送 (Enter)",
          new_chat: "新对话",
          duplicate_to_web: "复制到 Web",
          initial_message: "Web UI 已就绪。让皮卡丘检查工作区、编辑文件或研究内容。",
          sender_you: "你",
          sender_tool: "工具",
          sender_pikachu: "皮卡丘",
          copy: "复制",
          tool_output: "工具输出",
          starting_session: "正在开始新会话...",
          session_started: "新会话已开始。",
          failed_create_session: "创建会话失败",
          duplicating_session: "正在复制会话到 Web...",
          session_duplicated: "会话已复制到 Web。",
          failed_duplicate_session: "复制会话失败",
          starting_weixin_login: "正在启动微信登录...",
          scan_weixin_qr_continue: "扫描微信二维码以继续。",
          failed_start_weixin: "启动微信登录失败",
          disconnecting_weixin: "正在断开微信...",
          weixin_disconnected: "微信已断开。",
          failed_logout_weixin: "退出微信失败",
          enter_message: "请先输入消息。",
          readonly_session: "此会话为只读。请复制到 Web 以继续。",
          pikachu_working: "皮卡丘正在处理...",
          failed_load_sessions: "加载会话列表失败",
          failed_load_session: "加载会话失败",
          failed_load_weixin_account: "加载微信账号失败",
          failed_poll_weixin: "轮询微信登录失败",
          request_failed: "请求失败",
          switch_light: "切换至浅色模式",
          switch_dark: "切换至深色模式",
          lang_toggle_label: "E",
        },
      };

      const LANG_KEY = "pikachu.lang";
      let currentLang = localStorage.getItem(LANG_KEY) ||
        (navigator.language && navigator.language.startsWith("zh") ? "zh" : "en");

      function t(key) {
        return (TRANSLATIONS[currentLang] || TRANSLATIONS.en)[key] || key;
      }

      function tChannel(name) {
        const key = "channel_" + (name || "").toLowerCase();
        const tr = (TRANSLATIONS[currentLang] || TRANSLATIONS.en)[key];
        return tr || name;
      }

      function tToolCount(count) {
        if (currentLang === "zh") return `工具 ${count} 个\u00a0`;
        return `${count} tool${count > 1 ? "s" : ""}\u00a0`;
      }

      function applyI18n() {
        document.documentElement.lang = currentLang;
        document.title = t("page_title");
        document.querySelectorAll("[data-i18n]").forEach((el) => {
          el.textContent = t(el.dataset.i18n);
        });
        document.querySelectorAll("[data-i18n-title]").forEach((el) => {
          el.title = t(el.dataset.i18nTitle);
        });
        document.querySelectorAll("[data-i18n-placeholder]").forEach((el) => {
          el.placeholder = t(el.dataset.i18nPlaceholder);
        });
        document.querySelectorAll("[data-i18n-aria-label]").forEach((el) => {
          el.setAttribute("aria-label", t(el.dataset.i18nAriaLabel));
        });
        document.querySelectorAll("[data-i18n-alt]").forEach((el) => {
          el.alt = t(el.dataset.i18nAlt);
        });
        const langToggle = document.getElementById("lang-toggle");
        if (langToggle) langToggle.textContent = t("lang_toggle_label");
      }

      applyI18n();
      const SESSION_KEY = "pikachu.sessionId";
      const SELECTED_CHANNEL_KEY = "pikachu.selectedChannel";
      const SELECTED_SESSION_KEY = "pikachu.selectedSessionId";
      const composer = document.getElementById("composer");
      const transcript = document.getElementById("transcript");
      const sessionSelect = document.getElementById("session-select");
      const messageInput = document.getElementById("message-input");
      const sendButton = document.getElementById("send-button");
      const duplicateButton = document.getElementById("duplicate-session-button");
      const statusNode = document.getElementById("status");
      const profileSelect = document.getElementById("profile-select");
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
      const TOOL_AVATAR_SVG = `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>`;
      const COPY_SVG = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>`;
      const CHECK_SVG = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>`;

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

      function renderMessage(message, activeProfile) {
        const ts = message.timestamp || null;
        if (message.role === "user") {
          const { group, bubble } = makeMsgGroup("user", { timestamp: ts });
          bubble.textContent = message.content || "";
          transcript.appendChild(group);
        } else if (message.role === "assistant") {
          const { group, bubble } = makeMsgGroup("assistant", { profile: activeProfile || null, timestamp: ts });
          if (message.toolCalls && message.toolCalls.length > 0) {
            const toolsDiv = document.createElement("div");
            toolsDiv.className = "msg-tool-calls";
            const summary = document.createElement("div");
            summary.className = "msg-tool-summary";
            const count = message.toolCalls.length;
            const label = document.createTextNode(tToolCount(count));
            summary.appendChild(label);
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
            contentDiv.innerHTML = message.contentHtml;
            bubble.appendChild(contentDiv);
          } else if (message.content) {
            const contentDiv = document.createElement("div");
            contentDiv.textContent = message.content;
            bubble.appendChild(contentDiv);
          }
          transcript.appendChild(group);
        } else if (message.role === "tool") {
          const { group, bubble } = makeMsgGroup("tool", { timestamp: ts });
          const header = document.createElement("div");
          header.className = "msg-tool-output-header";
          const headerText = document.createTextNode(t("tool_output") + "\u00a0");
          header.appendChild(headerText);
          if (message.toolName) {
            const badge = document.createElement("span");
            badge.className = "msg-tool-badge";
            badge.textContent = message.toolName;
            header.appendChild(badge);
          }
          const contentEl = document.createElement("div");
          contentEl.className = "msg-tool-output-content";
          contentEl.textContent = message.content || "";
          header.addEventListener("click", () => {
            header.classList.toggle("open");
            contentEl.classList.toggle("open");
          });
          bubble.appendChild(header);
          bubble.appendChild(contentEl);
          transcript.appendChild(group);
        }
        transcript.scrollTop = transcript.scrollHeight;
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
        if (profile && profileSelect.querySelector(`option[value="${CSS.escape(profile)}"]`)) {
          profileSelect.value = profile;
        }
      }

      async function loadProfiles() {
        try {
          const response = await fetch("/api/profiles");
          const payload = await response.json();
          if (!response.ok) return;
          const profiles = payload.profiles || [];
          profileSelect.innerHTML = "";
          for (const p of profiles) {
            const opt = document.createElement("option");
            opt.value = p;
            opt.textContent = p;
            profileSelect.appendChild(opt);
          }
        } catch (_) {}
      }

      profileSelect.addEventListener("change", async () => {
        const profile = profileSelect.value;
        if (!profile || !currentChannel || !currentSessionId) return;
        try {
          await fetch(`/api/sessions/${currentChannel}/${currentSessionId}/profile`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ profile }),
          });
        } catch (_) {}
      });

      function setStatus(message, variant = "idle") {
        statusNode.textContent = message;
        statusNode.dataset.variant = variant;
      }

      function setBusy(busy) {
        isBusy = busy;
        sendButton.disabled = busy || currentSessionReadOnly;
        sessionSelect.disabled = busy;
        duplicateButton.disabled = busy;
      }

      function renderTranscript(messages, activeProfile) {
        transcript.innerHTML = "";
        if (!messages.length) {
          appendAssistantMessage(t("initial_message"));
          return;
        }
        for (const message of messages || []) {
          renderMessage(message, activeProfile || "");
        }
      }

      function renderSessionDetail(detail) {
        transcript.innerHTML = "";
        const activeProfile = detail.activeProfile || "";
        const messages = detail.messages || [];
        if (!messages.length) {
          appendAssistantMessage(t("initial_message"));
          return;
        }
        for (const message of messages) {
          renderMessage(message, activeProfile);
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
        const userId = account?.userId || account?.botId || t("login_from_console");
        weixinLoginButton.disabled = !enabled || loggedIn;
        weixinLogoutButton.disabled = !enabled || !loggedIn;

        if (!enabled) {
          clearWeixinPollTimer();
          weixinStatusLabel.textContent = t("weixin_disabled");
          weixinUserLabel.textContent = t("enable_weixin");
          weixinQrPanel.hidden = true;
          weixinQrImage.src = "";
          return;
        }

        if (loggedIn && !expired) {
          clearWeixinPollTimer();
          weixinStatusLabel.textContent = t("connected");
          weixinUserLabel.textContent = userId;
          weixinQrPanel.hidden = true;
          weixinQrImage.src = "";
          return;
        }

        if (expired) {
          clearWeixinPollTimer();
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

      async function fetchSessions() {
        const response = await fetch("/api/sessions");
        const payload = await response.json();
        if (!response.ok) {
          throw new Error(payload.error || t("failed_load_sessions"));
        }
        return payload.groups || [];
      }

      async function fetchSessionDetail(channel, sessionId) {
        const response = await fetch(`/api/sessions/${channel}/${sessionId}`);
        const detail = await response.json();
        if (!response.ok) {
          throw new Error(detail.error || t("failed_load_session"));
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
          throw new Error(payload.error || t("failed_create_session"));
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
          throw new Error(payload.error || t("failed_duplicate_session"));
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
          throw new Error(payload.error || t("failed_load_weixin_account"));
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
          throw new Error(payload.error || t("failed_start_weixin"));
        }
        weixinQrPanel.hidden = false;
        weixinQrImage.src = normalizeWeixinQrSource(payload.qrcodeImgContent || "");
        weixinStatusLabel.textContent = t("waiting_for_scan");
        weixinUserLabel.textContent = t("scan_qr_weixin");
        scheduleWeixinPoll();
      }

      async function pollWeixinLoginStatus() {
        try {
          const response = await fetch("/api/weixin/login/status");
          const payload = await response.json();
          if (!response.ok) {
            throw new Error(payload.error || t("failed_poll_weixin"));
          }

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
          themeToggle.title = t("switch_light");
        } else {
          document.documentElement.setAttribute("data-theme", "light");
          themeIcon.innerHTML = MOON_ICON;
          themeToggle.title = t("switch_dark");
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
      const backdropEl = document.getElementById("sidebar-backdrop");
      const mobileMenuBtn = document.getElementById("mobile-menu-btn");

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

      renderTranscript([]);
      setComposerAccess(false, false);

      duplicateButton.addEventListener("click", async () => {
        if (!currentSessionId || !currentSessionCanDuplicate) {
          return;
        }
        setBusy(true);
        setStatus(t("duplicating_session"), "loading");
        try {
          const duplicated = await duplicateSession();
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

      weixinLoginButton.addEventListener("click", async () => {
        clearWeixinPollTimer();
        weixinQrPanel.hidden = true;
        try {
          setStatus(t("starting_weixin_login"), "loading");
          await startWeixinLogin();
          setStatus(t("scan_weixin_qr_continue"), "idle");
        } catch (error) {
          weixinQrPanel.hidden = true;
          setStatus(error?.message || t("failed_start_weixin"), "error");
          await loadWeixinAccount().catch(() => {});
        }
      });

      weixinLogoutButton.addEventListener("click", async () => {
        clearWeixinPollTimer();
        try {
          setStatus(t("disconnecting_weixin"), "loading");
          const response = await fetch("/api/weixin/logout", {
            method: "POST",
          });
          const payload = await response.json();
          if (!response.ok) {
            throw new Error(payload.error || t("failed_logout_weixin"));
          }
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

      messageInput.addEventListener("focus", () => {
        if (isMobile()) {
          setTimeout(() => messageInput.scrollIntoView({ behavior: "smooth", block: "nearest" }), 300);
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
          const response = await fetch("/api/chat", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ message, channel: currentChannel, sessionId: currentSessionId }),
          });
          const payload = await response.json();
          if (!response.ok) {
            throw new Error(payload.error || t("request_failed"));
          }
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

      const langToggleBtn = document.getElementById("lang-toggle");
      langToggleBtn.addEventListener("click", () => {
        currentLang = currentLang === "en" ? "zh" : "en";
        localStorage.setItem(LANG_KEY, currentLang);
        applyI18n();
        applyTheme(document.documentElement.getAttribute("data-theme") || "light");
        renderSessionSelect(currentSessionGroups);
      });

      Promise.all([bootstrapSessions(), loadWeixinAccount(), loadProfiles()]).catch((error) => {
        clearWeixinPollTimer();
        setStatus(error?.message || t("failed_load_sessions"), "error");
      });
    </script>
  </body>
</html>"#
        .to_string()
}

pub async fn index() -> Html<String> {
    Html(render_index_html())
}
