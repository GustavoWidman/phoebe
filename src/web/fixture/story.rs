//! what happens to the simulated fleet, and when. every state it shows is a
//! function of the moment and these tables, nothing else.
//!
//! the timeline starts at 15:42 BRT on 2026-11-25, as the dashboard's
//! prototype does: lavalink failed on oracle-1 (a page), derp's certificate
//! is about to expire (info), oracle-2's full disk resolved at 13:20, lab
//! cannot reach r3dlust.com while the other nodes can, and both desktops
//! rest. then lab's route comes back (15:46), lavalink is restarted (15:50),
//! derp renews its certificate (15:54) and the laptop wakes (16:00). from
//! there on the fleet is all clear.

use super::{
    fleet::{MACHINES, MachineSpec, NODES, ProbeSpec, ServiceSpec},
    signal::{hash, knots, wobble},
};
use crate::web::wire::{Metric, Severity, UnitState};

pub const SECOND: i64 = 1_000;
pub const MINUTE: i64 = 60 * SECOND;
pub const HOUR: i64 = 60 * MINUTE;
pub const DAY: i64 = 24 * HOUR;

/// 2026-11-25 00:00 UTC
const DAY0: i64 = 1_795_564_800_000;

/// a moment of 2026-11-25 in Brasília time, UTC-3; hours below 0 or past
/// 23 reach the days around it
pub const fn brt(hours: i64, minutes: i64, seconds: i64) -> i64 {
    DAY0 + ((hours + 3) * 3600 + minutes * 60 + seconds) * SECOND
}

/// where the timeline starts
pub const START: i64 = brt(15, 42, 0);
/// the scrape interval, which every moment the fleet shows is a multiple of
pub const STEP: i64 = 15 * SECOND;
/// how far back series, logs and alerts reach
pub const HISTORY: i64 = 7 * DAY;

pub const LAVALINK_OOM: i64 = brt(14, 57, 3);
pub const LAVALINK_FAILED: i64 = brt(14, 58, 30);
pub const LAVALINK_RESTARTED: i64 = brt(15, 50, 0);
pub const ROUTE_LOST: i64 = brt(15, 20, 48);
pub const ROUTE_RESTORED: i64 = brt(15, 46, 24);
pub const LAB_PROBE_FAILS: i64 = brt(15, 21, 2);
pub const LAB_PROBE_OK: i64 = brt(15, 46, 32);
pub const GC: i64 = brt(13, 20, 4);
pub const CERT_RETRY: i64 = brt(15, 10, 0);
pub const CERT_RENEWED: i64 = brt(15, 54, 0);
/// when derp's certificate expires until it is renewed: 2026-12-04 18:00 UTC
const DERP_EXPIRY: i64 = DAY0 + 9 * DAY + 18 * HOUR;
/// lab's loki, down for a while three nights before
const LAB_LOKI_DOWN: (i64, i64) = (brt(-72 + 3, 14, 0), brt(-72 + 3, 41, 0));
pub const LAPTOP_SLEEPS: i64 = brt(13, 5, 0);
pub const LAPTOP_WAKES: i64 = brt(16, 0, 0);

/// when each machine is up, `[from, to)` in time order, and when it booted
/// if not at the start of each
struct Life {
    up: &'static [(i64, i64)],
    booted: Option<i64>,
}

const FOREVER: i64 = i64::MAX;

fn life(machine: &str) -> Life {
    match machine {
        "oracle-1" | "oracle-2" => Life {
            up: &[(ORACLE_BOOT, FOREVER)],
            booted: Some(ORACLE_BOOT),
        },
        "lab" => Life {
            up: &[(LAB_BOOT, FOREVER)],
            booted: Some(LAB_BOOT),
        },
        "altar" => Life {
            up: &[(ALTAR_BOOT, FOREVER)],
            booted: Some(ALTAR_BOOT),
        },
        "laptop-mac" => Life {
            up: &LAPTOP_UP,
            booted: Some(brt(-96 + 10, 12, 0)),
        },
        // powered off between uses, so each time up is a boot
        _ => Life {
            up: &DESKTOP_UP,
            booted: None,
        },
    }
}

