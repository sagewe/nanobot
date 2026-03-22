#[test]
fn page_shell_contains_core_ui_regions() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("nanobot-rs control room"));
    assert!(html.contains("id=\"transcript\""));
    assert!(html.contains("id=\"session-list\""));
    assert!(html.contains("id=\"composer\""));
    assert!(html.contains("id=\"message-input\""));
    assert!(html.contains("id=\"send-button\""));
}

#[test]
fn page_shell_includes_backend_session_api_hooks() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("localStorage"));
    assert!(html.contains("await fetch(\"/api/sessions\")"));
    assert!(html.contains("await fetch(`/api/sessions/${channel}/${sessionId}`)"));
    assert!(html.contains("await fetch(\"/api/sessions\", {"));
    assert!(html.contains("/api/chat"));
    assert!(html.contains("aria-live=\"polite\""));
    assert!(html.contains("data-role=\"assistant\""));
    assert!(html.contains("payload.activeProfile"));
}

#[test]
fn page_shell_trims_message_before_submit() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("messageInput.value.trim()"));
    assert!(html.contains("id=\"status\""));
}

#[test]
fn page_shell_clears_input_before_network_round_trip() {
    let html = nanobot_rs::web::page::render_index_html();

    let clear_index = html
        .find("messageInput.value = \"\";")
        .expect("clear input statement");
    let fetch_index = html.find("await fetch(\"/api/chat\"").expect("fetch call");

    assert!(clear_index < fetch_index);
}

#[test]
fn page_shell_uses_backend_session_ids_instead_of_local_uuid_generation() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("id=\"new-chat-button\""));
    assert!(!html.contains("crypto.randomUUID()"));
    assert!(html.contains("localStorage.setItem(SELECTED_CHANNEL_KEY, channel)"));
    assert!(html.contains("localStorage.setItem(SELECTED_SESSION_KEY, sessionId)"));
    assert!(html.contains("localStorage.removeItem(SELECTED_CHANNEL_KEY)"));
    assert!(html.contains("localStorage.removeItem(SELECTED_SESSION_KEY)"));
}

#[test]
fn page_shell_supports_ctrl_and_cmd_enter_submission() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("messageInput.addEventListener(\"keydown\""));
    assert!(html.contains("event.key === \"Enter\""));
    assert!(html.contains("event.ctrlKey || event.metaKey"));
    assert!(html.contains("composer.requestSubmit()"));
}

#[test]
fn page_shell_bootstraps_from_backend_sessions_and_stored_selection() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("const storedChannel = localStorage.getItem(SELECTED_CHANNEL_KEY);"));
    assert!(html.contains("const storedSessionId = localStorage.getItem(SELECTED_SESSION_KEY);"));
    assert!(html.contains("const sessions = await fetchSessions();"));
    assert!(html.contains("const restoredSessionId = storedSessionId || legacyStoredSessionId;"));
    assert!(html.contains(
        "const storedSession = findSession(groups, storedChannel || \"web\", restoredSessionId);"
    ));
    assert!(
        html.contains(
            "const initialSession = storedSession || findLatestWritableWebSession(groups);"
        )
    );
    assert!(html.contains("await createSession();"));
    assert!(
        html.contains("await selectSession(initialSession.channel, initialSession.sessionId);")
    );
}

#[test]
fn page_shell_replaces_transcript_from_backend_session_detail_messages() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("transcript.innerHTML = \"\";"));
    assert!(html.contains("for (const message of detail.messages || [])"));
    assert!(html.contains("if (message.role === \"assistant\")"));
    assert!(
        html.contains("appendAssistantMessage(message.contentHtml || message.content || \"\");")
    );
    assert!(html.contains("else if (message.role === \"user\")"));
    assert!(html.contains("appendMessage(\"user\", message.content || \"\");"));
}

#[test]
fn page_shell_refreshes_sessions_after_mutations_and_shows_active_profile() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("await refreshSessions();"));
    assert!(html.contains("message.startsWith(\"/model\")"));
    assert!(html.contains("setCurrentProfile(payload.activeProfile || \"\");"));
    assert!(html.contains("session.activeProfile || \"default\""));
    assert!(html.contains("currentProfileNode.textContent"));
}

#[test]
fn page_shell_renders_assistant_messages_as_html() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("node.innerHTML = content;"));
    assert!(html.contains("appendAssistantMessage(payload.replyHtml || \"\");"));
}

