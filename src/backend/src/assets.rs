use axum::{
    body::Body,
    http::{
        header::{CACHE_CONTROL, CONTENT_TYPE},
        Response, StatusCode,
    },
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../web/dist"]
struct WebAssets;

pub fn static_response(path: &str) -> Response<Body> {
    let normalized_path = normalize_path(path);
    match WebAssets::get(normalized_path) {
        Some(asset) => asset_response(normalized_path, asset.data.into_owned()),
        None if should_fallback_to_spa(normalized_path) => WebAssets::get("index.html")
            .map(|asset| asset_response("index.html", asset.data.into_owned()))
            .unwrap_or_else(not_found_response),
        None => not_found_response(),
    }
}

fn normalize_path(path: &str) -> &str {
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        "index.html"
    } else {
        path
    }
}

fn should_fallback_to_spa(path: &str) -> bool {
    !path.contains('.')
}

fn asset_response(path: &str, bytes: Vec<u8>) -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header(
            CONTENT_TYPE,
            mime_guess::from_path(path).first_or_octet_stream().as_ref(),
        )
        .header(CACHE_CONTROL, cache_control(path))
        .body(Body::from(bytes))
        .expect("static asset response must be valid")
}

fn not_found_response() -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from("not found"))
        .expect("static not found response must be valid")
}

fn cache_control(path: &str) -> &'static str {
    if path == "index.html" {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_serves_index_html() {
        let response = static_response("/");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).expect("content-type"),
            "text/html"
        );
    }

    #[test]
    fn spa_route_falls_back_to_index_html() {
        let response = static_response("/dashboard/overview");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(CACHE_CONTROL)
                .expect("cache-control"),
            "no-cache"
        );
    }

    #[test]
    fn missing_asset_with_extension_returns_not_found() {
        let response = static_response("/missing.js");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
