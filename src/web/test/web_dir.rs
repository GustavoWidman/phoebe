//! the built web app beside the api

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::web::fixture::Fixture;

/// a build of one page, as the web app's build writes it: the page, and its
/// brotli copy beside it
fn build() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("phoebe-site-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<div id=\"app\"></div>").unwrap();
    std::fs::write(dir.join("index.html.br"), "brotli").unwrap();
    dir
}

async fn get(
    dir: &std::path::Path,
    path: &str,
    encoding: &str,
) -> (StatusCode, Option<String>, String) {
    let router = Fixture::new(0.0)
        .site(Some(dir.to_owned()), "http://127.0.0.1:7980", false)
        .unwrap();
    let request = Request::get(path)
        .header(header::ACCEPT_ENCODING, encoding)
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let sent = response
        .headers()
        .get(header::CONTENT_ENCODING)
        .map(|v| v.to_str().unwrap().to_owned());
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, sent, String::from_utf8_lossy(&body).into_owned())
}

#[tokio::test]
async fn every_screen_deep_links_to_the_page_compressed_once() {
    let dir = build();
    // a notification opens an alert's path, which only the app knows
    let (status, sent, body) = get(&dir, "/alerts/0dc6658a43bd5ed1", "br, gzip").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sent.as_deref(), Some("br"));
    assert_eq!(body, "brotli");

    let (_, sent, body) = get(&dir, "/", "identity").await;
    assert_eq!(sent, None);
    assert_eq!(body, "<div id=\"app\"></div>");

    let (status, _, _) = get(&dir, "/api/nothing", "br").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    std::fs::remove_dir_all(dir).unwrap();
}

/// the response to `path`, sent with `if_none_match` when given
async fn fetch(
    dir: &std::path::Path,
    path: &str,
    if_none_match: Option<&str>,
) -> axum::response::Response {
    let router = Fixture::new(0.0)
        .site(Some(dir.to_owned()), "http://127.0.0.1:7980", false)
        .unwrap();
    let mut request = Request::get(path);
    if let Some(etag) = if_none_match {
        request = request.header(header::IF_NONE_MATCH, etag);
    }
    router
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

fn header_of(response: &axum::response::Response, name: header::HeaderName) -> Option<String> {
    response
        .headers()
        .get(name)
        .map(|v| v.to_str().unwrap().to_owned())
}

#[tokio::test]
async fn a_new_service_worker_reaches_every_cache_whatever_its_size() {
    let dir = std::env::temp_dir().join(format!("phoebe-caching-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("_app/immutable")).unwrap();
    std::fs::write(dir.join("index.html"), "<div id=\"app\"></div>").unwrap();
    std::fs::write(dir.join("service-worker.js"), "const BUILD = 'aaaa';").unwrap();
    std::fs::write(dir.join("_app/immutable/index-abc.js"), "export {}").unwrap();

    let first = fetch(&dir, "/service-worker.js", None).await;
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(
        header_of(&first, header::CACHE_CONTROL).as_deref(),
        Some("no-cache")
    );
    assert_eq!(header_of(&first, header::LAST_MODIFIED), None);
    let etag = header_of(&first, header::ETAG).expect("an etag of the content");

    let unchanged = fetch(&dir, "/service-worker.js", Some(&etag)).await;
    assert_eq!(unchanged.status(), StatusCode::NOT_MODIFIED);

    // a new build of the same size: the nix store gives every file the same
    // mtime, so only the content tells the two apart
    std::fs::write(dir.join("service-worker.js"), "const BUILD = 'bbbb';").unwrap();
    let changed = fetch(&dir, "/service-worker.js", Some(&etag)).await;
    assert_eq!(changed.status(), StatusCode::OK);
    assert_ne!(header_of(&changed, header::ETAG), Some(etag));

    // a deep link is the page, so it carries the page's etag
    let page = fetch(&dir, "/", None).await;
    let deep = fetch(&dir, "/alerts/0dc6658a43bd5ed1", None).await;
    assert_eq!(
        header_of(&deep, header::ETAG),
        header_of(&page, header::ETAG)
    );
    assert_eq!(
        header_of(&deep, header::CACHE_CONTROL).as_deref(),
        Some("no-cache")
    );

    let hashed = fetch(&dir, "/_app/immutable/index-abc.js", None).await;
    assert_eq!(
        header_of(&hashed, header::CACHE_CONTROL).as_deref(),
        Some("public, max-age=31536000, immutable")
    );
    std::fs::remove_dir_all(dir).unwrap();
}
