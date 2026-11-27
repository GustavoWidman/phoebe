//! the story at one moment, in the shapes the web app reads

use std::collections::BTreeMap;

use super::{
    fleet::{self, MACHINES, MachineSpec, NODES, SERVED_BY, SERVICES, ServiceSpec},
    story::{
        self, DAY, HISTORY, INCIDENTS, Incident, Presence, STEP, TIMEOUT_MS, cert_expiry, latency,
        presence, reach,
    },
};
use crate::web::{
    live::charts::spans,
    query::{Refusal, SeriesQuery},
    wire::*,
};

pub fn snapshot(at: i64) -> Snapshot {
    let mut firing: Vec<&Incident> = visible(at)
        .filter(|incident| incident.resolved_at().is_none_or(|resolved| at < resolved))
        .collect();
    firing.sort_by_key(|incident| std::cmp::Reverse(incident.starts_at()));
    let mut resolved: Vec<&Incident> = visible(at)
        .filter(|incident| {
            incident
                .resolved_at()
                .is_some_and(|resolved| resolved <= at && at - resolved < DAY)
        })
        .collect();
    resolved.sort_by_key(|incident| std::cmp::Reverse(incident.resolved_at()));

    Snapshot {
        schema: SCHEMA,
        at: Ms(at),
        served_by: Hostname::from(SERVED_BY),
        nodes: NODES
            .iter()
            .map(|node| Node {
                name: Hostname::from(*node),
                prometheus: true,
                loki: story::loki_up(node, at),
                alertmanager: true,
                blackbox: true,
            })
            .collect(),
        machines: MACHINES.iter().map(|spec| machine(spec, at)).collect(),
        services: SERVICES.iter().map(|spec| service(spec, at)).collect(),
        alerts: firing
            .into_iter()
            .chain(resolved)
            .map(|incident| alert(incident, at))
            .collect(),
    }
}

fn machine(spec: &MachineSpec, at: i64) -> Machine {
    let state = match presence(spec, at) {
        Presence::Up { since, booted } => {
            let value = |metric| story::metric(spec, metric, at)[0];
            let failed: Vec<FailedUnit> = SERVICES
                .iter()
                .filter(|service| service.machine == spec.name)
                .filter_map(|service| {
                    let (state, since) = story::unit(service, booted, at);
                    (state == UnitState::Failed).then(|| FailedUnit {
                        name: service.unit.unwrap_or_default().into(),
                        since: Ms(since),
                    })
                })
                .collect();
            MachineState::Up {
                since: Ms(since),
                booted_at: Ms(booted),
                vitals: Vitals {
                    cpu: value(Metric::Cpu),
                    mem: value(Metric::Mem),
                    disk: value(Metric::Disk),
                    load1: value(Metric::Load),
                },
                units: Units {
                    active: spec.units - failed.len() as u32,
                    failed,
                },
            }
        }
        Presence::Away { since } if spec.always_on => MachineState::Down { since: Ms(since) },
        Presence::Away { since } => MachineState::Resting { since: Ms(since) },
    };
    Machine {
        name: Hostname::from(spec.name),
        os: spec.os,
        always_on: spec.always_on,
        hardware: Some(Hardware {
            arch: spec.arch.into(),
            cores: spec.cores,
            mem_bytes: spec.mem_gib << 30,
            disk_bytes: spec.disk_gb * 1_000_000_000,
        }),
        state,
    }
}

fn service(spec: &ServiceSpec, at: i64) -> Service {
    let host = fleet::machine(spec.machine).expect("every service's machine is in the registry");
    let unit = spec.unit.map(|name| match presence(host, at) {
        Presence::Up { booted, .. } => {
            let (state, since) = story::unit(spec, booted, at);
            UnitStatus {
                name: name.into(),
                state,
                since: Some(Ms(since)),
            }
        }
        Presence::Away { .. } => UnitStatus {
            name: name.into(),
            state: UnitState::Unknown,
            since: None,
        },
    });
    let probes = spec
        .probes
        .iter()
        .enumerate()
        .map(|(index, probe)| Probe {
            kind: probe.kind,
            target: probe.target.into(),
            tls: probe.certified(),
            cert_expires_at: cert_expiry(probe, at).map(Ms),
            views: NODES
                .iter()
                .map(|node| {
                    let (ok, since) = reach(probe, node, at);
                    ProbeView {
                        node: Hostname::from(*node),
                        ok,
                        latency_ms: latency(spec, index, node, at).unwrap_or(TIMEOUT_MS),
                        since: Ms(since),
                    }
                })
                .collect(),
        })
        .collect();
    let heartbeat = spec.heartbeat.as_ref().map(|beat| {
        let (state, healthy_at) = match presence(host, at) {
            Presence::Up { .. } => (
                HeartbeatState::Fresh,
                Some(Ms(at.div_euclid(beat.every_ms) * beat.every_ms)),
            ),
            Presence::Away { .. } => (HeartbeatState::Unknown, None),
        };
        Heartbeat {
            metric: beat.metric.into(),
            within_ms: beat.within_ms,
            state,
            healthy_at,
        }
    });
    Service {
        name: ServiceName::from(spec.name),
        machine: Hostname::from(spec.machine),
        unit,
        probes,
        heartbeat,
    }
}

