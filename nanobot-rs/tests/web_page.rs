#[test]
fn page_shell_contains_core_ui_regions() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("nanobot-rs control room"));
    assert!(html.contains("id=\"transcript\""));
    assert!(html.contains("id=\"composer\""));
    assert!(html.contains("id=\"message-input\""));
    assert!(html.contains("id=\"send-button\""));
}

#[test]
fn page_shell_includes_client_behavior_hooks() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("localStorage"));
    assert!(html.contains("/api/chat"));
    assert!(html.contains("aria-live=\"polite\""));
    assert!(html.contains("data-role=\"assistant\""));
}

#[test]
fn page_shell_trims_message_before_submit() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("messageInput.value.trim()"));
    assert!(html.contains("id=\"status\""));
}
