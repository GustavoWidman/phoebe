use std::{
    collections::{BTreeSet, HashMap},
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Instant,
};

use axum::{
    Router,
    body::Body,
    extract::connect_info::MockConnectInfo,
    http::{Method, Request, StatusCode, header},
};
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD as B64};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

use super::session::Browser;
use crate::web::{
    session::{Key, Proof, Sessions},
    signin::{Caller, Signin},
    whois::{CAPABILITY, Peer, Whois},
    wire::{Ms, SignIn},
};

pub const ORIGIN: &str = "https://phoebe.example.com";
const PEER: &str = "100.73.0.1:51234";

pub fn phone() -> Peer {
    Peer {
        login: "someone@example.com".into(),
        device: "phone".into(),
        capabilities: BTreeSet::from([CAPABILITY.to_owned()]),
    }
}

fn signin(sessions: &Arc<Sessions>, whois: Whois) -> Router {
    Signin {
        sessions: sessions.clone(),
        whois,
        capability: CAPABILITY.into(),
        origin: ORIGIN.into(),
    }
    .router()
    .unwrap()
    .layer(MockConnectInfo(Caller(PEER.parse::<SocketAddr>().unwrap())))
}

pub fn sessions() -> Arc<Sessions> {
    Arc::new(Sessions::new(vec![Key::generate()]).unwrap())
}

/// what `browser` sends to sign in to `origin` at `ts`
pub fn request(browser: &Browser, origin: &str, ts: Ms) -> SignIn {
    let sig = browser.sign(&Proof::signin(origin, ts));
    SignIn {
        key: browser.jwk(),
        ts,
        sig: B64.encode(sig.to_bytes()),
    }
}

async fn post(router: &Router, request: &SignIn) -> (StatusCode, String) {
    let request = Request::post("/auth/session")
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(request).unwrap()))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn a_granted_device_signs_in_with_its_own_key() {
    let sessions = sessions();
    let router = signin(&sessions, Whois::Fixed(phone()));
    let browser = Browser::new();
    let now = Ms::now();

    let (status, answer) = post(&router, &request(&browser, ORIGIN, now)).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let answer: Value = serde_json::from_str(&answer).unwrap();
    assert_eq!(answer["viewer"]["login"], "someone@example.com");
    assert_eq!(answer["viewer"]["device"], "phone");

    let token = answer["token"].as_str().unwrap();
    let claims = sessions.check(token, now).unwrap();
    assert_eq!(claims.key, browser.public());
    assert_eq!(answer["viewer"]["expires_at"], claims.exp.0);
    assert!(claims.iat.0 >= now.0);
}

#[tokio::test]
async fn a_sign_in_must_be_signed_now_for_this_app_by_its_key() {
    let sessions = sessions();
    let router = signin(&sessions, Whois::Fixed(phone()));
    let browser = Browser::new();
    let now = Ms::now();

    let stale = request(&browser, ORIGIN, Ms(now.0 - 10 * 60 * 1000));
    let elsewhere = request(&browser, "https://evil.example.com", now);
    let borrowed = SignIn {
        key: Browser::new().jwk(),
        ..request(&browser, ORIGIN, now)
    };
    let mut keyless = request(&browser, ORIGIN, now);
    keyless.key.crv = "P-384".into();

    for request in [stale, elsewhere, borrowed, keyless] {
        let (status, why) = post(&router, &request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{why}");
    }
}

#[tokio::test]
async fn only_a_granted_tailnet_peer_signs_in() {
    let sessions = sessions();
    let browser = Browser::new();
    let now = Ms::now();
    let ungranted = Peer {
        capabilities: BTreeSet::from(["example.com/cap/other".to_owned()]),
        ..phone()
    };
    let unknown = Whois::Tailscale(Mutex::new(HashMap::from([(
        PEER.parse::<SocketAddr>().unwrap().ip(),
        (Instant::now(), None),
    )])));

    for (whois, expected, said) in [
        (
            Whois::Fixed(ungranted),
            StatusCode::FORBIDDEN,
            "the tailnet policy grants no access",
        ),
        (unknown, StatusCode::FORBIDDEN, "not a tailnet peer"),
    ] {
        let router = signin(&sessions, whois);
        let (status, why) = post(&router, &request(&browser, ORIGIN, now)).await;
        assert_eq!((status, why.as_str()), (expected, said));
    }
}

#[tokio::test]
async fn the_app_may_call_it_from_its_own_origin_alone() {
    let router = signin(&sessions(), Whois::Fixed(phone()));
    let preflight = |origin: &str| {
        Request::builder()
            .method(Method::OPTIONS)
            .uri("/auth/session")
            .header(header::ORIGIN, origin)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
            .header("access-control-request-private-network", "true")
            .body(Body::empty())
            .unwrap()
    };

    let response = router.clone().oneshot(preflight(ORIGIN)).await.unwrap();
    assert!(response.status().is_success());
    let headers = response.headers();
    assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], ORIGIN);
    assert_eq!(headers["access-control-allow-private-network"], "true");
    let methods = headers[header::ACCESS_CONTROL_ALLOW_METHODS]
        .to_str()
        .unwrap();
    assert!(
        methods.contains("POST") && methods.contains("GET"),
        "{methods}"
    );
    assert_eq!(
        headers[header::ACCESS_CONTROL_ALLOW_HEADERS],
        "content-type"
    );

    let response = router
        .clone()
        .oneshot(preflight("https://evil.example.com"))
        .await
        .unwrap();
    // a browser on another origin sees this one named and refuses
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        ORIGIN
    );

    let ping = Request::get("/auth/ping")
        .header(header::ORIGIN, ORIGIN)
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(ping).await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        ORIGIN
    );
}
