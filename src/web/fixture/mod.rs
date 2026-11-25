//! fixture mode: the site over a simulated fleet whose clock can be moved
//! to any moment, so the web app can be built and tested with no fleet at
//! all. every answer is a function of the moment alone ([story]), so a
//! moment looks the same however the clock got there.

mod fleet;
mod logs;
mod signal;
mod story;
mod world;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Query, State},
    routing::post,
};
use futures_util::Stream;
use serde::Deserialize;
use tokio::{sync::watch, time::Instant};

pub use self::story::START;
use self::{
    logs::Filter,
    story::{HISTORY, HOUR, STEP},
};
use super::{
    query::{LogPageQuery, LogQuery, Refusal, SeriesQuery},
    wire::{Alert, AlertDetail, AlertId, FixtureClock, LogEvent, LogPage, Series, Snapshot},
};

pub struct Fixture {
    clock: Mutex<Clock>,
    fleet: watch::Sender<Arc<Snapshot>>,
    /// tails look again after every move
    moved: watch::Sender<()>,
}

struct Clock {
    /// where the timeline was at `set`, in milliseconds after [START]
    at: f64,
    set: Instant,
    /// timeline milliseconds per real millisecond; 0 holds the clock
    speed: f64,
}

impl Clock {
    /// where the timeline is, in milliseconds after [START]
    fn at(&self) -> f64 {
        self.at + self.speed * self.set.elapsed().as_secs_f64() * 1_000.0
    }
}

/// a tail sends at most this many lines at once, the newest
const TAIL_BATCH: usize = 500;

impl Fixture {
    /// the fleet at the start of its timeline, its clock running at `speed`
    pub fn new(speed: f64) -> Arc<Self> {
        Arc::new(Self {
            clock: Mutex::new(Clock {
                at: 0.0,
                set: Instant::now(),
                speed: speed.max(0.0),
            }),
            fleet: watch::Sender::new(Arc::new(world::snapshot(START))),
            moved: watch::Sender::new(()),
        })
    }

    /// publishes a snapshot at every scrape the running clock reaches
    pub fn run(self: &Arc<Self>) {
        let fixture = self.clone();
        let mut moved = self.moved.subscribe();
        tokio::spawn(async move {
            loop {
                let wait = fixture
                    .until_next_step()
                    .unwrap_or(Duration::from_secs(3_600));
                tokio::select! {
                    () = tokio::time::sleep(wait) => {}
                    changed = moved.changed() => if changed.is_err() {
                        return;
                    },
                }
                fixture.publish();
            }
        });
    }

    /// the moment it is in the simulated fleet, to the millisecond
    fn now(&self) -> i64 {
        START + self.clock.lock().unwrap().at() as i64
    }

    /// the last scrape: the moment the fleet, its series and alerts show
    fn moment(&self) -> i64 {
        self.now().div_euclid(STEP) * STEP
    }

    /// how long until the running clock reaches the next scrape
    fn until_next_step(&self) -> Option<Duration> {
        let clock = self.clock.lock().unwrap();
        if clock.speed == 0.0 {
            return None;
        }
        let at = clock.at();
        let next = ((at / STEP as f64).floor() + 1.0) * STEP as f64;
        Some(
            Duration::from_secs_f64((next - at) / clock.speed / 1_000.0) + Duration::from_millis(1),
        )
    }

    fn publish(&self) {
        let snapshot = world::snapshot(self.moment());
        self.fleet.send_if_modified(|published| {
            let changed = **published != snapshot;
            if changed {
                *published = Arc::new(snapshot);
            }
            changed
        });
    }

    pub fn fleet(&self) -> watch::Receiver<Arc<Snapshot>> {
        self.fleet.subscribe()
    }

    /// moves the timeline to `t` seconds, or keeps it where it is, and runs
    /// it at `speed` from there, answering once the stream shows that moment
    pub fn seek(&self, t: Option<f64>, speed: f64) -> FixtureClock {
        let status = {
            let mut clock = self.clock.lock().unwrap();
            clock.at = t.map_or_else(|| clock.at(), |t| t.max(0.0) * 1_000.0);
            clock.set = Instant::now();
            clock.speed = speed.max(0.0);
            FixtureClock {
                t: clock.at / 1_000.0,
                speed: clock.speed,
                now: super::wire::Ms(START + clock.at as i64),
            }
        };
        self.publish();
        self.moved.send_replace(());
        status
    }

    /// `POST /fixture/seek?[t=<seconds>][&speed=<x>]`: moves the timeline,
    /// or keeps it where it is with no `t`, and holds it there, or runs it
    /// at `speed`
    pub fn routes(self: &Arc<Self>) -> Router {
        #[derive(Deserialize)]
        struct Seek {
            t: Option<f64>,
            #[serde(default)]
            speed: f64,
        }

        async fn seek(
            State(fixture): State<Arc<Fixture>>,
            Query(seek): Query<Seek>,
        ) -> Json<FixtureClock> {
            Json(fixture.seek(seek.t, seek.speed))
        }

        Router::new()
            .route("/fixture/seek", post(seek))
            .with_state(self.clone())
    }

