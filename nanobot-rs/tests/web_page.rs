#[test]
fn page_shell_contains_core_ui_regions() {
    let html = nanobot_rs::web::page::render_index_html();

    assert!(html.contains("nanobot-rs control room"));
    assert!(html.contains("id=\"transcript\""));
    assert!(html.contains("id=\"composer\""));
    assert!(html.contains("id=\"message-input\""));
    assert!(html.contains("id=\"send-button\""));
}
