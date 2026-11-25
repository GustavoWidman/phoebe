//! records what a real node's stores answer into `recorded/`, the answers
//! the other tests parse:
//!
//! ```text
//! PHOEBE_RECORD=/etc/phoebe/fleet.json cargo test record -- --ignored
//! ```
//!
//! the registry must name a node as its `machine`; that node's stores are
//! asked, as its phoebe would ask them

use std::path::Path;

use futures_util::StreamExt;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

use super::{RECORDED, Recorded};
use crate::web::live::{Live, alerts, loki::Direction, now, promql::Named, registry::Registry};

fn write(name: &str, body: &str) {
    let path = Path::new(RECORDED).join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

#[tokio::test]
#[ignore = "asks a real node; set PHOEBE_RECORD to its registry"]
async fn record() {
    let fleet = std::env::var("PHOEBE_RECORD").expect("PHOEBE_RECORD names a registry");
    let registry = Registry::load(Path::new(&fleet)).unwrap();
    write("fleet.json", &std::fs::read_to_string(&fleet).unwrap());
    let live = Live::new(registry);
    let registry = &live.registry;
    let now = now();
    let day = Recorded::DAY;

    for series in Named::SNAPSHOT {
        let promql = series.promql(registry, "", 60);
        let body = live.prometheus.query_body(&promql).await.unwrap();
        write(&format!("prometheus/{}.json", series.name()), &body);
    }

    // the alerts, as the snapshot asks for them
    let step = alerts::step_within(2 * day);
    let firing = Named::Firing.promql(registry, "", 60);
    let history = live
        .prometheus
        .range_body(&firing, now - 2 * day, now, step / 1_000)
        .await
        .unwrap();
    write("prometheus/history.json", &history);
    write(
        "alertmanager/alerts.json",
        &live.alertmanager.alerts_body().await.unwrap(),
    );
    let fired = Named::FiredWithin.promql(registry, "", (day / 1_000) as u64);
    for (node, prometheus) in &live.peers {
        write(
            &format!("nodes/{node}/alerts.json"),
            &prometheus.alerts_body().await.unwrap(),
        );
        write(
            &format!("nodes/{node}/fired.json"),
            &prometheus.query_body(&fired).await.unwrap(),
        );
    }

    // an alert's timeline, from this node's ALERTS of every state
    let alert = live
        .alertmanager
        .firing(now)
        .await
        .unwrap()
        .into_iter()
        .next()
        .expect("an alert firing to record the timeline of");
    let on: Vec<String> = alert
        .labels
        .iter()
        .map(|(name, value)| crate::web::live::promql::matcher(name, value))
        .collect();
    let timeline = live
        .prometheus
        .range_body(
            &Named::Alerts.promql(registry, &on.join(", "), 60),
            alert.starts_at.0 - 70 * 60_000,
            now,
            alerts::step_within(now - alert.starts_at.0 + 70 * 60_000) / 1_000,
        )
        .await
        .unwrap();
    write("prometheus/timeline.json", &timeline);

    // charts: a machine's cpu over an hour, a service's latency over six
    let hour = 3_600_000;
    let cpu = Named::Cpu.promql(registry, r#"machine="lab""#, 60);
    let cpu = live
        .prometheus
        .range_body(&cpu, now - hour, now, 15)
        .await
        .unwrap();
    write("charts/cpu.json", &cpu);
    let latency = Named::Latency.promql(registry, r#"service="mail""#, 90);
    let latency = live
        .prometheus
        .range_body(&latency, now - 6 * hour, now, 90)
        .await
        .unwrap();
    write("charts/latency.json", &latency);

    // logs: the stores' own lines, a nixos machine's from the journal and a
    // darwin one's from a file, which hold nothing private
    for (name, logql) in [
        ("journal", r#"{machine="lab", unit="loki.service"}"#),
        ("files", r#"{file="/var/log/coeusd.log"}"#),
    ] {
        let end = now * 1_000_000;
        let body = live
            .loki
            .range_body(logql, end - day * 1_000_000, end, 20, Direction::Backward)
            .await
            .unwrap();
        write(&format!("loki/{name}.json"), &body);
    }
    // a tail from a day back, which sends that day's lines first
    let (mut socket, _) = tokio_tungstenite::connect_async(live.loki.tail_url(
        r#"{machine="lab", unit="loki.service"}"#,
        (now - day) * 1_000_000,
    ))
    .await
    .unwrap();
    let message = loop {
        match socket.next().await.unwrap().unwrap() {
            Message::Text(text) => break text.to_string(),
            _ => continue,
        }
    };
    write("loki/tail.json", &message);

    write(
        "meta.json",
        &json!({ "now": now, "history_step": step }).to_string(),
    );
}