#[test]
fn page_shell_commits_session_selection_only_after_detail_load() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("let pendingSelectionToken = 0;"));
    assert!(html.contains("const selectionToken = ++pendingSelectionToken;"));
    assert!(html.contains("if (selectionToken !== pendingSelectionToken)"));

    let fetch_index = html
        .find("const detail = await fetchSessionDetail(channel, sessionId);")
        .expect("detail fetch");
    let commit_index = html
        .find("setSelectedSession(channel, sessionId);")
        .expect("selection commit");

    assert!(fetch_index < commit_index);
}

#[test]
fn page_shell_renders_grouped_session_sections() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("class=\"session-group\""));
    assert!(html.contains("heading.className = \"session-group-title\";"));
    assert!(html.contains("payload.groups || []"));
    assert!(html.contains("for (const group of groups)"));
    assert!(html.contains("group.channel"));
}

#[test]
fn page_shell_supports_persisted_cross_channel_selection_and_legacy_migration() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("const SELECTED_CHANNEL_KEY = \"nanobot-rs.selectedChannel\";"));
    assert!(html.contains("const SELECTED_SESSION_KEY = \"nanobot-rs.selectedSessionId\";"));
    assert!(html.contains("const legacyStoredSessionId = localStorage.getItem(SESSION_KEY);"));
    assert!(html.contains("const storedChannel = localStorage.getItem(SELECTED_CHANNEL_KEY);"));
    assert!(html.contains("const storedSessionId = localStorage.getItem(SELECTED_SESSION_KEY);"));
    assert!(html.contains("const restoredSessionId = storedSessionId || legacyStoredSessionId;"));
    assert!(html.contains("storedChannel || \"web\""));
}

#[test]
fn page_shell_disables_composer_for_read_only_sessions_and_exposes_duplicate_action() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains(
        "const duplicateButton = document.getElementById(\"duplicate-session-button\");"
    ));
    assert!(html.contains("messageInput.disabled = readOnly;"));
    assert!(html.contains("sendButton.disabled = busy || currentSessionReadOnly;"));
    assert!(html.contains("duplicateButton.hidden = !canDuplicate;"));
    assert!(html.contains("Duplicate to Web"));
}

#[test]
fn page_shell_duplicates_non_web_sessions_into_new_web_session_and_switches_selection() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("await fetch(\"/api/sessions/duplicate\", {"));
    assert!(html.contains(
        "body: JSON.stringify({ channel: currentChannel, sessionId: currentSessionId })"
    ));
    assert!(html.contains("const duplicated = await duplicateSession();"));
    assert!(html.contains("await refreshSessions();"));
    assert!(html.contains("await selectSession(duplicated.channel, duplicated.sessionId);"));
}

#[test]
fn page_shell_prefers_writable_web_fallback_and_creates_web_session_when_missing() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("findLatestWritableWebSession(groups)"));
    assert!(html.contains("const webSessions = groups"));
    assert!(html.contains(".filter((session) => session.channel === \"web\" && session.canSend)"));
    assert!(html.contains("if (!initialSession) {"));
    assert!(html.contains("const created = await createSession();"));
    assert!(html.contains("await selectSession(created.channel || \"web\", created.sessionId);"));
}

#[test]
fn page_shell_includes_weixin_account_controls() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("id=\"weixin-account-panel\""));
    assert!(html.contains("Weixin"));
    assert!(html.contains("id=\"weixin-login-button\""));
    assert!(html.contains("Login to Weixin"));
    assert!(html.contains("id=\"weixin-logout-button\""));
    assert!(html.contains("id=\"weixin-qr-image\""));
    assert!(html.contains("id=\"weixin-status-label\""));
}

#[test]
fn page_shell_bootstraps_weixin_account_and_login_polling() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("await fetch(\"/api/weixin/account\")"));
    assert!(html.contains("await fetch(\"/api/weixin/login/start\", {"));
    assert!(html.contains("await fetch(\"/api/weixin/login/status\")"));
    assert!(html.contains("function normalizeWeixinQrSource(content)"));
    assert!(html.contains("value.startsWith(\"data:\")"));
    assert!(html.contains("value.startsWith(\"https://\")"));
    assert!(html.contains("return `data:image/png;base64,${compact}`;"));
    assert!(html.contains(
        "weixinQrImage.src = normalizeWeixinQrSource(payload.qrcodeImgContent || \"\");"
    ));
    assert!(html.contains("setTimeout(() => pollWeixinLoginStatus(), 1500);"));
    assert!(html.contains("await loadWeixinAccount();"));
}

#[test]
fn page_shell_supports_weixin_logout_and_session_refresh() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("await fetch(\"/api/weixin/logout\", {"));
    assert!(html.contains("weixinLogoutButton.addEventListener(\"click\""));
    assert!(html.contains("await refreshSessions();"));
    assert!(html.contains("weixinQrPanel.hidden = true;"));
}
