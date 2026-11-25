mod contract;
mod fixture;
mod typescript;
mod web_dir;

use std::time::Duration;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

use super::{fixture::Fixture, site, source::Source};

/// the simulated fleet's site, its clock held at the start
pub fn simulated() -> Router {
    let fixture = Fixture::new(0.0);
    site::router(Source::Fixture(fixture.clone()), None).merge(fixture.routes())
}

pub async fn get(router: &Router, path: &str) -> (StatusCode, Value) {
    let request = Request::get(path).body(Body::empty()).unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (status, body)
}

/// the JSON `path` answers, which must be a success
pub async fn json(router: &Router, path: &str) -> Value {
    let (status, body) = get(router, path).await;
    assert_eq!(status, StatusCode::OK, "{path}: {body}");
    body
}

/// moves the clock to `t` seconds after the timeline starts, and holds it
pub async fn seek(router: &Router, t: f64) {
    let request = Request::post(format!("/fixture/seek?t={t}"))
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

/// a server-sent event stream, as a browser reads it
pub struct Events {
    body: Body,
    buffer: String,
}

pub struct Message {
    pub event: String,
    pub data: Value,
}

impl Events {
    pub async fn open(router: &Router, path: &str) -> Self {
        let request = Request::get(path).body(Body::empty()).unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        Self {
            body: response.into_body(),
            buffer: String::new(),
        }
    }

    /// the next event, skipping keep-alive comments
    pub async fn next(&mut self) -> Message {
        loop {
            if let Some(end) = self.buffer.find("\n\n") {
                let block: String = self.buffer.drain(..end + 2).collect();
                let field = |name: &str| {
                    block
                        .lines()
                        .find_map(|line| line.strip_prefix(name))
                        .map(str::to_owned)
                };
                if let (Some(event), Some(data)) = (field("event: "), field("data: ")) {
                    return Message {
                        event,
                        data: serde_json::from_str(&data).unwrap(),
                    };
                }
                continue;
            }
            let frame = tokio::time::timeout(Duration::from_secs(10), self.body.frame())
                .await
                .expect("an event within 10 seconds")
                .expect("the stream stays open")
                .unwrap();
            if let Ok(data) = frame.into_data() {
                self.buffer.push_str(std::str::from_utf8(&data).unwrap());
            }
        }
    }
}
