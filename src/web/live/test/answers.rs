//! what the real fleet answered, as the web app reads it

use std::collections::{BTreeMap, HashMap};

use super::Recorded;
use crate::web::{
    live::{
        alertmanager,
        alerts::{self, NoticeLine, Sources},
        charts, logs, loki,
        prometheus::{self, Labels},
    },
    query::LogQuery,
    wire::*,
};

fn up(machine: &Machine) -> (&Vitals, &Units) {
    match &machine.state {
        MachineState::Up { vitals, units, .. } => (vitals, units),
        other => panic!("{} is {other:?}", machine.name),
    }
}

#[test]
fn every_machine_of_the_recorded_fleet_is_up_with_its_hardware() {
    let recorded = Recorded::load();
    let snapshot = recorded.snapshot();

    let names: Vec<&str> = snapshot
        .machines
        .iter()
        .map(|m| m.name.0.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "altar",
            "desktop-nixos",
            "lab",
            "laptop-mac",
            "oracle-1",
            "oracle-2"
        ]
    );
    for machine in &snapshot.machines {
        let (vitals, units) = up(machine);
        for fraction in [vitals.cpu, vitals.mem, vitals.disk] {
            assert!(
                (0.0..=1.0).contains(&fraction),
                "{}: {vitals:?}",
                machine.name
            );
        }
        assert!(units.active > 0, "{} runs no units", machine.name);
        let hardware = machine.hardware.as_ref().expect("hardware");
        assert!(["aarch64", "x86_64"].contains(&hardware.arch.as_str()));
        assert!(
            hardware.cores > 0 && hardware.mem_bytes > 1 << 30 && hardware.disk_bytes > 1 << 30
        );
    }
    assert!(
        snapshot
            .nodes
            .iter()
            .all(|node| node.prometheus && node.loki && node.alertmanager && node.blackbox)
    );
    assert_eq!(snapshot.served_by, Hostname::from("altar"));
}

/// oracle's idle counters run faster than the clock, which made one minus
/// the idle rate negative
#[test]
fn a_virtual_machine_works_a_fraction_of_its_cpu() {
    let snapshot = Recorded::load().snapshot();
    let oracle = snapshot
        .machines
        .iter()
        .find(|machine| machine.name.0 == "oracle-1")
        .unwrap();
    let (vitals, _) = up(oracle);
    assert!(vitals.cpu > 0.0 && vitals.cpu < 0.5, "{vitals:?}");
}

#[test]
fn every_probe_is_seen_from_every_node() {
    let recorded = Recorded::load();
    let snapshot = recorded.snapshot();
    let probes: Vec<&Probe> = snapshot.services.iter().flat_map(|s| &s.probes).collect();
    assert_eq!(probes.len(), 19);
    for probe in probes {
        let nodes: Vec<&Hostname> = probe.views.iter().map(|view| &view.node).collect();
        assert_eq!(
            nodes,
            recorded.registry.nodes.iter().collect::<Vec<_>>(),
            "{}",
            probe.target
        );
        for view in &probe.views {
            assert!(
                view.ok && view.latency_ms > 0.0,
                "{}: {view:?}",
                probe.target
            );
            assert!(view.since.0 <= recorded.now);
        }
        let certified = probe.target.starts_with("https://") || probe.tls;
        if probe.target.starts_with("https://") {
            assert!(probe.cert_expires_at.is_some(), "{}", probe.target);
        }
        assert!(
            certified || probe.cert_expires_at.is_none(),
            "{}",
            probe.target
        );
    }
}

#[test]
fn every_service_unit_is_active_since_it_last_started() {
    let recorded = Recorded::load();
    for service in recorded.snapshot().services {
        let Some(unit) = service.unit else {
            continue;
        };
        assert_eq!(unit.state, UnitState::Active, "{}", service.name);
        assert!(unit.since.is_some_and(|since| since.0 < recorded.now));
    }
}

/// laptop-mac's niks3 upload job failed, which only launchd's reporter says
#[test]
fn a_failed_launchd_job_fails_from_when_its_alert_went_pending() {
    let snapshot = Recorded::load().snapshot();
    let laptop = snapshot
        .machines
        .iter()
        .find(|machine| machine.name.0 == "laptop-mac")
        .unwrap();
    let (_, units) = up(laptop);
    assert_eq!(
        units.failed,
        [FailedUnit {
            name: "org.nixos.niks3-auto-upload".into(),
            since: Ms(1_795_627_721_000),
        }]
    );
}