/// the laptop sleeps rather than shuts down
const LAPTOP_UP: [(i64, i64); 4] = [
    (brt(-48 + 9, 2, 0), brt(-48 + 19, 31, 0)),
    (brt(-24 + 8, 40, 0), brt(-24 + 22, 15, 0)),
    (brt(8, 30, 0), LAPTOP_SLEEPS),
    (LAPTOP_WAKES, FOREVER),
];
pub const DESKTOP_UP: [(i64, i64); 2] = [
    (brt(-72 + 19, 5, 0), brt(-72 + 23, 50, 0)),
    (brt(-24 + 18, 40, 0), brt(-24 + 23, 10, 0)),
];
const ORACLE_BOOT: i64 = START - (23 * DAY + 4 * HOUR);
const LAB_BOOT: i64 = START - (6 * DAY + 19 * HOUR);
const ALTAR_BOOT: i64 = START - (11 * DAY + 2 * HOUR);

/// a machine at one moment
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    Up {
        since: i64,
        booted: i64,
    },
    /// not answering since `since`; down if it is always on, resting if not
    Away {
        since: i64,
    },
}

pub fn presence(machine: &MachineSpec, at: i64) -> Presence {
    let life = life(machine.name);
    let ended = life.up.partition_point(|(from, _)| *from <= at);
    match ended.checked_sub(1).map(|i| life.up[i]) {
        Some((from, to)) if at < to => Presence::Up {
            since: from,
            booted: life.booted.unwrap_or(from),
        },
        Some((_, to)) => Presence::Away { since: to },
        None => Presence::Away {
            since: START - 30 * DAY,
        },
    }
}

pub fn up(machine: &MachineSpec, at: i64) -> bool {
    matches!(presence(machine, at), Presence::Up { .. })
}

/// where each machine's metrics sit, and how far they wander
struct Levels {
    cpu: (f64, f64),
    mem: (f64, f64),
    disk: f64,
    load: (f64, f64),
    /// bytes per second received and sent
    net: (f64, f64),
}

fn levels(machine: &str) -> Levels {
    match machine {
        "altar" => Levels {
            cpu: (0.13, 0.06),
            mem: (0.675, 0.016),
            disk: 0.411,
            load: (2.1, 0.7),
            net: (260e3, 190e3),
        },
        "desktop-nixos" => Levels {
            cpu: (0.07, 0.04),
            mem: (0.31, 0.02),
            disk: 0.382,
            load: (0.6, 0.3),
            net: (90e3, 40e3),
        },
        "lab" => Levels {
            cpu: (0.11, 0.05),
            mem: (0.47, 0.02),
            disk: 0.326,
            load: (1.6, 0.8),
            net: (320e3, 210e3),
        },
        "laptop-mac" => Levels {
            cpu: (0.09, 0.05),
            mem: (0.58, 0.02),
            disk: 0.553,
            load: (1.2, 0.5),
            net: (120e3, 60e3),
        },
        "oracle-1" => Levels {
            cpu: (0.19, 0.09),
            mem: (0.0, 0.015),
            disk: 0.472,
            load: (0.85, 0.5),
            net: (820e3, 2100e3),
        },
        _ => Levels {
            cpu: (0.08, 0.04),
            mem: (0.435, 0.014),
            disk: 0.0,
            load: (0.4, 0.3),
            net: (380e3, 610e3),
        },
    }
}

/// lab runs a benchmark 12 minutes of every 40
fn benchmarking(at: i64) -> bool {
    (at - DAY0).rem_euclid(40 * MINUTE) < 12 * MINUTE
}

/// lavalink restarting on oracle-1, out of memory each time
fn thrashing(at: i64) -> bool {
    (brt(14, 52, 0)..LAVALINK_FAILED).contains(&at)
}

/// oracle-1's memory: lavalink leaking heap until it failed, freed by its
/// failure, and leaking again once restarted
fn oracle_1_mem(at: i64) -> f64 {
    match at {
        at if at < LAVALINK_FAILED => knots(
            &[
                (LAVALINK_FAILED - DAY, 0.50),
                (brt(12, 0, 0), 0.52),
                (LAVALINK_FAILED, 0.74),
            ],
            at,
        ),
        at if at < LAVALINK_RESTARTED => 0.605,
        at => knots(
            &[(LAVALINK_RESTARTED, 0.55), (LAVALINK_RESTARTED + DAY, 0.62)],
            at,
        ),
    }
}

