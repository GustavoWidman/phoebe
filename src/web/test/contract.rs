//! what the browser reads, pinned from both ends: `contract/web.json` holds
//! an example of every message as the server serializes it, and
//! `web/src/lib/wire/types.ts` declares the types the browser parses them
//! into, generated from src/web/wire.rs. a change to the wire fails here
//! until both files are rewritten with `PHOEBE_BLESS=1 cargo test web::`.

use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;

use super::typescript::{literals, names, typescript};
use crate::web::{
    fixture::{Fixture, START},
    query::{LogPageQuery, LogQuery, Range, SeriesQuery},
    wire::*,
};

const CONTRACT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/contract/web.json");
const TYPES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/web/src/lib/wire/types.ts");

/// the file at `path` holds `generated`, or is rewritten to when blessing
fn pinned(path: &str, generated: &str) {
    if std::env::var_os("PHOEBE_BLESS").is_some() {
        std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
        std::fs::write(path, generated).unwrap();
        return;
    }
    let pinned = std::fs::read_to_string(path).unwrap_or_default();
    assert!(
        pinned == generated,
        "{path} is not what src/web/wire.rs generates; rewrite it with `PHOEBE_BLESS=1 cargo test web::`"
    );
}

#[test]
fn the_contract_holds_what_the_server_sends() {
    pinned(CONTRACT, &contract());
}

#[test]
fn the_typescript_types_are_the_rust_ones() {
    pinned(TYPES, &typescript());
}

/// every tag and every unit variant the types declare appears in an example
#[test]
fn the_contract_shows_every_kind_the_types_declare() {
    let mut strings = BTreeSet::new();
    collect(&serde_json::from_str(&contract()).unwrap(), &mut strings);

    let missing: Vec<String> = literals(&typescript())
        .into_iter()
        .filter(|literal| !strings.contains(literal))
        .collect();

    assert!(missing.is_empty(), "the contract shows no {missing:?}");
}

#[test]
fn every_type_the_typescript_names_is_declared() {
    let types = typescript();
    let declared: BTreeSet<&str> = types
        .lines()
        .filter_map(|line| line.strip_prefix("export type "))
        .filter_map(|line| line.split(' ').next())
        .collect();

    let undeclared: BTreeSet<String> = names(&types)
        .into_iter()
        .filter(|name| name != "Array" && !declared.contains(name.as_str()))
        .collect();

    assert!(undeclared.is_empty(), "undeclared: {undeclared:?}");
}

/// a series cut to its first three points, enough to show its shape
fn sample(mut series: Series) -> Series {
    match &mut series.data {
        SeriesData::Lines { lines } => lines.iter_mut().for_each(|line| line.points.truncate(3)),
        SeriesData::Band { points } => points.truncate(3),
    }
    series
}

/// states the simulated fleet's story never reaches, written by hand
#[derive(Serialize)]
struct Other {
    machines: Vec<Machine>,
    units: Vec<UnitStatus>,
    heartbeats: Vec<Heartbeat>,
    probes: Vec<Probe>,
    lines: Vec<LogLine>,
}

fn other() -> Other {
    let at = |offset: i64| Ms(START + offset);
    let unit = |name: &str, state, since| UnitStatus {
        name: name.into(),
        state,
        since,
    };
    Other {
        machines: vec![Machine {
            name: "oracle-1".into(),
            os: Os::NixOs,
            always_on: true,
            hardware: None,
            state: MachineState::Down {
                since: at(-420_000),
            },
        }],
        units: vec![
            unit("heimdall.service", UnitState::Activating, Some(at(-4_000))),
            unit("nfs-server.service", UnitState::Inactive, Some(at(-90_000))),
            unit("lavalink.service", UnitState::Unknown, None),
        ],
        heartbeats: [
            (HeartbeatState::Late, Some(at(-5_400_000))),
            (HeartbeatState::Missing, None),
            (HeartbeatState::Unknown, None),
        ]
        .map(|(state, healthy_at)| Heartbeat {
            metric: "norn_healthy_timestamp_seconds".into(),
            within_ms: 3_600_000,
            state,
            healthy_at,
        })
        .into(),
        probes: vec![Probe {
            kind: ProbeKind::Dns,
            target: "dns.r3dlust.com:53".into(),
            tls: false,
            cert_expires_at: None,
            views: vec![ProbeView {
                node: "altar".into(),
                ok: true,
                latency_ms: 27.4,
                since: at(-86_400_000),
            }],
        }],
        lines: vec![LogLine {
            at: at(-2_000),
            machine: "lab".into(),
            unit: "coeusd.service".into(),
            level: Level::Debug,
            text: "poll: generation 26.05.20261125 is current".into(),
        }],
    }
}

