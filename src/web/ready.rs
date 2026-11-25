//! whether this node can serve the app: tailscale in the tailnet and in
//! touch with its coordination server and relays, and this node's own
//! Prometheus, Loki and Alertmanager answering. no other node is asked, so
//! the last node standing keeps serving.

use std::{future::Future, time::Duration};

use futures_util::future::join_all;
use log::{info, warn};
use tokio::{
    sync::watch,
    time::{MissedTickBehavior, interval, timeout},
};

use super::{live::registry::Registry, tailnet::Tailnet};

/// how often the node is probed, and how many probes in a row change its
/// readiness
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pace {
    pub interval: Duration,
    /// passes in a row that make it ready
    pub up_after: u8,
    /// fails in a row that make it no longer ready
    pub down_after: u8,
}

impl Pace {
    /// a store that restarts answers again within a few seconds, which five
    /// fails two seconds apart ride out
    pub const DEFAULT: Self = Self {
        interval: Duration::from_secs(2),
        up_after: 3,
        down_after: 5,
    };
}

/// consecutive probes since readiness last changed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Down { passes: u8 },
    Up { fails: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Rose,
    Fell,
}

impl Readiness {
    pub const START: Self = Self::Down { passes: 0 };

    /// up after `up_after` passes in a row, down after `down_after` fails in
    /// a row
    pub fn step(self, passed: bool, pace: &Pace) -> (Self, Option<Edge>) {
        match (self, passed) {
            (Self::Down { passes }, true) => {
                let passes = passes.saturating_add(1);
                if passes >= pace.up_after {
                    (Self::Up { fails: 0 }, Some(Edge::Rose))
                } else {
                    (Self::Down { passes }, None)
                }
            }
            (Self::Down { .. }, false) => (Self::START, None),
            (Self::Up { .. }, true) => (Self::Up { fails: 0 }, None),
            (Self::Up { fails }, false) => {
                let fails = fails.saturating_add(1);
                if fails >= pace.down_after {
                    (Self::START, Some(Edge::Fell))
                } else {
                    (Self::Up { fails }, None)
                }
            }
        }
    }

    pub const fn is_up(self) -> bool {
        matches!(self, Self::Up { .. })
    }
}

/// runs `probe` every interval, each within the interval, and keeps `ready`
/// saying whether the node is ready, until nobody listens
pub async fn watch<P, F>(pace: Pace, mut probe: P, ready: watch::Sender<bool>)
where
    P: FnMut() -> F,
    F: Future<Output = Result<(), String>>,
{
    let mut state = Readiness::START;
    let mut trouble = None;
    let mut ticks = interval(pace.interval);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);

    while !ready.is_closed() {
        ticks.tick().await;
        let verdict = timeout(pace.interval, probe())
            .await
            .unwrap_or_else(|_| Err(format!("no answer within {:?}", pace.interval)));

        if let Err(reason) = &verdict
            && trouble.as_ref() != Some(reason)
        {
            warn!("not ready to serve: {reason}");
        }
        trouble = verdict.err();

        let (next, edge) = state.step(trouble.is_none(), &pace);
        state = next;
        match edge {
            Some(Edge::Rose) => info!("ready to serve"),
            Some(Edge::Fell) => warn!("no longer ready to serve"),
            None => continue,
        }
        ready.send_replace(state.is_up());
    }
}

/// this node's stores, by where each says it is ready
#[derive(Clone, Debug)]
pub struct Stores {
    client: reqwest::Client,
    ready: Vec<(&'static str, String)>,
}

impl Stores {
    pub fn of(registry: &Registry) -> Self {
        let me = &registry.machine;
        let ports = registry.ports;
        Self::at(vec![
            (
                "prometheus",
                format!("{}/-/ready", registry.url(me, ports.prometheus)),
            ),
            ("loki", format!("{}/ready", registry.url(me, ports.loki))),
            (
                "alertmanager",
                format!("{}/-/ready", registry.url(me, ports.alertmanager)),
            ),
        ])
    }

    /// stores that say they are ready at these addresses
    pub fn at(ready: Vec<(&'static str, String)>) -> Self {
        Self {
            client: reqwest::Client::new(),
            ready,
        }
    }

    /// passes when every store says it is ready within the probe's interval
    pub async fn answer(&self) -> Result<(), String> {
        let asked = self.ready.iter().map(|(name, url)| async move {
            let answered = self
                .client
                .get(url)
                .timeout(Pace::DEFAULT.interval)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status);
            match answered {
                Ok(_) => None,
                Err(error) if error.is_status() => Some(format!(
                    "this node's {name} is not ready ({})",
                    error
                        .status()
                        .map_or_else(String::new, |status| status.to_string())
                )),
                Err(_) => Some(format!("this node's {name} does not answer")),
            }
        });
        match join_all(asked).await.into_iter().flatten().next() {
            Some(trouble) => Err(trouble),
            None => Ok(()),
        }
    }
}

/// passes when tailscale finds no trouble and every store is ready
pub async fn probe(stores: &Stores) -> Result<(), String> {
    let (tailnet, answered) = tokio::join!(Tailnet::read(), stores.answer());
    match tailnet {
        None => Err("tailscale does not answer".to_owned()),
        Some(tailnet) => tailnet.trouble().map_or(answered, Err),
    }
}
