//! a service is its machine and its name: two machines may run a service of
//! one name, one machine only one, and each reads its own series

use serde_json::{Value, json};

use super::Recorded;
use crate::web::live::{registry::Registry, snapshot};

/// the recorded fleet, and derp a second time, on oracle-1, probing the
/// same target as the derp on oracle-2
fn fleet() -> Value {
    let mut fleet: Value = serde_json::from_str(&Recorded::file("fleet.json")).unwrap();
    fleet["services"].as_array_mut().unwrap().push(json!({
        "name": "derp",
        "machine": "oracle-1",
        "unit": "tailscale-derper.service",
        "probes": [{"kind": "http", "target": "https://derp.r3dlust.com", "tls": false, "query": null}],
        "metrics": null,
        "heartbeat": null,
    }));
    fleet
}

fn refusal(fleet: &Value) -> String {
    format!(
        "{:#}",
        Registry::parse(fleet.to_string().as_bytes()).unwrap_err()
    )
}

#[test]
fn services_sort_by_name_then_by_machine() {
    let registry = Registry::parse(fleet().to_string().as_bytes()).unwrap();
    let ids: Vec<String> = registry.services.keys().map(ToString::to_string).collect();
    let derps: Vec<&str> = ids
        .iter()
        .map(String::as_str)
        .filter(|id| id.starts_with("derp "))
        .collect();
    assert_eq!(derps, ["derp on oracle-1", "derp on oracle-2"]);
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
}

#[test]
fn a_machine_runs_one_service_of_a_name() {
    let mut fleet = fleet();
    fleet["services"].as_array_mut().unwrap().push(json!({
        "name": "derp",
        "machine": "oracle-2",
        "unit": null,
        "probes": [],
    }));
    let refusal = refusal(&fleet);
    assert!(
        refusal.contains("derp on oracle-2 is registered twice"),
        "{refusal}"
    );
}

#[test]
fn a_service_runs_on_a_machine_of_the_fleet() {
    let mut fleet = fleet();
    fleet["services"].as_array_mut().unwrap().push(json!({
        "name": "derp",
        "machine": "oracle-3",
        "unit": null,
        "probes": [],
    }));
    let refusal = refusal(&fleet);
    assert!(
        refusal.contains("oracle-3 is named, but not among the machines"),
        "{refusal}"
    );
}

#[test]
fn a_service_reads_the_series_of_its_own_machine() {
    let recorded = Recorded::load();
    let registry = Registry::parse(fleet().to_string().as_bytes()).unwrap();
    let snapshot = snapshot::build(&registry, &recorded.answers(), recorded.now, Vec::new());
    let derp = |machine: &str| {
        snapshot
            .services
            .iter()
            .find(|service| service.name.0 == "derp" && service.machine.0 == machine)
            .unwrap()
    };

    // the recorded fleet probes derp on oracle-2 alone
    let probed = &derp("oracle-2").probes[0];
    assert!(!probed.views.is_empty());
    assert!(probed.cert_expires_at.is_some());
    let unprobed = &derp("oracle-1").probes[0];
    assert!(unprobed.views.is_empty());
    assert_eq!(unprobed.cert_expires_at, None);
}