/// one example of every message, as the server serializes it: the
/// simulated fleet at the start of its timeline, and [Other]
fn contract() -> String {
    let fixture = Fixture::new(0.0);
    let snapshot = Snapshot::clone(&fixture.fleet().borrow());

    let series = |of: &str, metric, node: Option<&str>| {
        let (kind, name) = of.split_once(':').unwrap();
        let of = match kind {
            "machine" => Subject::Machine(name.into()),
            _ => Subject::Service(ServiceId::parse(name).unwrap()),
        };
        let query = SeriesQuery {
            of,
            metric,
            range: Range::Hour,
            node: node.map(Hostname::from),
        };
        sample(fixture.series(&query).unwrap())
    };
    let series = vec![
        series("machine:oracle-1", Metric::Cpu, None),
        series("machine:oracle-1", Metric::Mem, None),
        series("machine:oracle-2", Metric::Disk, None),
        series("machine:lab", Metric::Load, None),
        series("machine:lab", Metric::Net, None),
        series("service:oracle-2/portfolio", Metric::Latency, None),
        series("service:oracle-2/portfolio", Metric::Latency, Some("lab")),
    ];

    let logs = fixture
        .logs(&LogPageQuery {
            lines: LogQuery {
                machine: Some("oracle-1".into()),
                unit: Some("lavalink.service".into()),
                ..LogQuery::default()
            },
            before: None,
            limit: 6,
        })
        .unwrap();
    let tail = vec![
        LogEvent::Lines {
            lines: logs.lines[..2].iter().rev().cloned().collect(),
        },
        LogEvent::Expired,
    ];

    let alerts = fixture.alerts(Range::Day.ms());
    let resolved = alerts
        .iter()
        .find(|alert| alert.rule == "DiskAlmostFull")
        .unwrap();
    let mut alert = fixture.alert(&resolved.id).unwrap();
    alert.series = alert.series.map(sample);

    let viewer = Viewer {
        login: "gustavo@github".into(),
        device: "iphone".into(),
        expires_at: Ms(START + 7 * 24 * 3_600_000),
    };

    #[derive(Serialize)]
    struct Contract {
        hello: Hello,
        signin: SignIn,
        signed_in: SignedIn,
        push: PushView,
        stream: Vec<StreamEvent>,
        series: Vec<Series>,
        logs: LogPage,
        tail: Vec<LogEvent>,
        alerts: Vec<Alert>,
        alert: AlertDetail,
        fixture_clock: FixtureClock,
        other: Other,
    }
    let contract = Contract {
        hello: Hello {
            signin: vec![
                "https://lab.tail4a3ea.ts.net:45731".into(),
                "https://oracle-1.tail4a3ea.ts.net:45731".into(),
            ],
            build: "4f2a9c1e07b3".into(),
            schema: SCHEMA,
        },
        signin: SignIn {
            key: Jwk {
                kty: "EC".into(),
                crv: "P-256".into(),
                x: "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU".into(),
                y: "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0".into(),
            },
            ts: Ms(START),
            sig: "kTnLdVa0YyBGrdyKrq0W2RQSgwPRkpbOg9eW0bJ0rPQhoNTGDe2BMLjZz8x7FzWHZ2h8n2ShyeSQnBuqFPsKVw".into(),
        },
        signed_in: SignedIn {
            token: "v1.eyJ2IjoxfQ.c2lnbmVk".into(),
            viewer,
        },
        push: Fixture::push(),
        stream: vec![StreamEvent::Snapshot(snapshot), StreamEvent::Expired],
        series,
        logs,
        tail,
        alerts,
        alert,
        fixture_clock: fixture.seek(None, 0.0),
        other: other(),
    };
    serde_json::to_string_pretty(&contract).unwrap() + "\n"
}

fn collect(value: &Value, strings: &mut BTreeSet<String>) {
    match value {
        Value::String(string) => {
            strings.insert(string.clone());
        }
        Value::Array(values) => values.iter().for_each(|value| collect(value, strings)),
        Value::Object(map) => {
            for (key, value) in map {
                strings.insert(key.clone());
                collect(value, strings);
            }
        }
        _ => {}
    }
}
