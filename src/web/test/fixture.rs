//! the simulated fleet, read through the site as the web app reads it

use axum::http::StatusCode;
use serde_json::Value;

use super::{Events, get, json, seek, simulated};
use crate::web::fixture::{SETTLED, START};

const MINUTE: i64 = 60_000;

/// 15:42 BRT on 2026-11-25 plus `hours:minutes:seconds`, in epoch ms
fn brt(hours: i64, minutes: i64, seconds: i64) -> i64 {
    START + ((hours - 15) * 3600 + (minutes - 42) * 60 + seconds) * 1000
}

/// seconds after the start at which the fleet is all clear
fn settled() -> f64 {
    ((SETTLED - START) / 1000 + 60) as f64
}

fn named<'a>(list: &'a Value, name: &str) -> &'a Value {
    list.as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("no {name}"))
}

fn rules(alerts: &Value, state: &str) -> Vec<String> {
    alerts
        .as_array()
        .unwrap()
        .iter()
        .filter(|alert| alert["state"]["kind"] == state)
        .map(|alert| alert["rule"].as_str().unwrap().to_owned())
        .collect()
}

async fn fleet(router: &axum::Router) -> Value {
    let mut stream = Events::open(router, "/api/stream").await;
    let message = stream.next().await;
    assert_eq!(message.event, "snapshot");
    message.data
}