/// the incidents an alertmanager knows of at `at`: every one that fired
fn visible(at: i64) -> impl Iterator<Item = &'static Incident> {
    INCIDENTS
        .iter()
        .filter(move |incident| incident.starts_at() <= at)
}

fn alert(incident: &Incident, at: i64) -> Alert {
    let resolved = incident.resolved_at().filter(|resolved| *resolved <= at);
    let seen_by = incident
        .seen()
        .filter(|seen| seen.firing <= at)
        .filter(|seen| resolved.is_some() || seen.resolved.is_none_or(|gone| at < gone))
        .map(|seen| Hostname::from(seen.node))
        .collect();
    // its summary as last evaluated while it fired
    let evaluated = incident
        .cleared
        .map_or(at, |cleared| at.min(story::next_step(cleared) - STEP));

    let mut labels: BTreeMap<String, String> = incident
        .labels
        .iter()
        .map(|(key, value)| ((*key).into(), (*value).into()))
        .collect();
    labels.insert("alertname".into(), incident.rule.into());
    let severity = match incident.severity {
        Severity::Page => "page",
        Severity::Info => "info",
    };
    labels.insert("severity".into(), severity.into());
    if let Some(machine) = incident.machine {
        labels.insert("machine".into(), machine.into());
    }
    if let Some(service) = incident.service {
        labels.insert("service".into(), service.into());
    }

    Alert {
        id: fingerprint(&labels),
        rule: incident.rule.into(),
        severity: incident.severity,
        state: match resolved {
            Some(at) => AlertState::Resolved { at: Ms(at) },
            None => AlertState::Firing,
        },
        starts_at: Ms(incident.starts_at()),
        machine: incident.machine.map(Hostname::from),
        service: incident
            .service
            .zip(incident.machine)
            .map(|(name, machine)| ServiceId::new(machine, name)),
        summary: (incident.summary)(evaluated),
        seen_by,
        labels,
    }
}

/// an alert's identity, from its labels alone, as alertmanager's is
fn fingerprint(labels: &BTreeMap<String, String>) -> AlertId {
    // FNV-1a over the sorted labels
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (key, value) in labels {
        for byte in key.bytes().chain([0xff]).chain(value.bytes()).chain([0xff]) {
            hash = (hash ^ byte as u64).wrapping_mul(0x0100_0000_01b3);
        }
    }
    AlertId(format!("{hash:016x}"))
}

/// alerts that fired within `range` before `at`, or still fire, newest first
pub fn alerts(range: i64, at: i64) -> Vec<Alert> {
    let mut incidents: Vec<&Incident> = visible(at)
        .filter(|incident| {
            incident
                .resolved_at()
                .is_none_or(|resolved| resolved >= at - range)
        })
        .collect();
    incidents.sort_by_key(|incident| std::cmp::Reverse(incident.starts_at()));
    incidents
        .into_iter()
        .map(|incident| alert(incident, at))
        .collect()
}

pub fn alert_detail(id: &AlertId, at: i64) -> Result<AlertDetail, Refusal> {
    let (incident, alert) = visible(at)
        .map(|incident| (incident, alert(incident, at)))
        .find(|(_, alert)| alert.id == *id)
        .ok_or_else(|| Refusal::Missing(format!("no alert {id}")))?;

    let mut timeline: Vec<TimelineEntry> = Vec::new();
    for seen in incident.seen() {
        let node = Hostname::from(seen.node);
        timeline.push(TimelineEntry::Pending {
            at: Ms(seen.pending),
            node: node.clone(),
        });
        timeline.push(TimelineEntry::Firing {
            at: Ms(seen.firing),
            node: node.clone(),
        });
        if let Some(resolved) = seen.resolved {
            timeline.push(TimelineEntry::Resolved {
                at: Ms(resolved),
                node,
            });
        }
    }
    if let Some((by, notified, resolution)) = incident.notified() {
        let by = Hostname::from(by);
        timeline.push(TimelineEntry::Notified {
            at: Ms(notified),
            by: by.clone(),
            resolved: false,
        });
        if let Some(resolution) = resolution {
            timeline.push(TimelineEntry::Notified {
                at: Ms(resolution),
                by,
                resolved: true,
            });
        }
    }
    timeline.retain(|entry| entry_at(entry) <= at);
    timeline.sort_by_key(entry_at);

    let series = incident.chart.map(|metric| {
        let of = match metric {
            Metric::Latency => Subject::Service(ServiceId::new(
                incident.machine.unwrap_or_default(),
                incident.service.unwrap_or_default(),
            )),
            _ => Subject::Machine(Hostname::from(incident.machine.unwrap_or_default())),
        };
        let to = incident
            .resolved_at()
            .map_or(at, |resolved| at.min(resolved + story::HOUR));
        let from = (incident.starts_at() - story::HOUR).max(to - HISTORY);
        series(&of, metric, None, from, to, step_for(to - from))
    });

    Ok(AlertDetail {
        alert,
        timeline,
        series: series.transpose()?,
    })
}

