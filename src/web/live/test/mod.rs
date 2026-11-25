//! the real fleet's answers, recorded from a node's stores by [record],
//! parsed into the wire types as the live source parses them

mod answers;
mod record;

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;

use super::{
    alertmanager,
    alerts::{self, Seen, Sources},
    prometheus,
    promql::Named,
    registry::Registry,
    snapshot::{self, Answers},
};
use crate::web::wire::{Alert, Hostname, Snapshot};

const RECORDED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/web/live/test/recorded");

/// what [record] wrote, read back
pub struct Recorded {
    pub registry: Registry,
    pub now: i64,
    pub history_step: i64,
}

impl Recorded {
    pub const DAY: i64 = 24 * 3_600_000;

    pub fn file(name: &str) -> String {
        std::fs::read_to_string(format!("{RECORDED}/{name}"))
            .unwrap_or_else(|error| panic!("{name}: {error}"))
    }

    pub fn load() -> Self {
        #[derive(Deserialize)]
        struct Meta {
            now: i64,
            history_step: i64,
        }
        let meta: Meta = serde_json::from_str(&Self::file("meta.json")).unwrap();
        Self {
            registry: Registry::parse(Self::file("fleet.json").as_bytes()).unwrap(),
            now: meta.now,
            history_step: meta.history_step,
        }
    }

    pub fn answers(&self) -> Answers {
        Answers {
            series: Named::SNAPSHOT
                .into_iter()
                .map(|series| {
                    let body = Self::file(&format!("prometheus/{}.json", series.name()));
                    (series, prometheus::vector(&body).unwrap())
                })
                .collect(),
        }
    }

    pub fn alerts(&self) -> Vec<Alert> {
        let firing =
            alertmanager::firing(&Self::file("alertmanager/alerts.json"), self.now).unwrap();
        let history = prometheus::matrix(&Self::file("prometheus/history.json")).unwrap();
        let episodes = alerts::episodes(&history, self.history_step);
        let seen: BTreeMap<Hostname, Seen> = self
            .registry
            .nodes
            .iter()
            .map(|node| {
                let rules =
                    prometheus::rule_alerts(&Self::file(&format!("nodes/{node}/alerts.json")))
                        .unwrap();
                let fired =
                    prometheus::vector(&Self::file(&format!("nodes/{node}/fired.json"))).unwrap();
                (node.clone(), Seen::new(&rules, &fired))
            })
            .collect();
        alerts::list(&Sources {
            now: self.now,
            range: Self::DAY,
            firing: Some(&firing),
            episodes: &episodes,
            step: self.history_step,
            seen: &seen,
            summaries: &HashMap::new(),
        })
    }

    pub fn snapshot(&self) -> Snapshot {
        snapshot::build(&self.registry, &self.answers(), self.now, self.alerts())
    }
}