/// who fails `target` of `service`, by node
fn failing(snapshot: &Value, service: &str, target: &str) -> Vec<String> {
    let probes = &named(&snapshot["services"], service)["probes"];
    let probe = probes
        .as_array()
        .unwrap()
        .iter()
        .find(|probe| probe["target"] == target)
        .unwrap();
    probe["views"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|view| view["ok"] == false)
        .map(|view| view["node"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn the_timeline_starts_with_the_prototypes_fleet() {
    let router = simulated();
    let snapshot = fleet(&router).await;

    assert_eq!(snapshot["at"], START);
    assert_eq!(
        rules(&snapshot["alerts"], "firing"),
        ["UnitFailed", "CertExpiring"]
    );
    assert_eq!(rules(&snapshot["alerts"], "resolved"), ["DiskAlmostFull"]);
    let severities: Vec<&Value> = snapshot["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|alert| &alert["severity"])
        .collect();
    assert_eq!(severities, ["page", "info", "page"]);

    let oracle = named(&snapshot["machines"], "oracle-1");
    assert_eq!(
        oracle["state"]["units"]["failed"][0]["name"],
        "lavalink.service"
    );
    assert_eq!(
        named(&snapshot["services"], "lavalink")["unit"]["state"],
        "failed"
    );
    for desktop in ["laptop-mac", "desktop-nixos"] {
        let machine = named(&snapshot["machines"], desktop);
        assert_eq!(machine["state"]["kind"], "resting", "{desktop}");
        assert_eq!(machine["always_on"], false);
    }
    for server in ["altar", "lab", "oracle-1", "oracle-2"] {
        assert_eq!(named(&snapshot["machines"], server)["state"]["kind"], "up");
    }
    assert_eq!(
        failing(&snapshot, "portfolio", "https://r3dlust.com"),
        ["lab"]
    );
    assert_eq!(
        failing(&snapshot, "portfolio", "https://blog.r3dlust.com"),
        Vec::<String>::new()
    );
}

#[tokio::test]
async fn the_fleet_settles_all_clear() {
    let router = simulated();
    seek(&router, settled()).await;
    let snapshot = fleet(&router).await;

    assert_eq!(rules(&snapshot["alerts"], "firing"), Vec::<String>::new());
    assert_eq!(
        rules(&snapshot["alerts"], "resolved"),
        ["CertExpiring", "UnitFailed", "DiskAlmostFull"]
    );
    for service in snapshot["services"].as_array().unwrap() {
        for probe in service["probes"].as_array().unwrap() {
            for view in probe["views"].as_array().unwrap() {
                assert_eq!(
                    view["ok"], true,
                    "{} from {}",
                    probe["target"], view["node"]
                );
            }
        }
        if !service["unit"].is_null() {
            assert_eq!(service["unit"]["state"], "active", "{}", service["name"]);
        }
    }
    assert_eq!(
        named(&snapshot["machines"], "laptop-mac")["state"]["kind"],
        "up"
    );
    assert_eq!(
        named(&snapshot["machines"], "desktop-nixos")["state"]["kind"],
        "resting"
    );
}

/// everything a browser could ask at one moment
async fn everything(router: &axum::Router) -> Vec<Value> {
    let alert = fleet(router).await["alerts"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    vec![
        fleet(router).await,
        json(
            router,
            "/api/series?of=machine:oracle-1&metric=cpu&range=6h",
        )
        .await,
        json(
            router,
            "/api/series?of=service:mail&metric=latency&range=24h",
        )
        .await,
        json(router, "/api/logs?machine=lab&limit=200").await,
        json(router, "/api/alerts?range=7d").await,
        json(router, &format!("/api/alerts/{alert}")).await,
    ]
}

#[tokio::test]
async fn a_moment_looks_the_same_however_the_clock_got_there() {
    let direct = simulated();
    seek(&direct, 900.0).await;
    let roundabout = simulated();
    seek(&roundabout, 2_000.0).await;
    seek(&roundabout, 300.0).await;
    seek(&roundabout, 900.0).await;

    assert_eq!(everything(&direct).await, everything(&roundabout).await);
}

#[tokio::test]
async fn a_seek_reaches_an_open_stream() {
    let router = simulated();
    let mut stream = Events::open(&router, "/api/stream").await;
    assert_eq!(stream.next().await.data["at"], START);

    seek(&router, 600.0).await;
    let moved = stream.next().await;
    assert_eq!(moved.event, "snapshot");
    assert_eq!(moved.data["at"], START + 10 * MINUTE);
}

#[tokio::test]
async fn a_machine_series_spans_its_range_up_to_now() {
    let router = simulated();
    let series = json(
        &router,
        "/api/series?of=machine:oracle-1&metric=cpu&range=1h",
    )
    .await;

    assert_eq!(series["data"]["kind"], "lines");
    let points = series["data"]["lines"][0]["points"].as_array().unwrap();
    assert_eq!(points.len(), 240);
    assert_eq!(points.last().unwrap()[0], START);
    assert_eq!(points[0][0], START - 60 * MINUTE + 15_000);
    assert!(
        points
            .iter()
            .all(|point| (0.0..=1.0).contains(&point[1].as_f64().unwrap()))
    );

    let net = json(&router, "/api/series?of=machine:lab&metric=net&range=7d").await;
    let names: Vec<&Value> = net["data"]["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| &line["name"])
        .collect();
    assert_eq!(names, ["rx", "tx"]);
}

#[tokio::test]
async fn a_resting_machine_has_no_points() {
    let router = simulated();
    let series = json(
        &router,
        "/api/series?of=machine:laptop-mac&metric=mem&range=6h",
    )
    .await;
    let points = series["data"]["lines"][0]["points"].as_array().unwrap();
    let asleep = brt(13, 5, 0);

    for point in points {
        let at = point[0].as_i64().unwrap();
        assert_eq!(point[1].is_null(), at >= asleep, "at {at}");
    }
}

#[tokio::test]
async fn a_latency_band_leaves_out_the_node_that_cannot_reach() {
    let router = simulated();
    let from_lab = json(
        &router,
        "/api/series?of=service:portfolio&metric=latency&range=1h&node=lab",
    )
    .await;
    let band = json(
        &router,
        "/api/series?of=service:portfolio&metric=latency&range=1h",
    )
    .await;
    let lost = brt(15, 21, 2);

    let lab = from_lab["data"]["lines"][0]["points"].as_array().unwrap();
    assert_eq!(from_lab["data"]["lines"][0]["name"], "lab");
    for point in lab {
        let at = point[0].as_i64().unwrap();
        assert_eq!(point[1].is_null(), at >= lost, "at {at}");
    }
    assert_eq!(band["data"]["kind"], "band");
    for point in band["data"]["points"].as_array().unwrap() {
        let (low, mid, high) = (point[1].as_f64(), point[2].as_f64(), point[3].as_f64());
        assert!(low <= mid && mid <= high && low.is_some(), "{point}");
    }
}

#[tokio::test]
async fn a_series_that_cannot_exist_is_refused() {
    let router = simulated();
    for (path, status) in [
        (
            "/api/series?of=machine:lab&metric=latency&range=1h",
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/series?of=service:mail&metric=cpu&range=1h",
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/series?of=service:lavalink&metric=latency&range=1h",
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/series?of=lab&metric=cpu&range=1h",
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/series?of=machine:lab&metric=cpu",
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/series?of=machine:nowhere&metric=cpu&range=1h",
            StatusCode::NOT_FOUND,
        ),
        (
            "/api/series?of=service:mail&metric=latency&range=1h&node=laptop-mac",
            StatusCode::NOT_FOUND,
        ),
    ] {
        assert_eq!(get(&router, path).await.0, status, "{path}");
    }
}

fn texts(page: &Value) -> Vec<String> {
    page["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| format!("{} {} {}", line["at"], line["machine"], line["text"]))
        .collect()
}

#[tokio::test]
async fn pages_of_logs_follow_on_without_gaps() {
    let router = simulated();
    let whole = json(&router, "/api/logs?limit=120").await;
    let first = json(&router, "/api/logs?limit=60").await;
    let before = first["before"].as_str().unwrap();
    let second = json(&router, &format!("/api/logs?limit=60&before={before}")).await;

    let paged: Vec<String> = texts(&first).into_iter().chain(texts(&second)).collect();
    assert_eq!(paged, texts(&whole));
    let times: Vec<i64> = whole["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line["at"].as_i64().unwrap())
        .collect();
    assert!(
        times.windows(2).all(|pair| pair[0] >= pair[1]),
        "newest first"
    );
    assert!(times[0] <= START);
}

#[tokio::test]
async fn logs_narrow_to_what_is_asked() {
    let router = simulated();
    let oom = json(&router, "/api/logs?machine=oracle-1&q=outofmemoryerror").await;
    assert_eq!(oom["lines"].as_array().unwrap().len(), 5);
    assert!(oom["before"].is_null());
    for line in oom["lines"].as_array().unwrap() {
        assert_eq!(line["level"], "error");
        assert_eq!(line["unit"], "lavalink.service");
    }

    // systemd's lines about the unit count as the unit's
    let lavalink = json(&router, "/api/logs?service=lavalink&limit=1000").await;
    let units: std::collections::BTreeSet<&str> = lavalink["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line["unit"].as_str().unwrap())
        .collect();
    assert_eq!(units, ["lavalink.service", "systemd"].into());

    // a service with no unit is the lines that name its hosts
    let portfolio = json(
        &router,
        "/api/logs?service=portfolio&machine=oracle-2&limit=5",
    )
    .await;
    for line in portfolio["lines"].as_array().unwrap() {
        assert!(
            line["text"].as_str().unwrap().contains("r3dlust.com"),
            "{line}"
        );
    }

    assert_eq!(
        get(&router, "/api/logs?machine=nowhere").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&router, "/api/logs?service=mail&unit=postfix.service")
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get(&router, "/api/logs?service=mail&machine=lab").await.0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn the_tail_sends_the_lines_the_clock_passes() {
    let router = simulated();
    let mut tail = Events::open(&router, "/api/logs/tail?machine=lab").await;
    seek(&router, 120.0).await;

    let message = tail.next().await;
    assert_eq!(message.event, "lines");
    let tailed: Vec<String> = texts(&message.data).into_iter().rev().collect();

    let logs = json(&router, "/api/logs?machine=lab&limit=1000").await;
    let arrived: Vec<String> = logs["lines"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|line| line["at"].as_i64().unwrap() > START)
        .map(|line| format!("{} {} {}", line["at"], line["machine"], line["text"]))
        .collect();
    assert!(!arrived.is_empty());
    assert_eq!(tailed, arrived);
}

#[tokio::test]
async fn alerts_list_what_fired_within_the_range() {
    let router = simulated();
    let listed = |alerts: Value| -> Vec<String> {
        alerts
            .as_array()
            .unwrap()
            .iter()
            .map(|alert| alert["rule"].as_str().unwrap().to_owned())
            .collect()
    };

    assert_eq!(
        listed(json(&router, "/api/alerts").await),
        ["UnitFailed", "DiskAlmostFull", "CertExpiring"]
    );
    assert_eq!(
        listed(json(&router, "/api/alerts?range=1h").await),
        ["UnitFailed", "CertExpiring"]
    );
    assert_eq!(
        listed(json(&router, "/api/alerts?range=7d").await),
        [
            "UnitFailed",
            "DiskAlmostFull",
            "NodeUnhealthy",
            "CertExpiring"
        ]
    );
}

fn kinds(detail: &Value) -> Vec<String> {
    detail["timeline"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["kind"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn an_alert_shows_its_timeline_and_its_series() {
    let router = simulated();
    let id = json(&router, "/api/alerts").await[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let path = format!("/api/alerts/{id}");

    let firing = json(&router, &path).await;
    assert_eq!(firing["alert"]["rule"], "UnitFailed");
    assert_eq!(firing["alert"]["seen_by"].as_array().unwrap().len(), 4);
    assert_eq!(
        kinds(&firing),
        [["pending"; 4].as_slice(), &["firing"; 4], &["notified"]].concat()
    );
    assert_eq!(firing["timeline"][8]["by"], "oracle-2");
    assert_eq!(firing["series"]["of"]["machine"], "oracle-1");
    assert_eq!(firing["series"]["metric"], "mem");
    assert_eq!(firing["series"]["to"], START);

    seek(&router, settled()).await;
    let resolved = json(&router, &path).await;
    assert_eq!(resolved["alert"]["state"]["kind"], "resolved");
    let tail = &kinds(&resolved)[9..];
    assert_eq!(tail, [["resolved"; 4].as_slice(), &["notified"]].concat());
    assert_eq!(resolved["timeline"][13]["resolved"], true);

    assert_eq!(
        get(&router, "/api/alerts/0000000000000000").await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn hello_answers_and_unknown_routes_do_not() {
    let router = simulated();
    let hello = json(&router, "/api/hello").await;
    assert_eq!(hello["schema"], 1);
    assert_eq!(get(&router, "/api/nothing").await.0, StatusCode::NOT_FOUND);
}