    pub fn series(&self, query: &SeriesQuery) -> Result<Series, Refusal> {
        world::series_at(query, self.moment())
    }

    pub fn alerts(&self, range: i64) -> Vec<Alert> {
        world::alerts(range, self.moment())
    }

    pub fn alert(&self, id: &AlertId) -> Result<AlertDetail, Refusal> {
        world::alert_detail(id, self.moment())
    }

    pub fn logs(&self, query: &LogPageQuery) -> Result<LogPage, Refusal> {
        let filter = filter(&query.lines)?;
        let limit = query.limit.clamp(1, LogPageQuery::MAX_LIMIT);
        let now = self.now();
        let upper = match &query.before {
            Some(before) => cursor(before)?.min((now, u64::MAX)),
            None => (now, u64::MAX),
        };
        let floor = now - HISTORY;

        // newest first, an hour at a time, one line past the page to know
        // whether another follows
        let mut lines = Vec::new();
        let mut to = upper.0;
        while to >= floor && lines.len() <= limit {
            let from = (to - HOUR + 1).max(floor);
            let window = logs::between(from, to, &filter);
            lines.extend(window.into_iter().rev().filter(|entry| entry.key() < upper));
            to = from - 1;
        }
        let more = lines.len() > limit;
        lines.truncate(limit);
        Ok(LogPage {
            before: more
                .then(|| {
                    lines
                        .last()
                        .map(|entry| format!("{}-{:016x}", entry.line.at.0, entry.id))
                })
                .flatten(),
            lines: lines.into_iter().map(|entry| entry.line).collect(),
        })
    }

    /// the lines that arrive from now on, each time the clock moves past them
    pub fn tail(
        self: &Arc<Self>,
        query: &LogQuery,
    ) -> Result<impl Stream<Item = LogEvent> + use<>, Refusal> {
        let filter = filter(query)?;
        let state = (self.clone(), self.moved.subscribe(), self.now(), filter);
        Ok(futures_util::stream::unfold(
            state,
            |(fixture, mut moved, mut last, filter)| async move {
                loop {
                    tokio::select! {
                        () = tokio::time::sleep(Duration::from_secs(1)) => {}
                        changed = moved.changed() => if changed.is_err() {
                            return None;
                        },
                    }
                    let now = fixture.now();
                    // moved back: the tail goes on from the new moment
                    if now <= last {
                        last = now;
                        continue;
                    }
                    let mut lines = logs::between(last + 1, now, &filter);
                    last = now;
                    if lines.is_empty() {
                        continue;
                    }
                    lines.drain(..lines.len().saturating_sub(TAIL_BATCH));
                    let lines = lines.into_iter().map(|entry| entry.line).collect();
                    return Some((LogEvent::Lines { lines }, (fixture, moved, last, filter)));
                }
            },
        ))
    }
}

/// a page's `before`: the time and id of the oldest line it held
fn cursor(before: &str) -> Result<(i64, u64), Refusal> {
    let invalid = || Refusal::Invalid(format!("`before` is not a page's: {before}"));
    let (at, id) = before.split_once('-').ok_or_else(invalid)?;
    Ok((
        at.parse().map_err(|_| invalid())?,
        u64::from_str_radix(id, 16).map_err(|_| invalid())?,
    ))
}

/// the lines a query wants, every name it gives checked against the fleet
fn filter(query: &LogQuery) -> Result<Filter, Refusal> {
    let machine = query
        .machine
        .as_ref()
        .map(|name| {
            fleet::machine(&name.0)
                .map(|machine| machine.name)
                .ok_or_else(|| Refusal::Missing(format!("no machine {name}")))
        })
        .transpose()?;
    let mut filter = Filter {
        machine,
        unit: query.unit.clone(),
        hosts: Vec::new(),
        q: query.q.as_ref().map(|q| q.to_lowercase()),
    };
    if let Some(name) = &query.service {
        let service = fleet::service(&name.0)
            .ok_or_else(|| Refusal::Missing(format!("no service {name}")))?;
        if machine.is_some_and(|machine| machine != service.machine) {
            return Err(Refusal::Invalid(format!(
                "{name} runs on {}, not {}",
                service.machine,
                machine.unwrap_or_default()
            )));
        }
        match service.unit {
            Some(_) if query.unit.is_some() => {
                return Err(Refusal::Invalid(
                    "give a unit or a service, not both".into(),
                ));
            }
            Some(unit) => {
                filter.machine = Some(service.machine);
                filter.unit = Some(unit.into());
            }
            None => filter.hosts = service.probes.iter().map(|probe| probe.host()).collect(),
        }
    }
    Ok(filter)
}