fn entry_at(entry: &TimelineEntry) -> i64 {
    match entry {
        TimelineEntry::Pending { at, .. }
        | TimelineEntry::Firing { at, .. }
        | TimelineEntry::Resolved { at, .. }
        | TimelineEntry::Notified { at, .. } => at.0,
    }
}

/// points in a series: about this many, whatever its range
const POINTS: i64 = 240;

/// the step that puts about [POINTS] across `span`, whole scrapes
fn step_for(span: i64) -> i64 {
    (span / POINTS).div_euclid(STEP).max(1) * STEP
}

pub fn series_at(query: &SeriesQuery, at: i64) -> Result<Series, Refusal> {
    let span = query.range.ms();
    let step = step_for(span);
    let to = at.div_euclid(step) * step;
    series(
        &query.of,
        query.metric,
        query.node.as_ref(),
        to - span,
        to,
        step,
    )
}

/// `metric` of `of` at every `step` in `(from, to]`
fn series(
    of: &Subject,
    metric: Metric,
    node: Option<&Hostname>,
    from: i64,
    to: i64,
    step: i64,
) -> Result<Series, Refusal> {
    let start = from.div_euclid(step) * step + step;
    let times: Vec<i64> = (0..)
        .map(|k| start + k * step)
        .take_while(|t| *t <= to)
        .collect();

    let mut failing = Vec::new();
    let data = match (of, metric) {
        (Subject::Machine(name), Metric::Latency) => {
            return Err(Refusal::Invalid(format!(
                "latency is a service's; {name} is a machine"
            )));
        }
        (Subject::Machine(name), _) => {
            let spec = fleet::machine(&name.0)
                .ok_or_else(|| Refusal::Missing(format!("no machine {name}")))?;
            let names: &[&str] = match metric {
                Metric::Net => &["rx", "tx"],
                Metric::Cpu => &["cpu"],
                Metric::Mem => &["mem"],
                Metric::Disk => &["disk"],
                _ => &["load"],
            };
            let values: Vec<Option<Vec<f64>>> = times
                .iter()
                .map(|t| story::up(spec, *t).then(|| story::metric(spec, metric, *t)))
                .collect();
            SeriesData::Lines {
                lines: names
                    .iter()
                    .enumerate()
                    .map(|(i, line)| Line {
                        name: (*line).into(),
                        points: times
                            .iter()
                            .zip(&values)
                            .map(|(t, value)| (Ms(*t), value.as_ref().map(|value| value[i])))
                            .collect(),
                    })
                    .collect(),
            }
        }
        (Subject::Service(id), Metric::Latency) => {
            let spec =
                fleet::service(id).ok_or_else(|| Refusal::Missing(format!("no service {id}")))?;
            if spec.probes.is_empty() {
                return Err(Refusal::Invalid(format!("{id} has no probes")));
            }
            let probing: Vec<&str> = match node {
                Some(node) => vec![
                    *NODES
                        .iter()
                        .find(|n| **n == node.0)
                        .ok_or_else(|| Refusal::Missing(format!("no node {node}")))?,
                ],
                None => NODES.to_vec(),
            };
            let failed: Vec<bool> = times
                .iter()
                .map(|t| probing.iter().any(|node| slowest(spec, node, *t).is_none()))
                .collect();
            failing = spans(&failed, &times);
            match node {
                Some(_) => {
                    let node = probing[0];
                    SeriesData::Lines {
                        lines: vec![Line {
                            name: (*node).into(),
                            points: times
                                .iter()
                                .map(|t| (Ms(*t), slowest(spec, node, *t)))
                                .collect(),
                        }],
                    }
                }
                None => SeriesData::Band {
                    points: times.iter().map(|t| band(spec, *t)).collect(),
                },
            }
        }
        (Subject::Service(id), _) => {
            return Err(Refusal::Invalid(format!(
                "a service has a latency only; {id} is a service"
            )));
        }
    };
    Ok(Series {
        of: of.clone(),
        metric,
        from: Ms(from),
        to: Ms(to),
        data,
        failing,
    })
}

/// the slowest of a service's probes from `node`, unless one fails
fn slowest(spec: &ServiceSpec, node: &str, at: i64) -> Option<f64> {
    (0..spec.probes.len())
        .map(|index| latency(spec, index, node, at))
        .try_fold(0.0_f64, |slowest, latency| Some(slowest.max(latency?)))
}

/// min, median and max of the service's latency across the nodes that reach it
fn band(spec: &ServiceSpec, at: i64) -> BandPoint {
    let mut latencies: Vec<f64> = NODES
        .iter()
        .filter_map(|node| slowest(spec, node, at))
        .collect();
    latencies.sort_by(f64::total_cmp);
    let median = match latencies.len() {
        0 => None,
        n if n % 2 == 1 => Some(latencies[n / 2]),
        n => Some((latencies[n / 2 - 1] + latencies[n / 2]) / 2.0),
    };
    (
        Ms(at),
        latencies.first().copied(),
        median,
        latencies.last().copied(),
    )
}
