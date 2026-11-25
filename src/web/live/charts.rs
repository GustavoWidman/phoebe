//! which series a chart is, and its points on a fixed grid

use super::{prometheus::Range, promql::Named, registry::Registry};
use crate::web::{
    query::Refusal,
    wire::{BandPoint, Hostname, Line, Metric, Ms, Point, SeriesData, Subject},
};

/// points in a chart: about this many, whatever its range
const POINTS: i64 = 240;
/// the scrape interval, the finest step worth asking for
const SCRAPE: i64 = 15_000;

/// the step that puts about [POINTS] across `span` ms, whole scrapes
pub fn step_for(span: i64) -> i64 {
    (span / POINTS).div_euclid(SCRAPE).max(1) * SCRAPE
}

/// what to ask Prometheus for one chart
pub struct Chart {
    /// each line's name and series
    pub lines: Vec<(&'static str, Named)>,
    /// the label matchers that narrow every series to the subject
    pub on: String,
    /// a service's latency, which is a line per prober
    pub by_node: bool,
}

/// the chart of `metric` of `of`, if the registry has the subject and the
/// subject has the metric
pub fn chart(registry: &Registry, of: &Subject, metric: Metric) -> Result<Chart, Refusal> {
    match (of, metric) {
        (Subject::Machine(name), Metric::Latency) => Err(Refusal::Invalid(format!(
            "latency is a service's; {name} is a machine"
        ))),
        (Subject::Machine(name), _) => {
            registry
                .machine(name)
                .ok_or_else(|| Refusal::Missing(format!("no machine {name}")))?;
            let lines = match metric {
                Metric::Cpu => vec![("cpu", Named::Cpu)],
                Metric::Mem => vec![("mem", Named::Mem)],
                Metric::Disk => vec![("disk", Named::Disk)],
                Metric::Load => vec![("load", Named::Load)],
                Metric::Net => vec![("rx", Named::Rx), ("tx", Named::Tx)],
                Metric::Latency => unreachable!("refused above"),
            };
            Ok(Chart {
                lines,
                on: super::promql::matcher("machine", &name.0),
                by_node: false,
            })
        }
        (Subject::Service(name), Metric::Latency) => {
            let service = registry
                .services
                .get(name)
                .ok_or_else(|| Refusal::Missing(format!("no service {name}")))?;
            if service.probes.is_empty() {
                return Err(Refusal::Invalid(format!("{name} has no probes")));
            }
            Ok(Chart {
                lines: vec![("latency", Named::Latency), ("failing", Named::Failing)],
                on: super::promql::matcher("service", &name.0),
                by_node: true,
            })
        }
        (Subject::Service(name), _) => Err(Refusal::Invalid(format!(
            "a service has a latency only; {name} is a service"
        ))),
    }
}

/// the chart that shows what fired an alert of `rule`
pub fn chart_of_rule(rule: &str) -> Option<Metric> {
    match rule {
        "DiskAlmostFull" | "DiskFillingFast" => Some(Metric::Disk),
        "ServiceUnreachable" => Some(Metric::Latency),
        "MachineDown" => Some(Metric::Cpu),
        _ => None,
    }
}

/// the grid of a chart over `(from, to]`
pub fn grid(from: i64, to: i64, step: i64) -> Vec<i64> {
    let start = from.div_euclid(step) * step + step;
    (0..)
        .map(|k| start + k * step)
        .take_while(|t| *t <= to)
        .collect()
}

/// a range's values on the grid, unset where nothing was measured
pub fn on_grid(range: Option<&Range>, grid: &[i64]) -> Vec<Point> {
    grid.iter()
        .map(|t| {
            let value = range.and_then(|range| {
                range
                    .points
                    .binary_search_by_key(t, |(at, _)| *at)
                    .ok()
                    .and_then(|i| range.points[i].1)
            });
            (Ms(*t), value)
        })
        .collect()
}

/// a machine's lines, each from its own range answer
pub fn lines(answers: Vec<(&'static str, Vec<Range>)>, grid: &[i64]) -> SeriesData {
    SeriesData::Lines {
        lines: answers
            .into_iter()
            .map(|(name, ranges)| Line {
                name: name.to_owned(),
                points: on_grid(ranges.first(), grid),
            })
            .collect(),
    }
}

/// one node's latency
pub fn node_line(node: &Hostname, ranges: &[Range], grid: &[i64]) -> SeriesData {
    let range = ranges
        .iter()
        .find(|range| range.labels.get("prober") == Some(&node.0));
    SeriesData::Lines {
        lines: vec![Line {
            name: node.0.clone(),
            points: on_grid(range, grid),
        }],
    }
}

/// the spans a probe failed on the grid, from `node` or, without one, from
/// any node: each runs from the last point before the failure to the first
/// after it, as a line drawn over the grid leaves its gap
pub fn failing(ranges: &[Range], node: Option<&Hostname>, grid: &[i64]) -> Vec<(Ms, Ms)> {
    let lines: Vec<Vec<Point>> = ranges
        .iter()
        .filter(|range| node.is_none_or(|node| range.labels.get("prober") == Some(&node.0)))
        .map(|range| on_grid(Some(range), grid))
        .collect();
    let failed: Vec<bool> = (0..grid.len())
        .map(|i| lines.iter().any(|line| line[i].1.is_some()))
        .collect();
    spans(&failed, grid)
}

/// the spans of `grid` around each run of `flags` that holds
pub fn spans(flags: &[bool], grid: &[i64]) -> Vec<(Ms, Ms)> {
    let mut spans = Vec::new();
    let mut start = None;
    for (i, flag) in flags.iter().enumerate() {
        match (flag, start) {
            (true, None) => start = Some(i.saturating_sub(1)),
            (false, Some(from)) => {
                spans.push((Ms(grid[from]), Ms(grid[i])));
                start = None;
            }
            _ => {}
        }
    }
    if let (Some(from), Some(last)) = (start, grid.last()) {
        spans.push((Ms(grid[from]), Ms(*last)));
    }
    spans
}

/// min, median and max of the latency across the nodes that reach it
pub fn band(ranges: &[Range], grid: &[i64]) -> SeriesData {
    let lines: Vec<Vec<Point>> = ranges
        .iter()
        .map(|range| on_grid(Some(range), grid))
        .collect();
    SeriesData::Band {
        points: grid
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let mut values: Vec<f64> = lines.iter().filter_map(|line| line[i].1).collect();
                values.sort_by(f64::total_cmp);
                let median = match values.len() {
                    0 => None,
                    n if n % 2 == 1 => Some(values[n / 2]),
                    n => Some((values[n / 2 - 1] + values[n / 2]) / 2.0),
                };
                let point: BandPoint = (
                    Ms(*t),
                    values.first().copied(),
                    median,
                    values.last().copied(),
                );
                point
            })
            .collect(),
    }
}
