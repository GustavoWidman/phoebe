use axum::http::StatusCode;
use gjallar::{Receiver, VapidKey};
use serde_json::{Value, json};

use super::{access::Reader, get};
use crate::web::{
    fixture::{Fixture, PUSH_KEY, PUSH_SUBSCRIBED},
    live::hook::PushSettings,
    push::{Push, endpoint_hash},
};

const EDIT: &str = "agenix -e secrets/phoebe-push-subscriptions.age";

fn live(dir: &std::path::Path, list: &str) -> (Push, String) {
    let key = VapidKey::generate().unwrap();
    std::fs::write(dir.join("vapid"), key.to_base64url()).unwrap();
    let subscriptions = dir.join(list);
    let settings = PushSettings::new(
        "https://phoebe.example.com".into(),
        dir.join("vapid"),
        subscriptions,
    )
    .unwrap();
    let push = Push::Live {
        settings,
        edit: Some(EDIT.into()),
    };
    (push, key.public_key())
}

fn entry(name: &str, endpoint: &str) -> Value {
    let subscription = Receiver::generate()
        .unwrap()
        .subscription(endpoint)
        .unwrap();
    let mut entry = serde_json::to_value(&subscription).unwrap();
    entry["name"] = name.into();
    entry
}

#[test]
fn an_endpoint_is_named_by_the_start_of_its_hash() {
    // the app hashes its own endpoint the same way (web/src/lib/push/state.test.ts)
    assert_eq!(endpoint_hash(PUSH_SUBSCRIBED), "4029f3734d796cf1");
    assert_eq!(
        endpoint_hash("https://fcm.googleapis.com/fcm/send/abc"),
        "4e9bdab8bbe7189c"
    );
}

#[tokio::test]
async fn the_app_gets_the_public_key_and_the_hashes_of_the_list_read_each_time() {
    let dir = tempfile::tempdir().unwrap();
    let list = dir.path().join("subscriptions.json");
    let iphone = "https://web.push.apple.com/QGxiphone";
    let mac = "https://web.push.apple.com/QGxmac";
    std::fs::write(&list, json!([entry("iphone", iphone)]).to_string()).unwrap();
    let (push, public) = live(dir.path(), "subscriptions.json");

    let view = push.view().await.unwrap();
    assert_eq!(view.key, public);
    assert_eq!(view.subscribed, [endpoint_hash(iphone)]);

    // an edit shows without a restart
    std::fs::write(
        &list,
        json!([entry("iphone", iphone), entry("mac", mac)]).to_string(),
    )
    .unwrap();
    let reader = Reader::signed_in(Some(push));
    let (status, answer) = reader.get("/api/push").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        answer,
        json!({
            "key": public,
            "subscribed": [endpoint_hash(iphone), endpoint_hash(mac)],
            "edit": EDIT,
        })
    );
    let text = answer.to_string();
    assert!(!text.contains("QGx"), "no endpoint leaves the server");
    assert!(!text.contains("p256dh"), "no key leaves the server");
}

#[tokio::test]
async fn without_push_the_app_is_told_there_is_none() {
    let (status, answer) = Reader::signed_in(None).get("/api/push").await;
    assert_eq!((status, answer), (StatusCode::NOT_FOUND, json!("")));
}

#[tokio::test]
async fn a_list_that_cannot_be_read_is_an_error_not_an_empty_list() {
    let dir = tempfile::tempdir().unwrap();
    let (missing, _) = live(dir.path(), "missing.json");
    let (status, answer) = Reader::signed_in(Some(missing)).get("/api/push").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        !answer.to_string().contains(&*dir.path().to_string_lossy()),
        "{answer}"
    );

    std::fs::write(dir.path().join("broken.json"), "[{\"name\":\"iphone\"}]").unwrap();
    let (broken, _) = live(dir.path(), "broken.json");
    let (status, _) = Reader::signed_in(Some(broken)).get("/api/push").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn the_simulated_fleet_sends_to_one_known_endpoint() {
    let router = Fixture::new(0.0)
        .site(None, "http://127.0.0.1:7980", false)
        .unwrap();
    let (status, answer) = get(&router, "/api/push").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(answer["key"], PUSH_KEY);
    assert_eq!(
        answer["subscribed"],
        json!([endpoint_hash(PUSH_SUBSCRIBED)])
    );
}