#[test]
fn the_firing_alert_is_the_clusters_and_every_node_sees_it() {
    let recorded = Recorded::load();
    let snapshot = recorded.snapshot();
    let [alert] = snapshot.alerts.as_slice() else {
        panic!("{:?}", snapshot.alerts);
    };
    assert_eq!(alert.id, AlertId::from("ae8ac1acc2b4edb0"));
    assert_eq!(alert.state, AlertState::Firing);
    assert_eq!(alert.severity, Severity::Info);
    assert_eq!(
        alert.summary,
        "org.nixos.niks3-auto-upload failed on laptop-mac"
    );
    assert_eq!(alert.machine, Some(Hostname::from("laptop-mac")));
    assert_eq!(alert.seen_by, recorded.registry.nodes);
}

/// what the cluster fingerprints, phoebe fingerprints the same from the
/// labels alone, so this node's ALERTS name the cluster's alerts
#[test]
fn an_alerts_fingerprint_is_alertmanagers() {
    let body = Recorded::file("alertmanager/alerts.json");
    let cluster = alertmanager::firing(&body, 0).unwrap();
    assert!(!cluster.is_empty());
    for alert in cluster {
        assert_eq!(alerts::fingerprint(&alert.labels), alert.fingerprint);
    }
}

#[test]
fn a_resolved_alert_keeps_its_episode_and_who_fired_it() {
    let recorded = Recorded::load();
    let history = prometheus::matrix(&Recorded::file("prometheus/history.json")).unwrap();
    let episodes = alerts::episodes(&history, recorded.history_step);
    let (id, episode) = episodes.iter().next().unwrap();
    // it fired, resolved for a while, and fired again: the episode is the
    // latest stretch alone
    let first = history[0].points[0].0;
    assert!(episode.first - first > 3_600_000, "{episode:?}");

    // were the cluster silent and the alert gone a while, it lists as
    // resolved, seen by those that fired it
    let later = episode.last + 3_600_000;
    let seen = BTreeMap::from([(
        Hostname::from("lab"),
        alerts::Seen {
            firing: Default::default(),
            fired: [id.clone()].into(),
        },
    )]);
    let list = alerts::list(&Sources {
        now: later,
        range: Recorded::DAY,
        firing: Some(&[]),
        episodes: &episodes,
        step: recorded.history_step,
        seen: &seen,
        summaries: &HashMap::from([(id.clone(), "it failed".to_owned())]),
    });
    let [alert] = list.as_slice() else {
        panic!("{list:?}");
    };
    assert_eq!(
        alert.state,
        AlertState::Resolved {
            at: Ms(episode.last + alerts::EVALUATION)
        }
    );
    assert_eq!(
        (alert.summary.as_str(), alert.seen_by.as_slice()),
        ("it failed", &[Hostname::from("lab")][..])
    );
}

#[test]
fn a_timeline_is_each_nodes_pending_firing_and_resolved() {
    let recorded = Recorded::load();
    let history = prometheus::matrix(&Recorded::file("prometheus/timeline.json")).unwrap();
    let node = Hostname::from("altar");
    let timeline = alerts::timeline(&node, &history, recorded.now, alerts::EVALUATION);
    let kinds: Vec<&str> = timeline
        .iter()
        .map(|entry| match entry {
            TimelineEntry::Pending { .. } => "pending",
            TimelineEntry::Firing { .. } => "firing",
            TimelineEntry::Resolved { .. } => "resolved",
            TimelineEntry::Notified { .. } => "notified",
        })
        .collect();
    assert_eq!(kinds, ["firing", "resolved", "pending", "firing"]);
}

#[test]
fn a_journal_line_takes_the_level_it_writes() {
    let entries = loki::streams(&Recorded::file("loki/journal.json")).unwrap();
    assert!(!entries.is_empty());
    let lines: Vec<LogLine> = entries.iter().map(loki::Entry::log_line).collect();
    for line in &lines {
        assert_eq!(
            (line.machine.0.as_str(), line.unit.as_str()),
            ("lab", "loki.service")
        );
    }
    // the journal files loki's `level=warn` at info
    let warned = lines
        .iter()
        .find(|line| line.text.starts_with("level=warn"))
        .unwrap();
    assert_eq!(warned.level, Level::Warn);
}

#[test]
fn a_darwin_line_is_its_file_without_its_colors() {
    let entries = loki::streams(&Recorded::file("loki/files.json")).unwrap();
    assert!(!entries.is_empty());
    for line in entries.iter().map(loki::Entry::log_line) {
        assert_eq!(line.unit, "/var/log/coeusd.log");
        assert!(!line.text.contains('\u{1b}'), "{:?}", line.text);
    }
    let info = entries
        .iter()
        .map(loki::Entry::log_line)
        .find(|line| line.text.contains(" INFO "))
        .unwrap();
    assert_eq!(info.level, Level::Info);
}

#[test]
fn a_tails_batch_is_oldest_first() {
    let entries = loki::tailed(&Recorded::file("loki/tail.json")).unwrap();
    assert!(entries.len() > 1);
    let lines = logs::batch(entries, 5);
    assert_eq!(lines.len(), 5);
    assert!(lines.windows(2).all(|pair| pair[0].at <= pair[1].at));
}

