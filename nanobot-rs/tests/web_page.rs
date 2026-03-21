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
    assert!(html.contains("await fetch(`/api/sessions/${sessionId}`)"));
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
    assert!(html.contains("localStorage.setItem(SESSION_KEY, sessionId)"));
    assert!(html.contains("localStorage.removeItem(SESSION_KEY)"));
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

    assert!(html.contains("const storedSessionId = localStorage.getItem(SESSION_KEY);"));
    assert!(html.contains("const sessions = await fetchSessions();"));
    assert!(html.contains("sessions.find((session) => session.sessionId === storedSessionId)"));
    assert!(html.contains("const initialSession = storedSession || sessions[0];"));
    assert!(html.contains("await createSession();"));
    assert!(html.contains("await selectSession(initialSession.sessionId);"));
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
