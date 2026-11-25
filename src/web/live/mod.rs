//! the real fleet, as this node's Prometheus, Loki and Alertmanager hold
//! it. a task builds one [Snapshot] every scrape and publishes it when it
//! changed; every other route asks the stores when a browser does. each
//! answer is parsed where it arrives ([prometheus], [loki],
//! [alertmanager]), and every PromQL is in [promql]

pub mod alertmanager;
pub mod alerts;
mod charts;
pub mod hook;
pub mod logs;
pub mod loki;
pub mod prometheus;
pub mod promql;
pub mod registry;
pub mod snapshot;

#[cfg(test)]
mod test;

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

use futures_util::{Stream, StreamExt, future::join_all};
use log::warn;
use tokio::sync::watch;

use self::{
    alertmanager::{Alertmanager, ClusterAlert},
    alerts::{NoticeLine, Seen, Sources},
    charts::step_for,
    loki::{Direction, Loki},
    prometheus::Prometheus,
    promql::Named,
    registry::Registry,
    snapshot::Answers,
};
use super::{
    query::{LogPageQuery, LogQuery, Refusal, SeriesQuery},
    wire::*,
};

/// how often the snapshot is built: every scrape
const TICK: Duration = Duration::from_secs(15);
/// how long this node's own stores may take to answer
const LOCAL: Duration = Duration::from_secs(10);
/// how long another node's Prometheus may take, before it is left out of
/// who sees an alert
const PEER: Duration = Duration::from_secs(2);
const DAY: i64 = 24 * 3_600_000;
/// how far back logs go: Loki's retention
const LOG_HISTORY: i64 = 30 * DAY;
/// a tail sends at most this many lines at once, the newest
const TAIL_BATCH: usize = 500;

pub struct Live {
    registry: Registry,
    prometheus: Prometheus,
    alertmanager: Alertmanager,
    loki: Loki,
    /// every node's Prometheus, this one's included, in registry order
    peers: Vec<(Hostname, Prometheus)>,
    fleet: watch::Sender<Arc<Snapshot>>,
    /// the summary of every alert seen firing, or found in a notice line,
    /// for when it has resolved and only its labels are left
    summaries: Mutex<HashMap<AlertId, String>>,
}

pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("the clock is past 1970")
        .as_millis() as i64
}

fn unavailable(error: eyre::Report) -> Refusal {
    Refusal::Unavailable(format!("{error:#}"))
}

impl Live {
    pub fn new(registry: Registry) -> Arc<Self> {
        let me = &registry.machine;
        let ports = registry.ports;
        let prometheus = Prometheus::new(registry.url(me, ports.prometheus), LOCAL);
        let alertmanager = Alertmanager::new(registry.url(me, ports.alertmanager), LOCAL);
        let loki = Loki::new(
            format!("{}:{}", registry.machines[me].address, ports.loki),
            LOCAL,
        );
        let peers = registry
            .nodes
            .iter()
            .map(|node| {
                (
                    node.clone(),
                    Prometheus::new(registry.url(node, ports.prometheus), PEER),
                )
            })
            .collect();
        let empty = snapshot::build(&registry, &Answers::default(), now(), Vec::new());
        Arc::new(Self {
            prometheus,
            alertmanager,
            loki,
            peers,
            fleet: watch::Sender::new(Arc::new(empty)),
            summaries: Mutex::new(HashMap::new()),
            registry,
        })
    }

    /// builds a snapshot now, before anyone can read the fleet, then at
    /// every tick after, publishing each that differs from the last in more
    /// than its moment
    pub async fn run(self: &Arc<Self>) {
        self.publish().await;
        let live = self.clone();
        tokio::spawn(async move {
            let mut ticks = tokio::time::interval(TICK);
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticks.tick().await;
            loop {
                ticks.tick().await;
                live.publish().await;
            }
        });
    }

    async fn publish(&self) {
        let snapshot = self.snapshot().await;
        self.fleet.send_if_modified(|published| {
            let moved = Snapshot {
                at: published.at,
                ..snapshot.clone()
            };
            let changed = **published != moved;
            if changed {
                *published = Arc::new(snapshot);
            }
            changed
        });
    }

    pub fn fleet(&self) -> watch::Receiver<Arc<Snapshot>> {
        self.fleet.subscribe()
    }

    async fn snapshot(&self) -> Snapshot {
        let now = now();
        let (answers, alerts) = tokio::join!(self.answers(), self.alert_list(now, DAY));
        let alerts = alerts.unwrap_or_else(|error| {
            warn!("no alerts for the snapshot: {error:?}");
            Vec::new()
        });
        snapshot::build(&self.registry, &answers, now, alerts)
    }

    /// every series of the snapshot this node's Prometheus answers
    async fn answers(&self) -> Answers {
        let asked = Named::SNAPSHOT.map(|series| async move {
            let promql = series.promql(&self.registry, "", 60);
            (series, self.prometheus.query(&promql).await)
        });
        let mut answers = Answers::default();
        for (series, answer) in join_all(asked).await {
            match answer {
                Ok(samples) => {
                    answers.series.insert(series, samples);
                }
                Err(error) => warn!("{} did not answer: {error:#}", series.name()),
            }
        }
        answers
    }

