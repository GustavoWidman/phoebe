//! the built web app beside the api

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::web::{fixture::Fixture, site, source::Source};

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
    let router = site::router(Source::Fixture(Fixture::new(0.0)), Some(dir));
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
