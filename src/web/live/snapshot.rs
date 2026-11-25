//! the fleet at one moment, built from what this node's stores answered.
//! everything here is a function of the answers, so recorded ones build the
//! same snapshot as live ones

use std::collections::BTreeMap;

use super::{
    prometheus::Sample,
    promql::Named,
    registry::{ProbeEntry, Registry, ServiceEntry},
};
use crate::web::wire::*;

/// a minute, the step every "since" is found at
const MINUTE: i64 = 60_000;
const WEEK: i64 = 7 * 24 * 60 * MINUTE;

/// every instant query of [Named::SNAPSHOT] this node's Prometheus
/// answered; one that did not answer is missing
#[derive(Clone, Debug, Default)]
pub struct Answers {
    pub series: BTreeMap<Named, Vec<Sample>>,
}

impl Answers {
    /// the samples of `series` whose labels hold every `(name, value)`
    fn all<'a>(
        &'a self,
        series: Named,
        labels: &'a [(&'a str, &'a str)],
    ) -> impl Iterator<Item = &'a Sample> + 'a {
        self.series
            .get(&series)
            .into_iter()
            .flatten()
            .filter(move |sample| {
                labels.iter().all(|(name, value)| {
                    sample.labels.get(*name).map(String::as_str) == Some(value)
                })
            })
    }

    fn value(&self, series: Named, labels: &[(&str, &str)]) -> Option<f64> {
        self.all(series, labels).next().map(|sample| sample.value)
    }

    /// a time the series holds in epoch seconds, in epoch ms
    fn time(&self, series: Named, labels: &[(&str, &str)]) -> Option<i64> {
        self.value(series, labels)
            .map(|seconds| (seconds * 1_000.0) as i64)
    }
}

/// the fleet at `now` as `answers` show it, with `alerts` as they are
pub fn build(registry: &Registry, answers: &Answers, now: i64, alerts: Vec<Alert>) -> Snapshot {
    Snapshot {
        schema: SCHEMA,
        at: Ms(now),
        served_by: registry.machine.clone(),
        nodes: registry
            .nodes
            .iter()
            .map(|node| {
                let up = |job: &str| {
                    answers.value(Named::Stores, &[("machine", &node.0), ("job", job)]) == Some(1.0)
                };
                Node {
                    name: node.clone(),
                    // the snapshot's own answers are this node's prometheus
                    // answering
                    prometheus: up("prometheus")
                        || (*node == registry.machine && !answers.series.is_empty()),
                    loki: up("loki"),
                    alertmanager: up("alertmanager"),
                    blackbox: up("blackbox"),
                }
            })
            .collect(),
        machines: registry
            .machines
            .iter()
            .map(|(name, entry)| machine(answers, now, name, entry.os, entry.always_on))
            .collect(),
        services: registry
            .services
            .iter()
            .map(|(name, entry)| service(registry, answers, now, name, entry))
            .collect(),
        alerts,
    }
}

/// when a machine last started answering, or stopped
struct Presence {
    up: bool,
    since: i64,
    booted: i64,
}

fn presence(answers: &Answers, now: i64, name: &str) -> Presence {
    let on = [("machine", name)];
    let first = answers.time(Named::FirstScraped, &on);
    let up = answers.value(Named::Up, &on) == Some(1.0);
    if up {
        let booted = answers.time(Named::BootTime, &on).or(first).unwrap_or(now);
        let since = answers
            .time(Named::LastDown, &on)
            .map_or(booted, |down| (down + MINUTE).max(booted));
        Presence {
            up,
            since: since.min(now),
            booted,
        }
    } else {
        let since = answers
            .time(Named::LastUp, &on)
            .map(|last| last + MINUTE)
            .or(first)
            .unwrap_or(now - WEEK);
        Presence {
            up,
            since: since.min(now),
            booted: since,
        }
    }
}