#[test]
fn a_chart_is_its_answer_on_the_grid() {
    let ranges = prometheus::matrix(&Recorded::file("charts/cpu.json")).unwrap();
    let points = &ranges[0].points;
    let grid: Vec<i64> = points.iter().map(|(t, _)| *t).collect();
    let SeriesData::Lines { lines } = charts::lines(vec![("cpu", ranges.clone())], &grid) else {
        panic!("not lines");
    };
    assert_eq!(lines[0].name, "cpu");
    assert!(
        lines[0]
            .points
            .iter()
            .all(|(_, v)| v.is_some_and(|v| (0.0..=1.0).contains(&v)))
    );

    let latency = prometheus::matrix(&Recorded::file("charts/latency.json")).unwrap();
    let grid: Vec<i64> = latency[0].points.iter().map(|(t, _)| *t).collect();
    let SeriesData::Band { points } = charts::band(&latency, &grid) else {
        panic!("not a band");
    };
    let measured: Vec<_> = points.iter().filter(|(_, lo, ..)| lo.is_some()).collect();
    assert!(!measured.is_empty());
    for (_, lo, mid, hi) in measured {
        assert!(lo <= mid && mid <= hi, "{lo:?} {mid:?} {hi:?}");
    }
}

#[test]
fn a_notice_line_is_found_wherever_the_logger_put_it() {
    let notice = NoticeLine {
        alert: AlertId::from("ae8ac1acc2b4edb0"),
        resolved: false,
        rule: "UnitFailed".into(),
        summary: "lavalink failed on oracle-1".into(),
        devices: 1,
        delivered: 1,
    };
    let logged = format!(
        "[2026-11-25T18:00:00Z INFO  phoebe::web::live::hook] {}",
        notice.text()
    );
    assert_eq!(NoticeLine::find(&logged), Some(notice));
    assert_eq!(NoticeLine::find("notice of something else"), None);
}

fn query(machine: Option<&str>, service: Option<&str>, q: Option<&str>) -> LogQuery {
    LogQuery {
        machine: machine.map(Hostname::from),
        unit: None,
        service: service.map(ServiceName::from),
        q: q.map(str::to_owned),
    }
}

#[test]
fn a_log_query_is_one_logql() {
    let registry = Recorded::load().registry;
    let logql = |machine, service, q| logs::logql(&registry, &query(machine, service, q));
    assert_eq!(logql(None, None, None).unwrap(), r#"{machine=~".+"}"#);
    assert_eq!(
        logql(None, Some("heimdall"), None).unwrap(),
        r#"{machine="oracle-1", unit="heimdall.service"}"#
    );
    assert_eq!(
        logql(None, Some("tailscale-proxy"), Some("a.b")).unwrap(),
        r#"{machine=~".+"} |~ "(?i)(tailscale\\.r3dlust\\.com)" |~ "(?i)a\\.b""#
    );
    assert_eq!(
        logql(Some("lab"), Some("heimdall"), None),
        Err(crate::web::query::Refusal::Invalid(
            "heimdall runs on oracle-1, not lab".into()
        ))
    );
    assert!(logql(Some("nope"), None, None).is_err());
    assert_eq!(
        logs::notices(&registry, Some(&AlertId::from("ab"))),
        [
            r#"{unit="phoebe-web.service"} |= "notice {\"alert\":\"ab\"""#,
            r#"{file="/var/log/phoebe-web.log"} |= "notice {\"alert\":\"ab\"""#,
        ]
    );
}

fn entry(ns: i64, line: &str) -> loki::Entry {
    loki::Entry {
        ns,
        labels: Labels::from([("machine".into(), "lab".into())]),
        line: line.into(),
    }
}

/// lines that share a nanosecond split across pages without one lost or shown twice
#[test]
fn a_page_ends_between_lines_of_the_same_moment() {
    let lines = [
        entry(3, "c"),
        entry(2, "b1"),
        entry(2, "b2"),
        entry(2, "b3"),
        entry(1, "a"),
    ];
    let first = logs::page(lines.to_vec(), None, 2);
    assert_eq!(
        first
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>(),
        ["c", "b1"]
    );
    let before = logs::Cursor::parse(first.before.as_deref().unwrap()).unwrap();
    // loki answers the next page up to and including that moment
    let rest: Vec<_> = lines
        .iter()
        .filter(|e| e.ns <= before.ns)
        .cloned()
        .collect();
    let second = logs::page(rest.clone(), Some(before), 2);
    assert_eq!(
        second
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>(),
        ["b2", "b3"]
    );
    let before = logs::Cursor::parse(second.before.as_deref().unwrap()).unwrap();
    let third = logs::page(rest, Some(before), 2);
    assert_eq!(
        third
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>(),
        ["a"]
    );
    assert_eq!(third.before, None);
}
