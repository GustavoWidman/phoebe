//! which LogQL a log query is, and pages of what Loki answers. the labels
//! come from the data plane: `machine`, `unit` and `level` on a line from
//! the journal, `machine` and `file` on one from a darwin log file

use super::{
    alerts::NoticeLine,
    loki::Entry,
    promql::{escape_regex, matcher, quote},
    registry::Registry,
};
use crate::web::{
    query::{LogQuery, Refusal},
    wire::{AlertId, LogLine, LogPage, Os},
};

/// phoebe web's own lines, where each platform's module sends them
const OWN_UNIT: &str = "phoebe-web.service";
const OWN_FILE: &str = "/var/log/phoebe-web.log";

/// the LogQL of the lines a query wants, every name it gives checked
/// against the registry
pub fn logql(registry: &Registry, query: &LogQuery) -> Result<String, Refusal> {
    let mut machine = match &query.machine {
        Some(name) => Some(
            registry
                .machine(name)
                .map(|entry| (name, entry.os))
                .ok_or_else(|| Refusal::Missing(format!("no machine {name}")))?,
        ),
        None => None,
    };
    let mut unit = query.unit.clone();
    let mut hosts: Vec<String> = Vec::new();
    if let Some(id) = &query.service {
        let service = registry
            .service(id)
            .ok_or_else(|| Refusal::Missing(format!("no service {id}")))?;
        if let Some((machine, _)) = machine
            && *machine != id.machine
        {
            return Err(Refusal::Invalid(format!("{id} does not run on {machine}")));
        }
        match &service.unit {
            Some(_) if query.unit.is_some() => {
                return Err(Refusal::Invalid(
                    "give a unit or a service, not both".into(),
                ));
            }
            Some(own) => {
                let entry = registry
                    .machine(&id.machine)
                    .expect("a registry's services run on its machines");
                machine = Some((&id.machine, entry.os));
                unit = Some(own.clone());
            }
            None => {
                hosts = service
                    .probes
                    .iter()
                    .map(|probe| escape_regex(probe.host()))
                    .collect();
            }
        }
    }

    let mut matchers = vec![match machine {
        Some((name, _)) => matcher("machine", &name.0),
        None => r#"machine=~".+""#.to_owned(),
    }];
    if let Some(unit) = &unit {
        matchers.push(match machine {
            // a launchd job writes to a file of its own name, when it writes
            // to one at all
            Some((_, Os::Darwin)) => {
                let stem = unit.rsplit('.').next().unwrap_or(unit);
                format!(
                    "file=~{}",
                    quote(&format!(".*/{}[^/]*", escape_regex(stem)))
                )
            }
            _ => matcher("unit", unit),
        });
    }
    let mut logql = format!("{{{}}}", matchers.join(", "));
    if !hosts.is_empty() {
        logql += &format!(" |~ {}", quote(&format!("(?i)({})", hosts.join("|"))));
    }
    if let Some(q) = query.q.as_deref().filter(|q| !q.is_empty()) {
        logql += &format!(" |~ {}", quote(&format!("(?i){}", escape_regex(q))));
    }
    Ok(logql)
}

/// the LogQL of phoebe's own notice lines on every node, one per platform
/// the nodes run, for one alert or all of them
pub fn notices(registry: &Registry, alert: Option<&AlertId>) -> Vec<String> {
    let filter = match alert {
        Some(id) => format!("{}{{\"alert\":\"{id}\"", NoticeLine::MARK),
        None => format!("{}{{\"alert\":", NoticeLine::MARK),
    };
    let mut platforms: Vec<Os> = registry
        .nodes
        .iter()
        .filter_map(|node| registry.machine(node).map(|entry| entry.os))
        .collect();
    platforms.sort_by_key(|os| *os == Os::Darwin);
    platforms.dedup();
    platforms
        .into_iter()
        .map(|os| {
            let selector = match os {
                Os::NixOs => matcher("unit", OWN_UNIT),
                Os::Darwin => matcher("file", OWN_FILE),
            };
            format!("{{{selector}}} |= {}", quote(&filter))
        })
        .collect()
}

/// where a page ends: the time of its oldest line, and how many lines at
/// exactly that time it and the pages before it held
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub ns: i64,
    pub shown: usize,
}

impl Cursor {
    pub fn parse(before: &str) -> Result<Self, Refusal> {
        let invalid = || Refusal::Invalid(format!("`before` is not a page's: {before}"));
        let (ns, shown) = before.split_once('.').ok_or_else(invalid)?;
        Ok(Self {
            ns: ns.parse().map_err(|_| invalid())?,
            shown: shown.parse().map_err(|_| invalid())?,
        })
    }

    fn text(self) -> String {
        format!("{}.{}", self.ns, self.shown)
    }
}

/// newest first, ties in a fixed order so a page boundary falls the same
/// way every time
fn newest_first(entries: &mut [Entry]) {
    entries.sort_by(|a, b| {
        b.ns.cmp(&a.ns)
            .then_with(|| a.labels.cmp(&b.labels))
            .then_with(|| a.line.cmp(&b.line))
    });
}

/// a page of `limit` lines from what Loki answered for the page before
/// `cursor`, asked with room for the lines already shown and one more
pub fn page(mut entries: Vec<Entry>, cursor: Option<Cursor>, limit: usize) -> LogPage {
    newest_first(&mut entries);
    let skip = cursor.map_or(0, |cursor| {
        entries
            .iter()
            .take_while(|entry| entry.ns == cursor.ns)
            .count()
            .min(cursor.shown)
    });
    let rest = &entries[skip..];
    let lines: Vec<&Entry> = rest.iter().take(limit).collect();
    let before = (rest.len() > limit)
        .then(|| lines.last())
        .flatten()
        .map(|oldest| {
            let at_oldest = lines.iter().filter(|entry| entry.ns == oldest.ns).count();
            let carried = match cursor {
                Some(cursor) if cursor.ns == oldest.ns => cursor.shown,
                _ => 0,
            };
            Cursor {
                ns: oldest.ns,
                shown: carried + at_oldest,
            }
            .text()
        });
    LogPage {
        lines: lines.into_iter().map(Entry::log_line).collect(),
        before,
    }
}

/// a tail's batch, oldest first, at most `most` of the newest
pub fn batch(entries: Vec<Entry>, most: usize) -> Vec<LogLine> {
    let skip = entries.len().saturating_sub(most);
    entries
        .into_iter()
        .skip(skip)
        .map(|entry| entry.log_line())
        .collect()
}
