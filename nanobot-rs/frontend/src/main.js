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
      const ASSISTANT_AVATAR_SVG = `AI`;
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
          const raw = message.content || "";
          try { contentEl.textContent = JSON.stringify(JSON.parse(raw), null, 2); }
          catch { contentEl.textContent = raw; }
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

      (function() {
        const sessionHeader = document.querySelector(".session-header");
        const composerEl = document.getElementById("composer");
        let showTimer = null;
        function hideChrome() {
          sessionHeader.classList.add("scroll-hidden");
          composerEl.classList.add("scroll-hidden");
          clearTimeout(showTimer);
        }

        transcript.addEventListener("touchmove", () => {
          if (!isMobile()) return;
          hideChrome();
        }, { passive: true });
        transcript.addEventListener("touchend", () => {
          if (!isMobile()) return;
          sessionHeader.classList.remove("scroll-hidden");
          composerEl.classList.remove("scroll-hidden");
          clearTimeout(showTimer);
        }, { passive: true });
        transcript.addEventListener("touchcancel", () => {
          if (!isMobile()) return;
          sessionHeader.classList.remove("scroll-hidden");
          composerEl.classList.remove("scroll-hidden");
          clearTimeout(showTimer);
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