    /// what every node that answers in time says of the alerts that fire,
    /// and of those that fired within `range` ms
    async fn seen(&self, range: i64) -> BTreeMap<Hostname, Seen> {
        let fired = Named::FiredWithin.promql(&self.registry, "", (range / 1_000) as u64);
        let asked = self.peers.iter().map(|(node, prometheus)| {
            let fired = fired.clone();
            async move {
                let (alerts, fired) = tokio::join!(prometheus.alerts(), prometheus.query(&fired));
                match (alerts, fired) {
                    (Ok(alerts), Ok(fired)) => Some((node.clone(), Seen::new(&alerts, &fired))),
                    (Err(error), _) | (_, Err(error)) => {
                        log::debug!("{node} is left out of who sees what: {error:#}");
                        None
                    }
                }
            }
        });
        join_all(asked).await.into_iter().flatten().collect()
    }

    /// the alerts firing now, then those resolved within `range` ms
    async fn alert_list(&self, now: i64, range: i64) -> Result<Vec<Alert>, Refusal> {
        // the history reaches a day further back, for when what resolved
        // within the range began
        let span = range + DAY;
        let step = alerts::step_within(span);
        let history = Named::Firing.promql(&self.registry, "", 60);
        let (firing, history, seen) = tokio::join!(
            self.alertmanager.firing(now),
            self.prometheus
                .range(&history, now - span, now, step / 1_000),
            self.seen(range),
        );
        let history = history.map_err(unavailable)?;
        let firing: Option<Vec<ClusterAlert>> = firing
            .inspect_err(|error| warn!("the alertmanager did not answer: {error:#}"))
            .ok();
        let episodes = alerts::episodes(&history, step);

        {
            let mut summaries = self.summaries.lock().unwrap();
            for alert in firing.iter().flatten() {
                if let Some(summary) = alert.annotations.get("summary") {
                    summaries.insert(alert.fingerprint.clone(), summary.clone());
                }
            }
        }
        let missing = {
            let summaries = self.summaries.lock().unwrap();
            episodes.keys().any(|id| !summaries.contains_key(id))
        };
        if missing {
            self.recall_summaries(now - span, now).await;
        }

        let summaries = self.summaries.lock().unwrap().clone();
        Ok(alerts::list(&Sources {
            now,
            range,
            firing: firing.as_deref(),
            episodes: &episodes,
            step,
            seen: &seen,
            summaries: &summaries,
        }))
    }

    /// every notice line logged over `[from, to]`, epoch ms, for one alert or all
    async fn notices(
        &self,
        alert: Option<&AlertId>,
        from: i64,
        to: i64,
    ) -> Vec<(Hostname, i64, NoticeLine)> {
        let asked = logs::notices(&self.registry, alert)
            .into_iter()
            .map(|logql| async move {
                self.loki
                    .range(
                        &logql,
                        from * 1_000_000,
                        to * 1_000_000,
                        5_000,
                        Direction::Forward,
                    )
                    .await
            });
        let mut notices = Vec::new();
        for answer in join_all(asked).await {
            match answer {
                Ok(entries) => notices.extend(entries.into_iter().filter_map(|entry| {
                    let notice = NoticeLine::find(&entry.line)?;
                    let by = Hostname::from(entry.labels.get("machine")?.as_str());
                    Some((by, entry.ns / 1_000_000, notice))
                })),
                Err(error) => warn!("no notices from loki: {error:#}"),
            }
        }
        notices.sort_by_key(|(_, at, _)| *at);
        notices
    }

    /// summaries of alerts that resolved before this phoebe saw them fire,
    /// from the notices any node logged of them
    async fn recall_summaries(&self, from: i64, to: i64) {
        let notices = self.notices(None, from, to).await;
        let mut summaries = self.summaries.lock().unwrap();
        for (_, _, notice) in notices {
            summaries.entry(notice.alert).or_insert(notice.summary);
        }
    }

    pub async fn alerts(&self, range: i64) -> Result<Vec<Alert>, Refusal> {
        self.alert_list(now(), range).await
    }