/// oracle-2's root filesystem: filling until it crossed 90% at 10:52,
/// emptied by a garbage collection at 13:20
fn oracle_2_disk(at: i64) -> f64 {
    match at < GC {
        true => knots(
            &[(GC - 7 * DAY, 0.78), (brt(10, 52, 0), 0.900), (GC, 0.9036)],
            at,
        ),
        false => knots(&[(GC, 0.708), (GC + 30 * DAY, 0.74)], at),
    }
}

fn seed(machine: &MachineSpec, metric: u64) -> u64 {
    let index = MACHINES
        .iter()
        .position(|other| other.name == machine.name)
        .unwrap_or(0) as u64;
    index * 16 + metric
}

/// `metric` of `machine` at `at`, whether or not it was up
pub fn metric(machine: &MachineSpec, metric: Metric, at: i64) -> Vec<f64> {
    let levels = levels(machine.name);
    let around = |(level, spread): (f64, f64), n| level + spread * wobble(seed(machine, n), at);
    let busy = machine.name == "lab" && benchmarking(at);
    let thrash = machine.name == "oracle-1" && thrashing(at);
    match metric {
        Metric::Cpu => {
            let cpu = match (busy, thrash) {
                (true, _) => around((0.84, 0.06), 0),
                (_, true) => around(levels.cpu, 0) + 0.34,
                _ => around(levels.cpu, 0),
            };
            vec![round(cpu.clamp(0.01, 0.99), 4)]
        }
        Metric::Mem => {
            let mem = match machine.name {
                "oracle-1" => oracle_1_mem(at) + levels.mem.1 * wobble(seed(machine, 1), at),
                "lab" if busy => around((0.61, 0.02), 1),
                _ => around(levels.mem, 1),
            };
            vec![round(mem.clamp(0.0, 1.0), 4)]
        }
        Metric::Disk => {
            let base = match machine.name {
                "oracle-2" => oracle_2_disk(at),
                _ => levels.disk,
            };
            vec![round(base + 0.0004 * wobble(seed(machine, 2), at), 4)]
        }
        Metric::Load => {
            let load = match (busy, thrash) {
                (true, _) => around((13.5, 0.8), 3),
                (_, true) => around(levels.load, 3) + 2.1,
                _ => around(levels.load, 3),
            };
            vec![round(load.max(0.02), 2)]
        }
        Metric::Net => {
            let (rx, tx) = match busy {
                true => (14_000e3, 9_000e3),
                false => levels.net,
            };
            vec![
                round(
                    (rx * (1.0 + 0.6 * wobble(seed(machine, 4), at))).max(5e3),
                    0,
                ),
                round(
                    (tx * (1.0 + 0.6 * wobble(seed(machine, 5), at))).max(5e3),
                    0,
                ),
            ]
        }
        Metric::Latency => vec![],
    }
}

/// `value` to `digits` decimals, as precise as a dashboard reads it
fn round(value: f64, digits: i32) -> f64 {
    let scale = 10_f64.powi(digits);
    (value * scale).round() / scale
}

/// the state of a registry service's unit, and since when, while its
/// machine is up
pub fn unit(service: &ServiceSpec, booted: i64, at: i64) -> (UnitState, i64) {
    if service.name != "lavalink" {
        return (UnitState::Active, booted);
    }
    match at {
        at if at < LAVALINK_OOM => (UnitState::Active, booted),
        at if at < LAVALINK_FAILED => (UnitState::Activating, LAVALINK_OOM),
        at if at < LAVALINK_RESTARTED => (UnitState::Failed, LAVALINK_FAILED),
        _ => (UnitState::Active, LAVALINK_RESTARTED),
    }
}

pub fn loki_up(node: &str, at: i64) -> bool {
    !(node == "lab" && (LAB_LOKI_DOWN.0..LAB_LOKI_DOWN.1).contains(&at))
}

/// a probe one node fails for a while
struct Fault {
    target: &'static str,
    node: &'static str,
    from: i64,
    to: i64,
}

/// lab loses its route to oracle-2, so it cannot reach r3dlust.com
const FAULTS: [Fault; 1] = [Fault {
    target: "https://r3dlust.com",
    node: "lab",
    from: LAB_PROBE_FAILS,
    to: LAB_PROBE_OK,
}];

