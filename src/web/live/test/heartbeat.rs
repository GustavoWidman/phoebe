//! a service that reports a heartbeat, with a unit and probes or with
//! nothing else, from the registry the module writes to what the snapshot
//! says of it

use serde_json::{Value, json};

use super::Recorded;
use crate::web::{
    live::{prometheus::Sample, promql::Named, registry::Registry, snapshot},
    wire::*,
};

/// the recorded fleet with heartbeats: on mail, which has a unit and
/// probes, and on three services that have nothing else
fn fleet() -> Value {
    let mut fleet: Value = serde_json::from_str(&Recorded::file("fleet.json")).unwrap();
    let services = &mut fleet["services"];
    services["mail"]["heartbeat"] =
        json!({"metric": "mail_queue_healthy_timestamp_seconds", "within": 600});
    for (name, machine) in [
        ("backup", "lab"),
        ("ghost", "altar"),
        ("report", "desktop-nixos"),
    ] {
        services[name] = json!({
            "machine": machine,
            "unit": null,
            "probes": [],
            "metrics": null,
            "heartbeat": {"metric": format!("{name}_healthy_timestamp_seconds"), "within": 3600},
        });
    }
    fleet
}

fn registry() -> Registry {
    Registry::parse(fleet().to_string().as_bytes()).unwrap()
}

#[test]
fn a_service_with_nothing_but_a_heartbeat_is_in_the_registry() {
    let registry = registry();
    let backup = &registry.services[&ServiceName::from("backup")];
    assert!(backup.unit.is_none() && backup.probes.is_empty());
    let heartbeat = backup.heartbeat.as_ref().unwrap();
    assert_eq!(
        (heartbeat.metric.as_str(), heartbeat.within),
        ("backup_healthy_timestamp_seconds", 3600)
    );
    // a registry written before heartbeats has none
    assert!(
        registry.services[&ServiceName::from("adguard")]
            .heartbeat
            .is_none()
    );
}

#[test]
fn a_heartbeat_that_is_no_metric_name_is_refused() {
    let mut fleet = fleet();
    fleet["services"]["backup"]["heartbeat"]["metric"] = json!("backup} or vector(1)");
    let error = Registry::parse(fleet.to_string().as_bytes()).unwrap_err();
    assert!(
        format!("{error:#}").contains("is not a metric name"),
        "{error:#}"
    );
}

#[test]
fn a_heartbeat_is_fresh_late_missing_or_unknown() {
    let recorded = Recorded::load();
    let registry = registry();
    let now = recorded.now;
    let mut answers = recorded.answers();
    let gauge = |service: &str, machine: &str, ago_ms: i64| Sample {
        labels: [("service", service), ("machine", machine)]
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .into(),
        value: (now - ago_ms) as f64 / 1_000.0,
    };
    answers.series.insert(
        Named::Heartbeat,
        vec![
            gauge("backup", "lab", 10 * 60_000),
            gauge("mail", "oracle-2", 20 * 60_000),
        ],
    );
    // desktop-nixos is off, so its gauges are unknown, not missing
    for sample in answers.series.get_mut(&Named::Up).unwrap() {
        if sample.labels["machine"] == "desktop-nixos" {
            sample.value = 0.0;
        }
    }

    let snapshot = snapshot::build(&registry, &answers, now, Vec::new());
    let heartbeat = |name: &str| {
        let service = snapshot
            .services
            .iter()
            .find(|service| service.name.0 == name)
            .unwrap();
        service.heartbeat.clone().unwrap()
    };
    assert_eq!(
        heartbeat("backup"),
        Heartbeat {
            metric: "backup_healthy_timestamp_seconds".into(),
            within_ms: 3_600_000,
            state: HeartbeatState::Fresh,
            healthy_at: Some(Ms(now - 10 * 60_000)),
        }
    );
    assert_eq!(heartbeat("mail").state, HeartbeatState::Late);
    assert_eq!(heartbeat("ghost").state, HeartbeatState::Missing);
    assert_eq!(heartbeat("ghost").healthy_at, None);
    assert_eq!(heartbeat("report").state, HeartbeatState::Unknown);

    // mail keeps its unit and probes beside it
    let mail = snapshot
        .services
        .iter()
        .find(|service| service.name.0 == "mail")
        .unwrap();
    assert!(mail.unit.is_some() && mail.probes.len() == 3);

    // a prometheus that does not answer leaves every heartbeat unknown
    answers.series.remove(&Named::Heartbeat);
    let snapshot = snapshot::build(&registry, &answers, now, Vec::new());
    assert!(
        snapshot
            .services
            .iter()
            .filter_map(|service| service.heartbeat.as_ref())
            .all(|heartbeat| heartbeat.state == HeartbeatState::Unknown)
    );
}
