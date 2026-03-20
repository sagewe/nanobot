use axum::response::Html;

pub async fn index() -> Html<&'static str> {
    Html("<!doctype html><html><body>nanobot-rs</body></html>")
}
