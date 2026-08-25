use std::{
    borrow::Cow,
    collections::HashMap,
    io::Write,
    sync::{Mutex, OnceLock},
};

use axum::{
    body::{Body, Bytes},
    http::{
        header::{
            ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE, ETAG, IF_NONE_MATCH,
            VARY,
        },
        HeaderMap, Response, StatusCode,
    },
};
use flate2::{write::GzEncoder, Compression};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../web/dist"]
struct WebAssets;

/// 超过这个大小才值得压:小文件压完省不下几个字节,白烧 CPU。
const MIN_GZIP_BYTES: usize = 1024;

/// 面板前端资源响应。
///
/// 三件事都是奔着低配主机去的:
/// 1. **零拷贝**:release 下 rust-embed 给的是 `&'static [u8]`,此前每个请求都
///    `into_owned()` 克隆一份 —— 单个 vendor chunk 就 350KB,刷一次页面白白分配几 MB。
/// 2. **ETag / 304**:index.html 是 `no-cache`(每次都要回源问),带上 ETag 后
///    命中就只回一个空响应。
/// 3. **gzip**:全部资源裸传约 1.4MB,压完约 400KB。压缩结果按需惰性缓存(只缓存
///    真被请求过的资源),整站压满也就几百 KB 常驻。
pub fn static_response(path: &str, headers: &HeaderMap) -> Response<Body> {
    let normalized_path = normalize_path(path);
    match WebAssets::get(normalized_path) {
        Some(asset) => asset_response(normalized_path, asset, headers),
        None if should_fallback_to_spa(normalized_path) => WebAssets::get("index.html")
            .map(|asset| asset_response("index.html", asset, headers))
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

fn asset_response(
    path: &str,
    asset: rust_embed::EmbeddedFile,
    headers: &HeaderMap,
) -> Response<Body> {
    let etag = etag_for(&asset.metadata.sha256_hash());
    if request_matches_etag(headers, &etag) {
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(ETAG, &etag)
            .header(CACHE_CONTROL, cache_control(path))
            .body(Body::empty())
            .unwrap_or_else(|_| Response::new(Body::empty()));
    }

    let builder = Response::builder()
        .status(StatusCode::OK)
        .header(
            CONTENT_TYPE,
            mime_guess::from_path(path).first_or_octet_stream().as_ref(),
        )
        .header(CACHE_CONTROL, cache_control(path))
        .header(ETAG, &etag)
        .header(VARY, ACCEPT_ENCODING.as_str());

    let raw = to_bytes(asset.data);
    let builder = match gzipped(path, &raw, headers) {
        Some(compressed) => builder
            .header(CONTENT_ENCODING, "gzip")
            .body(Body::from(compressed)),
        None => builder.body(Body::from(raw)),
    };

    builder.unwrap_or_else(|_| Response::new(Body::empty()))
}

/// `Cow::Borrowed` 走 `Bytes::from_static`(零拷贝);只有 debug-embed 的
/// `Cow::Owned` 才会真的搬一次内存。
fn to_bytes(data: Cow<'static, [u8]>) -> Bytes {
    match data {
        Cow::Borrowed(slice) => Bytes::from_static(slice),
        Cow::Owned(vec) => Bytes::from(vec),
    }
}

fn etag_for(hash: &[u8; 32]) -> String {
    let mut out = String::with_capacity(2 + hash.len() * 2);
    out.push('"');
    for byte in hash.iter().take(16) {
        out.push_str(&format!("{byte:02x}"));
    }
    out.push('"');
    out
}

fn request_matches_etag(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get(IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .map(|candidate| candidate.trim().trim_start_matches("W/"))
                .any(|candidate| candidate == etag || candidate == "*")
        })
}

fn accepts_gzip(headers: &HeaderMap) -> bool {
    headers
        .get(ACCEPT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("gzip"))
}

/// 惰性 gzip 缓存:key 是资源路径,只缓存真被请求过的资源。
fn gzip_cache() -> &'static Mutex<HashMap<String, Bytes>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Bytes>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn gzipped(path: &str, raw: &Bytes, headers: &HeaderMap) -> Option<Bytes> {
    if raw.len() < MIN_GZIP_BYTES || !accepts_gzip(headers) {
        return None;
    }
    if let Ok(cache) = gzip_cache().lock() {
        if let Some(hit) = cache.get(path) {
            return Some(hit.clone());
        }
    }

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(raw).ok()?;
    let compressed = Bytes::from(encoder.finish().ok()?);
    // 压不动(已是压缩格式)就别缓存也别用,直接回原始字节。
    if compressed.len() >= raw.len() {
        return None;
    }
    if let Ok(mut cache) = gzip_cache().lock() {
        cache.insert(path.to_owned(), compressed.clone());
    }
    Some(compressed)
}

fn not_found_response() -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from("not found"))
        .unwrap_or_else(|_| Response::new(Body::empty()))
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

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).expect("header name"),
                value.parse().expect("header value"),
            );
        }
        headers
    }

    #[test]
    fn root_serves_index_html() {
        let response = static_response("/", &HeaderMap::new());

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).expect("content-type"),
            "text/html"
        );
    }

    #[test]
    fn spa_route_falls_back_to_index_html() {
        let response = static_response("/dashboard/overview", &HeaderMap::new());

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
        let response = static_response("/missing.js", &HeaderMap::new());

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn matching_etag_returns_304_without_body() {
        let first = static_response("/", &HeaderMap::new());
        let etag = first
            .headers()
            .get(ETAG)
            .expect("etag")
            .to_str()
            .expect("ascii")
            .to_owned();

        let second = static_response("/", &headers(&[("if-none-match", &etag)]));

        assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
        assert!(second.headers().get(CONTENT_ENCODING).is_none());
    }

    #[test]
    fn stale_etag_still_serves_body() {
        let response = static_response("/", &headers(&[("if-none-match", "\"deadbeef\"")]));

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn gzip_only_when_client_accepts_it() {
        let plain = static_response("/", &HeaderMap::new());
        assert!(plain.headers().get(CONTENT_ENCODING).is_none());

        let compressed = static_response("/", &headers(&[("accept-encoding", "gzip, deflate")]));
        // index.html 可能小于压缩阈值,压了就必须标 Content-Encoding。
        if compressed.headers().get(CONTENT_ENCODING).is_some() {
            assert_eq!(
                compressed
                    .headers()
                    .get(CONTENT_ENCODING)
                    .expect("encoding"),
                "gzip"
            );
        }
        assert_eq!(
            compressed.headers().get(VARY).expect("vary"),
            "accept-encoding"
        );
    }

    #[test]
    fn gzip_compresses_and_caches_large_assets() {
        let raw = Bytes::from(vec![b'a'; 64 * 1024]);
        let accept = headers(&[("accept-encoding", "gzip")]);

        let first = gzipped("test-asset.js", &raw, &accept).expect("compressed");
        assert!(first.len() < raw.len());
        // 第二次必须命中缓存,拿到同一份字节。
        let second = gzipped("test-asset.js", &raw, &accept).expect("cached");
        assert_eq!(first, second);

        assert!(gzipped("test-asset.js", &raw, &HeaderMap::new()).is_none());
        assert!(gzipped("tiny.js", &Bytes::from_static(b"tiny"), &accept).is_none());
    }
}
