//! who reads the fleet: a browser that signed in, proving its session on
//! every request, and nobody else

use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use super::{Events, get, session::Browser};
use crate::web::{
    fixture::Fixture,
    push::Push,
    session::{Key, LIFETIME, Sessions},
    site::{self, Access, Settings},
    source::Source,
    wire::Ms,
};

const SIGNIN: &str = "https://lab.tail0000.ts.net:45731";

/// a browser signed in to a site over the simulated fleet
pub struct Reader {
    pub router: Router,
    pub sessions: Arc<Sessions>,
    pub browser: Browser,
    pub token: String,
}

impl Reader {
    /// signed in now, its session a week long
    pub fn signed_in(push: Option<Push>) -> Self {
        Self::signed_in_at(Ms::now(), push)
    }

    /// signed in at `at`, so its session ends a week after
    pub fn signed_in_at(at: Ms, push: Option<Push>) -> Self {
        let sessions = Arc::new(Sessions::new(vec![Key::generate()]).unwrap());
        let router = site::router(
            Source::Fixture(Fixture::new(0.0)),
            Settings {
                web_dir: None,
                access: Access::Sessions {
                    sessions: sessions.clone(),
                    signin: vec![SIGNIN.into()],
                },
                push,
            },
        );
        let browser = Browser::new();
        let token = sessions
            .issue(
                "someone@example.com".into(),
                "phone".into(),
                browser.public(),
                at,
            )
            .token;
        Self {
            router,
            sessions,
            browser,
            token,
        }
    }

    pub fn request(&self, path: &str) -> Request<Body> {
        let mut request = Request::get(path).body(Body::empty()).unwrap();
        *request.headers_mut() = self
            .browser
            .headers(&Method::GET, path, &self.token, Ms::now());
        request
    }

    /// what `path` answers to this browser
    pub async fn get(&self, path: &str) -> (StatusCode, Value) {
        let response = self
            .router
            .clone()
            .oneshot(self.request(path))
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (status, body)
    }
}

const SIGNED: [&str; 7] = [
    "/api/stream",
    "/api/series?of=machine:lab&metric=cpu&range=1h",
    "/api/logs?machine=lab",
    "/api/logs/tail?machine=lab",
    "/api/alerts",
    "/api/push",
    "/api/nothing",
];

#[tokio::test]
async fn hello_answers_anyone_with_where_to_sign_in() {
    let reader = Reader::signed_in(None);
    let (status, hello) = get(&reader.router, "/api/hello").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hello["signin"], json!([SIGNIN]));
}

#[tokio::test]
async fn everything_else_wants_a_session_and_says_nothing_without_one() {
    let reader = Reader::signed_in(Some(Push::Fixed(Fixture::push())));
    for path in SIGNED {
        assert_eq!(
            get(&reader.router, path).await,
            (StatusCode::UNAUTHORIZED, json!("")),
            "{path}"
        );
    }
}

#[tokio::test]
async fn a_signed_in_browser_reads_every_route_its_proof_names() {
    let reader = Reader::signed_in(Some(Push::Fixed(Fixture::push())));
    let (status, alerts) = reader.get("/api/alerts").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!alerts.as_array().unwrap().is_empty());
    assert_eq!(reader.get("/api/push").await.0, StatusCode::OK);
    assert_eq!(reader.get("/api/nothing").await.0, StatusCode::NOT_FOUND);

    // a proof over one request lets in no other
    let mut request = reader.request("/api/alerts");
    *request.uri_mut() = "/api/alerts?range=7d".parse().unwrap();
    let response = reader.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // nor does the same session under a key rotated away
    let rotated = Sessions::new(vec![Key::generate()]).unwrap();
    assert!(rotated.check(&reader.token, Ms::now()).is_err());
    assert!(reader.sessions.check(&reader.token, Ms::now()).is_ok());
}

#[tokio::test]
async fn a_stream_ends_with_its_session() {
    // the held clock brings the tail no lines, so it says only that it ended
    for (path, first) in [
        ("/api/stream", "snapshot"),
        ("/api/logs/tail?machine=lab", "expired"),
    ] {
        // a session that ends a second from now
        let at = Ms(Ms::now().0 - LIFETIME.as_millis() as i64 + 1_000);
        let reader = Reader::signed_in_at(at, None);
        let response = reader
            .router
            .clone()
            .oneshot(reader.request(path))
            .await
            .unwrap();
        let mut events = Events::from(response);
        let mut kinds = vec![events.next().await.event];
        while kinds.last().map(String::as_str) != Some("expired") {
            kinds.push(events.next().await.event);
        }
        assert_eq!(kinds.first().map(String::as_str), Some(first), "{path}");
        assert!(events.ended().await, "{path} closes after it says so");
    }
}

#[tokio::test]
async fn a_session_past_its_end_is_refused() {
    let at = Ms(Ms::now().0 - LIFETIME.as_millis() as i64 - 1);
    let reader = Reader::signed_in_at(at, None);
    assert_eq!(reader.get("/api/stream").await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_simulated_fleet_is_open_to_anyone() {
    let router = Fixture::new(0.0)
        .site(None, "http://127.0.0.1:7980", false)
        .unwrap();
    let (status, hello) = get(&router, "/api/hello").await;
    assert_eq!((status, &hello["signin"]), (StatusCode::OK, &json!([])));
    assert_eq!(get(&router, "/api/alerts").await.0, StatusCode::OK);

    let signing_in = Fixture::new(0.0)
        .site(None, "http://127.0.0.1:7980", true)
        .unwrap();
    let (_, hello) = get(&signing_in, "/api/hello").await;
    assert_eq!(hello["signin"], json!(["http://127.0.0.1:7980"]));
    assert_eq!(
        get(&signing_in, "/api/alerts").await.0,
        StatusCode::UNAUTHORIZED
    );
}
