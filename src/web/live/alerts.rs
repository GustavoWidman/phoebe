//! alerts as the fleet fired them: what fires now from the Alertmanager
//! cluster, what fired before from this node's `ALERTS` series, which nodes
//! saw each from theirs, and each one's timeline

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use super::{
    alertmanager::ClusterAlert,
    prometheus::{Labels, Range, RuleAlert, RuleState, Sample},
};
use crate::web::wire::{
    Alert, AlertId, AlertState, Hostname, Ms, ServiceName, Severity, TimelineEntry,
};

/// how often every node evaluates its rules
pub const EVALUATION: i64 = 15_000;

/// the finest step, in whole evaluations, that keeps a range of `span` ms
/// under the 11,000 points Prometheus answers per series
pub fn step_within(span: i64) -> i64 {
    let evaluations = (span.max(1) as u64).div_ceil(10_000 * EVALUATION as u64) as i64;
    evaluations * EVALUATION
}

/// an alert's identity from its labels, as Alertmanager fingerprints it:
/// FNV-1a over the sorted names and values, each followed by 0xff
pub fn fingerprint(labels: &Labels) -> AlertId {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (name, value) in labels {
        for byte in name
            .bytes()
            .chain([0xff])
            .chain(value.bytes())
            .chain([0xff])
        {
            hash = (hash ^ byte as u64).wrapping_mul(0x0100_0000_01b3);
        }
    }
    AlertId(format!("{hash:016x}"))
}

/// the labels an `ALERTS` series carries for its alert: its own, without
/// the metric's name and the state
pub fn alert_labels(series: &Labels) -> Labels {
    series
        .iter()
        .filter(|(name, _)| *name != "__name__" && *name != "alertstate")
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

/// the last stretch an alert fired without a break, as one node's `ALERTS`
/// shows it
#[derive(Clone, Debug, PartialEq)]
pub struct Episode {
    pub labels: Labels,
    /// its first and last firing sample, epoch ms
    pub first: i64,
    pub last: i64,
}

/// each alert's latest episode in a range of `ALERTS{alertstate="firing"}`
/// sampled every `step` ms
pub fn episodes(history: &[Range], step: i64) -> BTreeMap<AlertId, Episode> {
    let mut episodes = BTreeMap::new();
    for series in history {
        let times: Vec<i64> = series
            .points
            .iter()
            .filter(|(_, value)| value.is_some())
            .map(|(t, _)| *t)
            .collect();
        let Some(&last) = times.last() else {
            continue;
        };
        // walk back from the last sample until a gap longer than a missed
        // evaluation or two
        let mut first = last;
        for t in times.iter().rev().skip(1) {
            if first - t > 2 * step.max(EVALUATION) {
                break;
            }
            first = *t;
        }
        let labels = alert_labels(&series.labels);
        let id = fingerprint(&labels);
        let episode = Episode {
            labels,
            first,
            last,
        };
        match episodes.get(&id) {
            Some(Episode { last: seen, .. }) if *seen >= last => {}
            _ => {
                episodes.insert(id, episode);
            }
        }
    }
    episodes
}

/// what one node's Prometheus says of the fleet's alerts
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seen {
    pub firing: BTreeSet<AlertId>,
    /// every alert it fired within the range asked about
    pub fired: BTreeSet<AlertId>,
}

impl Seen {
    pub fn new(alerts: &[RuleAlert], fired: &[Sample]) -> Self {
        Self {
            firing: alerts
                .iter()
                .filter(|alert| alert.state == RuleState::Firing)
                .map(|alert| fingerprint(&alert.labels))
                .collect(),
            fired: fired
                .iter()
                .map(|sample| fingerprint(&alert_labels(&sample.labels)))
                .collect(),
        }
    }
}

/// everything an alert list is made of
pub struct Sources<'a> {
    pub now: i64,
    /// how far back resolved alerts reach
    pub range: i64,
    /// the cluster's firing alerts; unset when the Alertmanager did not
    /// answer, and this node's own view stands in
    pub firing: Option<&'a [ClusterAlert]>,
    pub episodes: &'a BTreeMap<AlertId, Episode>,
    /// the step `episodes` were sampled at
    pub step: i64,
    /// by node, for each that answered
    pub seen: &'a BTreeMap<Hostname, Seen>,
    pub summaries: &'a HashMap<AlertId, String>,
}