fn machine(answers: &Answers, now: i64, name: &Hostname, os: Os, always_on: bool) -> Machine {
    let on = [("machine", name.0.as_str())];
    let presence = presence(answers, now, &name.0);
    let state = if presence.up {
        let fraction = |series| {
            answers
                .value(series, &on)
                .map_or(0.0, |value| value.clamp(0.0, 1.0))
        };
        let mut failed: Vec<FailedUnit> = answers
            .all(Named::UnitsFailed, &on)
            .filter_map(|sample| sample.labels.get("name"))
            .map(|unit| FailedUnit {
                name: unit.clone(),
                since: Ms(answers
                    .time(Named::FailedSince, &[("machine", &name.0), ("name", unit)])
                    .unwrap_or(now)),
            })
            .collect();
        failed.sort_by(|a, b| a.name.cmp(&b.name));
        failed.dedup_by(|a, b| a.name == b.name);
        MachineState::Up {
            since: Ms(presence.since),
            booted_at: Ms(presence.booted),
            vitals: Vitals {
                cpu: fraction(Named::Cpu),
                mem: fraction(Named::Mem),
                disk: fraction(Named::Disk),
                load1: answers.value(Named::Load, &on).unwrap_or(0.0),
            },
            units: Units {
                active: answers.value(Named::UnitsActive, &on).unwrap_or(0.0) as u32,
                failed,
            },
        }
    } else if always_on {
        MachineState::Down {
            since: Ms(presence.since),
        }
    } else {
        MachineState::Resting {
            since: Ms(presence.since),
        }
    };
    Machine {
        name: name.clone(),
        os,
        always_on,
        hardware: hardware(answers, &name.0),
        state,
    }
}

/// as last scraped within a week
fn hardware(answers: &Answers, name: &str) -> Option<Hardware> {
    let on = [("machine", name)];
    let arch = answers
        .all(Named::Uname, &on)
        .find_map(|sample| sample.labels.get("exported_machine"))?;
    Some(Hardware {
        arch: match arch.as_str() {
            "arm64" => "aarch64",
            "amd64" => "x86_64",
            other => other,
        }
        .to_owned(),
        cores: answers.value(Named::Cores, &on)? as u32,
        mem_bytes: answers.value(Named::MemBytes, &on)? as u64,
        disk_bytes: answers.value(Named::DiskBytes, &on)? as u64,
    })
}

fn service(
    registry: &Registry,
    answers: &Answers,
    now: i64,
    name: &ServiceName,
    entry: &ServiceEntry,
) -> Service {
    let host = presence(answers, now, &entry.machine.0);
    let unit = entry.unit.as_ref().map(|unit| {
        if !host.up {
            return UnitStatus {
                name: unit.clone(),
                state: UnitState::Unknown,
                since: None,
            };
        }
        let on = [
            ("machine", entry.machine.0.as_str()),
            ("name", unit.as_str()),
        ];
        let current = answers
            .all(Named::UnitState, &on)
            .find(|sample| sample.value == 1.0)
            .and_then(|sample| sample.labels.get("state"));
        let Some(current) = current else {
            // systemd forgets a unit that is neither loaded nor wanted
            return UnitStatus {
                name: unit.clone(),
                state: UnitState::Inactive,
                since: Some(Ms(host.since)),
            };
        };
        let since = answers
            .time(
                Named::UnitLeft,
                &[
                    ("machine", entry.machine.0.as_str()),
                    ("name", unit.as_str()),
                    ("state", current.as_str()),
                ],
            )
            .map_or(host.booted, |left| (left + MINUTE).max(host.booted));
        UnitStatus {
            name: unit.clone(),
            state: match current.as_str() {
                "active" => UnitState::Active,
                "activating" | "reloading" | "refreshing" => UnitState::Activating,
                "failed" => UnitState::Failed,
                _ => UnitState::Inactive,
            },
            since: Some(Ms(since.min(now))),
        }
    });
    Service {
        name: name.clone(),
        machine: entry.machine.clone(),
        unit,
        probes: entry
            .probes
            .iter()
            .map(|probe| self::probe(registry, answers, now, name, probe))
            .collect(),
    }
}

fn probe(
    registry: &Registry,
    answers: &Answers,
    now: i64,
    service: &ServiceName,
    probe: &ProbeEntry,
) -> Probe {
    let target = [
        ("service", service.0.as_str()),
        ("target", probe.target.as_str()),
    ];
    let views = registry
        .nodes
        .iter()
        .filter_map(|node| {
            let on = [
                ("service", service.0.as_str()),
                ("target", probe.target.as_str()),
                ("prober", node.0.as_str()),
            ];
            let ok = answers.value(Named::ProbeOk, &on)? == 1.0;
            let other = match ok {
                true => Named::ProbeLastFailed,
                false => Named::ProbeLastOk,
            };
            let since = answers
                .time(other, &on)
                .map(|t| t + MINUTE)
                .or_else(|| answers.time(Named::ProbeFirst, &on))
                .unwrap_or(now - WEEK);
            Some(ProbeView {
                node: node.clone(),
                ok,
                latency_ms: answers
                    .value(Named::ProbeSeconds, &on)
                    .map_or(0.0, |seconds| seconds * 1_000.0),
                since: Ms(since.min(now)),
            })
        })
        .collect();
    Probe {
        kind: probe.kind,
        target: probe.target.clone(),
        tls: probe.tls(),
        cert_expires_at: answers.time(Named::CertExpiry, &target).map(Ms),
        views,
    }
}
