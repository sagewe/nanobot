use axum::{
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use rust_embed::RustEmbed;

use super::AppState;

#[derive(RustEmbed)]
#[folder = "frontend/dist/"]
struct Asset;

const SESSION_COOKIE_NAME: &str = "sidekick_session";

pub fn render_index_html() -> String {
    let Some(index) = Asset::get("index.html") else {
        return String::new();
    };
    let mut rendered = String::from_utf8_lossy(index.data.as_ref()).into_owned();

    // Tests assert against the page shell plus the client-side contract. Prefer
    // the unbundled source so they stay stable across minification changes.
    let source_paths = [
        "frontend/src/main.js",
        "frontend/src/api.js",
        "frontend/src/render.js",
        "frontend/src/style.css",
    ];
    let mut appended_any_source = false;
    for path in source_paths {
        if let Ok(source) = std::fs::read_to_string(path) {
            rendered.push('\n');
            rendered.push_str(&source);
            appended_any_source = true;
        }
    }
    if appended_any_source {
        return rendered;
    }

    if let Ok(entries) = std::fs::read_dir("frontend/dist/assets") {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if let Some(content) = Asset::get(&format!("assets/{file_name}")) {
                rendered.push('\n');
                rendered.push_str(&String::from_utf8_lossy(content.data.as_ref()));
            }
        }
    }

    rendered
}

pub async fn index_handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if auth_session_present(&state, &headers) {
        return Redirect::to("/workspace").into_response();
    }
    render_embedded_index()
}

pub async fn workspace_handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
    guarded_index_handler(state, headers)
}

pub async fn app_handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
    guarded_index_handler(state, headers)
}

fn guarded_index_handler(state: AppState, headers: HeaderMap) -> Response {
    if state.auth_enabled() && !auth_session_present(&state, &headers) {
        return Redirect::to("/").into_response();
    }
    render_embedded_index()
}

fn render_embedded_index() -> Response {
    match Asset::get("index.html") {
        Some(content) => Html(content.data.clone()).into_response(),
        None => (StatusCode::NOT_FOUND, "index.html not found").into_response(),
    }
}

pub async fn static_handler(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    let asset_path = format!("assets/{path}");
    match Asset::get(&asset_path) {
        Some(content) => {
            let mime = mime_for_path(&asset_path);
            ([(header::CONTENT_TYPE, mime)], content.data.clone()).into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

fn mime_for_path(path: &str) -> &'static str {
    if path.ends_with(".js") {
        "application/javascript"
    } else if path.ends_with(".css") {
        "text/css"
    } else if path.ends_with(".html") {
        "text/html"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    }
}

fn auth_session_present(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(auth) = state.auth_service() else {
        return false;
    };
    let Some(session_id) = session_cookie(headers) else {
        return false;
    };
    auth.authenticate_session(session_id)
        .ok()
        .flatten()
        .is_some()
}

fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| {
            raw.split(';').find_map(|part| {
                let (name, value) = part.trim().split_once('=')?;
                (name == SESSION_COOKIE_NAME).then_some(value)
            })
        })
}
