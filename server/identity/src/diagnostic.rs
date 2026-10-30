//! The diagnostic browser page. It is served only when a browser origin is
//! configured, from the same origin as the API so the session cookie applies.

use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

use super::AppState;

const HTML: &str = include_str!("../../../client/browser/index.html");
const SCRIPT: &str = include_str!("../../../client/browser/diagnostic.js");
const STYLE: &str = include_str!("../../../client/browser/diagnostic.css");

pub(super) async fn page(State(state): State<AppState>) -> Response {
    document(&state, "text/html; charset=utf-8", HTML)
}

pub(super) async fn script(State(state): State<AppState>) -> Response {
    document(&state, "text/javascript; charset=utf-8", SCRIPT)
}

pub(super) async fn style(State(state): State<AppState>) -> Response {
    document(&state, "text/css; charset=utf-8", STYLE)
}

fn document(state: &AppState, content_type: &'static str, body: &'static str) -> Response {
    if state.browser_origin.is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mut response = body.into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_page_does_not_store_a_credential() {
        for source in [super::HTML, super::SCRIPT] {
            assert!(!source.contains("localStorage"));
            assert!(!source.contains("sessionStorage"));
        }
    }
}