/// how long a failed probe takes: its timeout
pub const TIMEOUT_MS: f64 = 10_000.0;

/// whether `node` reaches `probe` at `at`, and since when it has or has not
pub fn reach(probe: &ProbeSpec, node: &str, at: i64) -> (bool, i64) {
    let booted = life(node).booted.unwrap_or(START - 30 * DAY);
    FAULTS
        .iter()
        .filter(|fault| fault.target == probe.target && fault.node == node && fault.from <= at)
        .map(|fault| match at < fault.to {
            true => (false, fault.from),
            false => (true, fault.to),
        })
        .next_back()
        .unwrap_or((true, booted))
}

/// milliseconds from a node to a machine, by where both are
fn distance(node: &str, machine: &str) -> f64 {
    let cloud = |name: &str| name.starts_with("oracle");
    match (node == machine, cloud(node), cloud(machine)) {
        (true, _, _) => 0.4,
        (_, true, true) => 1.7,
        (_, false, false) => 0.6,
        _ => 26.0,
    }
}

/// how long `probe` of `service` takes from `node`, if it succeeds
pub fn latency(service: &ServiceSpec, index: usize, node: &str, at: i64) -> Option<f64> {
    let probe = &service.probes[index];
    if !reach(probe, node, at).0 {
        return None;
    }
    let node_index = NODES.iter().position(|n| *n == node).unwrap_or(0) as u64;
    let seed = 1000 + (service.name.len() as u64) * 97 + index as u64 * 13 + node_index;
    let base = distance(node, service.machine) + service.overhead_ms + 0.4 * index as f64;
    let spike = match hash(seed ^ 0xa5a5, at / STEP) < 0.04 {
        true => base * 2.0 * hash(seed ^ 0x5a5a, at / STEP),
        false => 0.0,
    };
    Some(round(
        (base * (1.0 + 0.25 * wobble(seed, at)) + spike).max(0.2),
        2,
    ))
}

/// when the certificate `probe` is served expires: derp's lapses unless
/// renewed, every other one renews every 60 days for 90
pub fn cert_expiry(probe: &ProbeSpec, at: i64) -> Option<i64> {
    if !probe.certified() {
        return None;
    }
    if probe.target == "https://derp.r3dlust.com" {
        return Some(match at < CERT_RENEWED {
            true => DERP_EXPIRY,
            false => CERT_RENEWED + 90 * DAY,
        });
    }
    let phase = DAY0 - (hash(0xce57, probe.target.len() as i64 * 31) * 60.0 * DAY as f64) as i64;
    let issued = phase + (at - phase).div_euclid(60 * DAY) * 60 * DAY;
    Some(issued + 90 * DAY)
}

/// something that fired one of the rules
pub struct Incident {
    pub rule: &'static str,
    pub severity: Severity,
    pub machine: Option<&'static str>,
    pub service: Option<&'static str>,
    /// labels besides alertname, severity, machine and service
    pub labels: &'static [(&'static str, &'static str)],
    /// when its condition first held
    pub held: i64,
    /// how long the rule waits before firing
    pub wait: i64,
    /// when its condition stopped holding
    pub cleared: Option<i64>,
    /// the node that pushed it, for a page
    pub notified_by: Option<&'static str>,
    /// the metric that shows it: its service's if `Latency`, else its machine's
    pub chart: Option<Metric>,
    /// the rule's summary, as evaluated at a moment it fired
    pub summary: fn(i64) -> String,
}