/// the alerts that fire, newest first, then those resolved within the
/// range, newest first
pub fn list(sources: &Sources) -> Vec<Alert> {
    let Sources {
        now,
        range,
        firing,
        episodes,
        step,
        seen,
        summaries,
    } = *sources;
    let seen_by = |id: &AlertId, firing: bool| -> Vec<Hostname> {
        seen.iter()
            .filter(|(_, seen)| {
                if firing {
                    seen.firing.contains(id)
                } else {
                    seen.fired.contains(id)
                }
            })
            .map(|(node, _)| node.clone())
            .collect()
    };
    let summary = |id: &AlertId, labels: &Labels| {
        summaries
            .get(id)
            .cloned()
            .unwrap_or_else(|| fallback_summary(labels))
    };

    let mut fires: Vec<Alert> = Vec::new();
    let mut resolved: Vec<Alert> = Vec::new();
    for alert in firing.unwrap_or_default() {
        let summary = alert
            .annotations
            .get("summary")
            .cloned()
            .unwrap_or_else(|| summary(&alert.fingerprint, &alert.labels));
        fires.push(make(
            alert.fingerprint.clone(),
            &alert.labels,
            AlertState::Firing,
            alert.starts_at,
            summary,
            seen_by(&alert.fingerprint, true),
        ));
    }
    let firing_ids: BTreeSet<&AlertId> = fires.iter().map(|alert| &alert.id).collect();
    let mut late = Vec::new();
    for (id, episode) in episodes {
        if firing_ids.contains(id) {
            continue;
        }
        // still firing here, though the cluster has not heard of it yet, or
        // did not answer
        if now - episode.last <= 2 * step.max(EVALUATION) {
            late.push(make(
                id.clone(),
                &episode.labels,
                AlertState::Firing,
                Ms(episode.first),
                summary(id, &episode.labels),
                seen_by(id, true),
            ));
            continue;
        }
        let at = episode.last + EVALUATION;
        if at < now - range {
            continue;
        }
        resolved.push(make(
            id.clone(),
            &episode.labels,
            AlertState::Resolved { at: Ms(at) },
            Ms(episode.first),
            summary(id, &episode.labels),
            seen_by(id, false),
        ));
    }
    fires.extend(late);
    fires.sort_by_key(|alert| std::cmp::Reverse(alert.starts_at));
    resolved.sort_by_key(|alert| match alert.state {
        AlertState::Resolved { at } => std::cmp::Reverse(at),
        AlertState::Firing => std::cmp::Reverse(Ms(i64::MAX)),
    });
    fires.extend(resolved);
    fires
}

fn make(
    id: AlertId,
    labels: &Labels,
    state: AlertState,
    starts_at: Ms,
    summary: String,
    seen_by: Vec<Hostname>,
) -> Alert {
    let label = |name: &str| labels.get(name).map(String::as_str);
    Alert {
        id,
        rule: label("alertname").unwrap_or("unknown").to_owned(),
        severity: match label("severity") {
            Some("page") => Severity::Page,
            _ => Severity::Info,
        },
        state,
        starts_at,
        machine: label("machine").map(Hostname::from),
        service: label("service").map(ServiceName::from),
        summary,
        seen_by,
        labels: labels.clone(),
    }
}

/// what an alert is about when its own summary is lost: it resolved
/// before this phoebe started, and no notice of it was ever logged
fn fallback_summary(labels: &Labels) -> String {
    let rule = labels.get("alertname").map_or("an alert", String::as_str);
    match (labels.get("service"), labels.get("machine")) {
        (Some(service), _) => format!("{rule} for {service}"),
        (None, Some(machine)) => format!("{rule} on {machine}"),
        (None, None) => rule.to_owned(),
    }
}

/// one node's pending, firing and resolved for an alert, from its `ALERTS`
/// of every state over `[from, to]`, sampled every `step` ms
pub fn timeline(node: &Hostname, history: &[Range], to: i64, step: i64) -> Vec<TimelineEntry> {
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum State {
        Pending,
        Firing,
    }
    let mut states: BTreeMap<i64, State> = BTreeMap::new();
    for series in history {
        let state = match series.labels.get("alertstate").map(String::as_str) {
            Some("firing") => State::Firing,
            Some("pending") => State::Pending,
            _ => continue,
        };
        for (t, value) in &series.points {
            if value.is_some() {
                let held = states.entry(*t).or_insert(state);
                *held = (*held).max(state);
            }
        }
    }

    let gap = 2 * step.max(EVALUATION);
    let mut entries = Vec::new();
    let mut previous: Option<(i64, State)> = None;
    let close = |entries: &mut Vec<TimelineEntry>, (at, state): (i64, State)| {
        if state == State::Firing {
            entries.push(TimelineEntry::Resolved {
                at: Ms(at + EVALUATION),
                node: node.clone(),
            });
        }
    };
    for (&t, &state) in &states {
        let continues = previous.is_some_and(|(at, _)| t - at <= gap);
        if !continues && let Some(previous) = previous {
            close(&mut entries, previous);
        }
        let was = previous.filter(|_| continues).map(|(_, state)| state);
        if was != Some(state) {
            let at = Ms(t);
            entries.push(match state {
                State::Pending => TimelineEntry::Pending {
                    at,
                    node: node.clone(),
                },
                State::Firing => TimelineEntry::Firing {
                    at,
                    node: node.clone(),
                },
            });
        }
        previous = Some((t, state));
    }
    if let Some(previous) = previous
        && to - previous.0 > gap
    {
        close(&mut entries, previous);
    }
    entries
}

/// the line phoebe logs for each alert of every notice the hook gets, which
/// every node's Loki ends up holding
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoticeLine {
    pub alert: AlertId,
    pub resolved: bool,
    pub rule: String,
    pub summary: String,
    /// the devices it was pushed to, none unless it pages
    pub devices: usize,
    /// those that took it
    pub delivered: usize,
}

impl NoticeLine {
    /// what precedes the line's JSON, for LogQL to find it by
    pub const MARK: &str = "notice ";

    /// the notice in a log line, wherever the logger put it
    pub fn find(line: &str) -> Option<Self> {
        let start = line.find(concat!("notice ", "{\"alert\":"))?;
        serde_json::Deserializer::from_str(&line[start + Self::MARK.len()..])
            .into_iter()
            .next()?
            .ok()
    }
}
