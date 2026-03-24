use axum::{
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "frontend/dist/"]
struct Asset;

pub async fn index_handler() -> Response {
    match Asset::get("index.html") {
        Some(content) => Html(content.data.clone()).into_response(),
        None => (StatusCode::NOT_FOUND, "index.html not found").into_response(),
    }
}

pub async fn static_handler(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> Response {
    match Asset::get(&path) {
        Some(content) => {
            let mime = mime_for_path(&path);
            (
                [(header::CONTENT_TYPE, mime)],
                content.data.clone(),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

fn mime_for_path(path: &str) -> &'static str {
    if path.ends_with(".js") { "application/javascript" }
    else if path.ends_with(".css") { "text/css" }
    else if path.ends_with(".html") { "text/html" }
    else if path.ends_with(".svg") { "image/svg+xml" }
    else { "application/octet-stream" }
}