    pub async fn alert(&self, id: &AlertId) -> Result<AlertDetail, Refusal> {
        let now = now();
        let alert = self
            .alert_list(now, 7 * DAY)
            .await?
            .into_iter()
            .find(|alert| alert.id == *id)
            .ok_or_else(|| Refusal::Missing(format!("no alert {id}")))?;
        let to = match alert.state {
            AlertState::Firing => now,
            AlertState::Resolved { at } => (at.0 + 10 * 60_000).min(now),
        };
        // the longest a rule waits before it fires
        let from = alert.starts_at.0 - 70 * 60_000;
        let step = alerts::step_within(to - from);
        let on: Vec<String> = alert
            .labels
            .iter()
            .map(|(name, value)| promql::matcher(name, value))
            .collect();
        let history = Named::Alerts.promql(&self.registry, &on.join(", "), 60);

        let nodes = self.peers.iter().map(|(node, prometheus)| {
            let history = history.clone();
            let since = alert.starts_at.0;
            async move {
                match prometheus.range(&history, from, to, step / 1_000).await {
                    Ok(ranges) => alerts::timeline(node, &ranges, since, to, step),
                    Err(error) => {
                        log::debug!("{node} is left out of the timeline: {error:#}");
                        Vec::new()
                    }
                }
            }
        });
        let (timelines, notices, series) = tokio::join!(
            join_all(nodes),
            self.notices(Some(id), from, to),
            self.alert_series(&alert, to),
        );
        let mut timeline: Vec<TimelineEntry> = timelines.into_iter().flatten().collect();
        timeline.extend(
            notices
                .into_iter()
                .filter(|(_, _, notice)| notice.delivered > 0)
                .map(|(by, at, notice)| TimelineEntry::Notified {
                    at: Ms(at),
                    by,
                    resolved: notice.resolved,
                }),
        );
        timeline.sort_by_key(entry_at);
        Ok(AlertDetail {
            alert,
            timeline,
            series,
        })
    }

    /// the chart of what fired the alert, from an hour before it did
    async fn alert_series(&self, alert: &Alert, to: i64) -> Option<Series> {
        let metric = charts::chart_of_rule(&alert.rule)?;
        let of = match metric {
            Metric::Latency => Subject::Service(alert.service.clone()?),
            _ => Subject::Machine(alert.machine.clone()?),
        };
        let from = (alert.starts_at.0 - 3_600_000).max(to - 7 * DAY);
        self.chart(&of, metric, None, from, to)
            .await
            .inspect_err(|refusal| warn!("no chart for alert {}: {refusal:?}", alert.id))
            .ok()
    }

    pub async fn series(&self, query: &SeriesQuery) -> Result<Series, Refusal> {
        let span = query.range.ms();
        let step = step_for(span);
        let to = now().div_euclid(step) * step;
        self.chart(&query.of, query.metric, query.node.as_ref(), to - span, to)
            .await
    }

    /// `metric` of `of` at every step in `(from, to]`
    async fn chart(
        &self,
        of: &Subject,
        metric: Metric,
        node: Option<&Hostname>,
        from: i64,
        to: i64,
    ) -> Result<Series, Refusal> {
        let chart = charts::chart(&self.registry, of, metric)?;
        if let Some(node) = node
            && !self.registry.nodes.contains(node)
        {
            return Err(Refusal::Missing(format!("no node {node}")));
        }
        let step = step_for(to - from);
        let grid = charts::grid(from, to, step);
        let (Some(&start), Some(&end)) = (grid.first(), grid.last()) else {
            return Err(Refusal::Invalid("the range holds no point".into()));
        };
        let asked = chart.lines.iter().map(|(name, series)| {
            let promql = series.promql(&self.registry, &chart.on, (step / 1_000) as u64);
            async move {
                self.prometheus
                    .range(&promql, start, end, step / 1_000)
                    .await
                    .map(|ranges| (*name, ranges))
            }
        });
        let answers = join_all(asked)
            .await
            .into_iter()
            .collect::<eyre::Result<Vec<_>>>()
            .map_err(unavailable)?;
        let data = match (chart.by_node, node) {
            (false, _) => charts::lines(answers, &grid),
            (true, Some(node)) => charts::node_line(node, &answers[0].1, &grid),
            (true, None) => charts::band(&answers[0].1, &grid),
        };
        Ok(Series {
            of: of.clone(),
            metric,
            from: Ms(from),
            to: Ms(to),
            data,
        })
    }

    pub async fn logs(&self, query: &LogPageQuery) -> Result<LogPage, Refusal> {
        let logql = logs::logql(&self.registry, &query.lines)?;
        let limit = query.limit.clamp(1, LogPageQuery::MAX_LIMIT);
        let cursor = query
            .before
            .as_deref()
            .map(logs::Cursor::parse)
            .transpose()?;
        let end = cursor.map_or(now() * 1_000_000 + 1, |cursor| cursor.ns + 1);
        let shown = cursor.map_or(0, |cursor| cursor.shown);
        let entries = self
            .loki
            .range(
                &logql,
                end - LOG_HISTORY * 1_000_000,
                end,
                limit + shown + 1,
                Direction::Backward,
            )
            .await
            .map_err(unavailable)?;
        Ok(logs::page(entries, cursor, limit))
    }

    pub async fn tail(
        &self,
        query: &LogQuery,
    ) -> Result<impl Stream<Item = LogEvent> + Send + use<>, Refusal> {
        let logql = logs::logql(&self.registry, query)?;
        let lines = self
            .loki
            .tail(&logql, now() * 1_000_000)
            .await
            .map_err(unavailable)?;
        Ok(lines.map(|entries| LogEvent::Lines {
            lines: logs::batch(entries, TAIL_BATCH),
        }))
    }
}

fn entry_at(entry: &TimelineEntry) -> Ms {
    match entry {
        TimelineEntry::Pending { at, .. }
        | TimelineEntry::Firing { at, .. }
        | TimelineEntry::Resolved { at, .. }
        | TimelineEntry::Notified { at, .. } => *at,
    }
}