pub const INCIDENTS: [Incident; 4] = [
    Incident {
        rule: "NodeUnhealthy",
        severity: Severity::Page,
        machine: Some("lab"),
        service: None,
        labels: &[("instance", "100.122.90.22:3100"), ("job", "loki")],
        held: LAB_LOKI_DOWN.0,
        wait: 5 * MINUTE,
        cleared: Some(LAB_LOKI_DOWN.1),
        notified_by: Some("oracle-1"),
        chart: None,
        summary: |_| "loki on lab is down".into(),
    },
    Incident {
        rule: "CertExpiring",
        severity: Severity::Info,
        machine: Some("oracle-2"),
        service: Some("derp"),
        labels: &[("target", "https://derp.r3dlust.com")],
        held: DERP_EXPIRY - 14 * DAY,
        wait: 10 * MINUTE,
        cleared: Some(CERT_RENEWED),
        notified_by: None,
        chart: None,
        summary: |at| {
            format!(
                "the certificate of https://derp.r3dlust.com expires in {}",
                humanize_duration((DERP_EXPIRY - at) / SECOND)
            )
        },
    },
    Incident {
        rule: "DiskAlmostFull",
        severity: Severity::Page,
        machine: Some("oracle-2"),
        service: None,
        labels: &[
            ("device", "/dev/sda1"),
            ("fstype", "ext4"),
            ("instance", "100.119.88.94:9100"),
            ("job", "node"),
            ("mountpoint", "/"),
        ],
        held: brt(10, 52, 0),
        wait: 15 * MINUTE,
        cleared: Some(GC),
        notified_by: Some("altar"),
        chart: Some(Metric::Disk),
        summary: |at| {
            let free = 1.0 - oracle_2_disk(at);
            format!("/ on oracle-2 is {} free", humanize_percentage(free))
        },
    },
    Incident {
        rule: "UnitFailed",
        severity: Severity::Page,
        machine: Some("oracle-1"),
        service: Some("lavalink"),
        labels: &[
            ("instance", "100.84.113.80:9100"),
            ("job", "node"),
            ("name", "lavalink.service"),
            ("state", "failed"),
        ],
        held: LAVALINK_FAILED,
        wait: MINUTE,
        cleared: Some(LAVALINK_RESTARTED),
        notified_by: Some("oracle-2"),
        chart: Some(Metric::Mem),
        summary: |_| "lavalink.service of lavalink failed on oracle-1".into(),
    },
];

/// how far behind each node evaluates its rules, in the order of [NODES]
const EVALUATES: [i64; 4] = [11 * SECOND, 4 * SECOND, 0, 7 * SECOND];

/// how long the alertmanager waits before sending a group
const GROUP_WAIT: i64 = 30 * SECOND;

/// an incident as each node's prometheus saw it
pub struct Seen {
    pub node: &'static str,
    pub pending: i64,
    pub firing: i64,
    pub resolved: Option<i64>,
}

impl Incident {
    pub fn seen(&self) -> impl Iterator<Item = Seen> + '_ {
        NODES.iter().zip(EVALUATES).map(|(node, late)| {
            let pending = next_step(self.held) + late;
            Seen {
                node,
                pending,
                firing: pending + self.wait,
                resolved: self.cleared.map(|cleared| next_step(cleared) + late),
            }
        })
    }

    /// when the first node fired it
    pub fn starts_at(&self) -> i64 {
        self.seen().map(|seen| seen.firing).min().unwrap_or(FOREVER)
    }

    /// when the last node stopped firing it
    pub fn resolved_at(&self) -> Option<i64> {
        self.seen().map(|seen| seen.resolved).max().flatten()
    }

    /// when it was pushed, and its resolution
    pub fn notified(&self) -> Option<(&'static str, i64, Option<i64>)> {
        let by = self.notified_by?;
        let resolved = self.resolved_at().map(|at| at + GROUP_WAIT);
        Some((by, self.starts_at() + GROUP_WAIT, resolved))
    }
}

/// the first scrape at or after `at`
pub fn next_step(at: i64) -> i64 {
    (at + STEP - 1).div_euclid(STEP) * STEP
}

/// prometheus' `humanizeDuration`, of whole seconds
fn humanize_duration(seconds: i64) -> String {
    let (days, hours) = (seconds / 86_400, seconds / 3_600 % 24);
    let (minutes, seconds) = (seconds / 60 % 60, seconds % 60);
    match (days, hours, minutes) {
        (0, 0, 0) => format!("{seconds}s"),
        (0, 0, _) => format!("{minutes}m {seconds}s"),
        (0, _, _) => format!("{hours}h {minutes}m {seconds}s"),
        _ => format!("{days}d {hours}h {minutes}m {seconds}s"),
    }
}

/// prometheus' `humanizePercentage`: four significant digits
fn humanize_percentage(fraction: f64) -> String {
    let percent = fraction * 100.0;
    let digits = (4 - (percent.abs().log10().floor() as i32 + 1)).max(0) as usize;
    let text = format!("{percent:.digits$}");
    let text = match text.contains('.') {
        true => text.trim_end_matches('0').trim_end_matches('.').to_owned(),
        false => text,
    };
    format!("{text}%")
}
