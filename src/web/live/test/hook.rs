//! the hook end to end: a notice posted as Alertmanager posts it, pushed to
//! a push service on loopback that decrypts what it gets as a browser would

use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, Request, StatusCode},
    routing::post,
};
use gjallar::{Receiver, VapidKey};
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::web::live::hook::{self, PushSettings};

const ORIGIN: &str = "https://phoebe.example.com";

/// what the push service got: each request's headers and decrypted payload
type Pushed = Arc<Mutex<Vec<(HeaderMap, Value)>>>;

/// a push service on loopback for one browser, and the files that send to it
async fn push_service(name: &str) -> (PushSettings, Pushed) {
    let receiver = Receiver::generate().unwrap();
    let pushed: Pushed = Arc::default();

    async fn take(
        State((receiver, pushed)): State<(Receiver, Pushed)>,
        headers: HeaderMap,
        body: Bytes,
    ) -> StatusCode {
        let payload = receiver.decrypt(&body).unwrap();
        let payload = serde_json::from_slice(&payload).unwrap();
        pushed.lock().unwrap().push((headers, payload));
        StatusCode::CREATED
    }
    let service = Router::new()
        .route("/push/phone", post(take))
        .with_state((receiver.clone(), pushed.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, service).await });

    let dir: PathBuf =
        std::env::temp_dir().join(format!("phoebe-hook-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let key = dir.join("vapid");
    std::fs::write(&key, VapidKey::generate().unwrap().to_base64url()).unwrap();
    let subscription = receiver
        .subscription(&format!("http://{address}/push/phone"))
        .unwrap();
    let mut entry = serde_json::to_value(&subscription).unwrap();
    entry["name"] = json!("phone");
    let subscriptions = dir.join("subscriptions.json");
    std::fs::write(&subscriptions, json!([entry]).to_string()).unwrap();

    let settings = PushSettings::new(ORIGIN.into(), key, subscriptions).unwrap();
    (settings, pushed)
}

/// a notice for lavalink's unit failing on oracle-1, as Alertmanager posts it
fn notice(severity: &str, status: &str) -> String {
    json!({
        "version": "4",
        "status": status,
        "receiver": "phoebe",
        "groupLabels": {"alertname": "UnitFailed"},
        "commonLabels": {},
        "commonAnnotations": {},
        "externalURL": "http://oracle-1:9093",
        "groupKey": "{}:{alertname=\"UnitFailed\"}",
        "truncatedAlerts": 0,
        "alerts": [{
            "status": status,
            "labels": {
                "alertname": "UnitFailed",
                "machine": "oracle-1",
                "name": "lavalink.service",
                "service": "lavalink",
                "severity": severity,
            },
            "annotations": {"summary": "lavalink failed on oracle-1"},
            "startsAt": "2026-11-25T18:42:00.000Z",
            "endsAt": if status == "firing" { "0001-01-01T00:00:00Z" } else { "2026-11-25T18:54:00Z" },
            "generatorURL": "http://oracle-1:9090/graph",
            "fingerprint": "5c1d0a5b3e8f2a71",
        }],
    })
    .to_string()
}

async fn post_notice(router: &Router, body: String) -> StatusCode {
    let request = Request::post("/hook")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    router.clone().oneshot(request).await.unwrap().status()
}

#[tokio::test]
async fn a_page_goes_out_firing_then_resolved_under_one_tag() {
    let (settings, pushed) = push_service("page").await;
    let router = hook::router(Some(settings));

    assert_eq!(
        post_notice(&router, notice("page", "firing")).await,
        StatusCode::OK
    );
    assert_eq!(
        post_notice(&router, notice("page", "resolved")).await,
        StatusCode::OK
    );

    let pushed = pushed.lock().unwrap();
    let [(firing, fired), (resolution, resolved)] = pushed.as_slice() else {
        panic!("{pushed:?}");
    };
    for (headers, payload) in [(firing, fired), (resolution, resolved)] {
        assert_eq!(headers["ttl"], "86400");
        assert_eq!(headers["content-encoding"], "aes128gcm");
        assert!(
            headers["authorization"]
                .to_str()
                .unwrap()
                .starts_with("vapid t=")
        );
        assert_eq!(payload["web_push"], 8030);
        let notification = &payload["notification"];
        assert_eq!(notification["title"], "lavalink failed on oracle-1");
        assert_eq!(
            notification["navigate"],
            format!("{ORIGIN}/alerts/5c1d0a5b3e8f2a71")
        );
        assert_eq!(notification["tag"], "5c1d0a5b3e8f2a71");
    }
    assert_eq!(firing["topic"], resolution["topic"]);
    assert_eq!(firing["urgency"], "high");
    assert_eq!(resolution["urgency"], "normal");
    assert_eq!(fired["notification"]["body"], "UnitFailed, firing");
    assert_eq!(
        resolved["notification"]["body"],
        "Resolved after 12 minutes"
    );

    // the service worker shows this very payload (web/src/lib/push/notice.test.ts)
    let sent = serde_json::to_string_pretty(fired).unwrap() + "\n";
    if std::env::var_os("PHOEBE_BLESS").is_some() {
        std::fs::write(PUSH, &sent).unwrap();
    }
    assert!(
        std::fs::read_to_string(PUSH).unwrap_or_default() == sent,
        "{PUSH} is not what the hook sends; rewrite it with `PHOEBE_BLESS=1 cargo test web::`"
    );
}

/// a page alert's push as the hook sends it, for the service worker's tests
const PUSH: &str = "contract/push.json";

#[tokio::test]
async fn an_info_alert_is_logged_and_never_pushed() {
    let (settings, pushed) = push_service("info").await;
    let router = hook::router(Some(settings));
    assert_eq!(
        post_notice(&router, notice("info", "firing")).await,
        StatusCode::OK
    );
    assert!(pushed.lock().unwrap().is_empty());
}

/// another node's phoebe sends it instead, once Alertmanager gives up here
#[tokio::test]
async fn a_page_with_no_key_to_sign_it_fails_the_notice() {
    let (mut settings, pushed) = push_service("keyless").await;
    settings.key_file = settings.key_file.with_file_name("missing");
    let router = hook::router(Some(settings));
    assert_eq!(
        post_notice(&router, notice("page", "firing")).await,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert!(pushed.lock().unwrap().is_empty());
}
