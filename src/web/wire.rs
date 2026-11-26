//! what the web app reads, as the server serializes it. the browser's
//! types in `web/src/lib/wire/types.ts` and the examples in
//! `contract/web.json` are generated from these, so a change here is a
//! change to the contract.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// the version of these shapes; raised only when a change would break a
/// browser still running the previous build
pub const SCHEMA: u16 = 1;

/// epoch milliseconds
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Ms(pub i64);

impl Ms {
    pub fn now() -> Self {
        Self(super::live::now())
    }
}

macro_rules! name {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl From<&str> for $name {
            fn from(name: &str) -> Self {
                Self(name.to_owned())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

name!(
    /// a machine's name in the registry, nodes included
    Hostname
);
name!(
    /// a service's name in the registry, unique across the fleet
    ServiceName
);
name!(
    /// an alert's alertmanager fingerprint, 16 hex digits
    AlertId
);

/// `GET /api/hello`, the one route that answers before signing in, and it
/// carries no fleet data
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Hello {
    /// the serving nodes' sign-in addresses, reachable only on the tailnet
    pub signin: Vec<String>,
    /// the web app build this server serves
    pub build: String,
    pub schema: u16,
}

/// what a browser sends to `POST /auth/session` to sign in: its public
/// key, and its signature over `phoebe-signin-v1\n<public origin>\n<ts>` by
/// the matching private key
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SignIn {
    pub key: Jwk,
    pub ts: Ms,
    /// `r || s`, base64url, as WebCrypto signs
    pub sig: String,
}

/// a P-256 public key as a JSON Web Key, as WebCrypto exports it
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Jwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
    pub y: String,
}

/// what signing in on the tailnet answers: the session to send as
/// `Authorization: Phoebe <token>`, and who it is for
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SignedIn {
    pub token: String,
    pub viewer: Viewer,
}

/// who is signed in, on which device, until when
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Viewer {
    /// the tailnet login
    pub login: String,
    /// the tailnet node, by its MagicDNS name
    pub device: String,
    pub expires_at: Ms,
}

/// `GET /api/push`: the key a browser subscribes with, and the devices
/// pushes go to, each the first 16 hex digits of its endpoint's SHA-256
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PushView {
    /// the VAPID public key, base64url, the browser's `applicationServerKey`
    pub key: String,
    pub subscribed: Vec<String>,
    /// the command that edits the subscription list
    pub edit: Option<String>,
}

/// one message of `GET /api/stream`. a snapshot is the whole fleet, sent on
/// connect and again whenever it changes
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamEvent {
    Snapshot(Snapshot),
    /// the session ended; the stream closes after this
    Expired,
}

/// the fleet at one moment, as one node sees it
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Snapshot {
    pub schema: u16,
    /// the moment it describes
    pub at: Ms,
    /// the node whose phoebe built it, from whose view `nodes` is seen
    pub served_by: Hostname,
    /// in registry order
    pub nodes: Vec<Node>,
    /// in registry order, nodes included
    pub machines: Vec<Machine>,
    /// in registry order
    pub services: Vec<Service>,
    /// firing, newest first, then resolved in the last 24 hours, newest first
    pub alerts: Vec<Alert>,
}

/// a node's stores, each up or not as `served_by` sees it
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Node {
    pub name: Hostname,
    pub prometheus: bool,
    pub loki: bool,
    pub alertmanager: bool,
    pub blackbox: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Machine {
    pub name: Hostname,
    pub os: Os,
    /// a machine that is not always on rests instead of going down
    pub always_on: bool,
    /// as last scraped; unset until it first is
    pub hardware: Option<Hardware>,
    pub state: MachineState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Os {
    NixOs,
    Darwin,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Hardware {
    /// `aarch64`, `x86_64`
    pub arch: String,
    pub cores: u32,
    pub mem_bytes: u64,
    /// the size of the root filesystem
    pub disk_bytes: u64,
}

/// whether the machine answers its scrapes
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MachineState {
    /// answering since `since`
    Up {
        since: Ms,
        booted_at: Ms,
        vitals: Vitals,
        units: Units,
    },
    /// an always-on machine that stopped answering at `since`
    Down { since: Ms },
    /// a machine that is not always on, asleep or off since `since`; never
    /// an alert
    Resting { since: Ms },
}

/// the machine's last scrape
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Vitals {
    /// the fraction of cpu time spent working, over the last minute
    pub cpu: f64,
    /// the fraction of memory in use
    pub mem: f64,
    /// the fraction of the root filesystem in use
    pub disk: f64,
    pub load1: f64,
}

/// the machine's systemd units, or its launchd jobs on darwin
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Units {
    pub active: u32,
    pub failed: Vec<FailedUnit>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FailedUnit {
    pub name: String,
    pub since: Ms,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Service {
    pub name: ServiceName,
    pub machine: Hostname,
    /// unset for a service with no unit of its own
    pub unit: Option<UnitStatus>,
    /// in registry order
    pub probes: Vec<Probe>,
    /// unset for a service that reports no heartbeat
    pub heartbeat: Option<Heartbeat>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct UnitStatus {
    /// the systemd unit, or the launchd label on darwin
    pub name: String,
    pub state: UnitState,
    /// when it entered `state`; unset while it is unknown
    pub since: Option<Ms>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum UnitState {
    Active,
    /// starting, or restarting after it exited
    Activating,
    Inactive,
    Failed,
    /// its machine is not answering
    Unknown,
}

/// a gauge on the service's machine, holding when the service last knew
/// itself healthy
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Heartbeat {
    pub metric: String,
    /// how far behind the gauge may fall before ServiceStale fires
    pub within_ms: i64,
    pub state: HeartbeatState,
    /// the last moment the service knew itself healthy; unset while the
    /// gauge is missing
    pub healthy_at: Option<Ms>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum HeartbeatState {
    /// healthy within `within_ms`
    Fresh,
    /// last healthy longer ago than `within_ms`
    Late,
    /// its machine answers, without the gauge
    Missing,
    /// its machine is not answering
    Unknown,
}

/// one probe of a service, as every node that answered runs it
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Probe {
    pub kind: ProbeKind,
    /// a URL for http, `host:port` for the others, a server for dns
    pub target: String,
    /// whether it speaks TLS from the first byte, as an https URL does
    pub tls: bool,
    /// the earliest expiry in the certificate chain it was served
    pub cert_expires_at: Option<Ms>,
    /// one per node that answered, in registry order
    pub views: Vec<ProbeView>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ProbeKind {
    Http,
    Tcp,
    Smtp,
    Imap,
    Dns,
}

/// the last probe one node ran
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProbeView {
    pub node: Hostname,
    pub ok: bool,
    /// how long the probe took, up to its timeout when it failed
    pub latency_ms: f64,
    /// when `ok` last changed
    pub since: Ms,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Alert {
    pub id: AlertId,
    /// the rule's alertname
    pub rule: String,
    pub severity: Severity,
    pub state: AlertState,
    /// when the first node fired it
    pub starts_at: Ms,
    pub machine: Option<Hostname>,
    pub service: Option<ServiceName>,
    pub summary: String,
    /// the nodes whose prometheus fires it, or fired it once it resolved;
    /// a node that did not answer is left out
    pub seen_by: Vec<Hostname>,
    pub labels: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// pushed to the phone
    Page,
    /// shown in the app only
    Info,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AlertState {
    Firing,
    /// no node fires it since `at`
    Resolved {
        at: Ms,
    },
}

/// `GET /api/alerts/<id>`
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AlertDetail {
    pub alert: Alert,
    /// oldest first
    pub timeline: Vec<TimelineEntry>,
    /// the series that shows what fired it, around when it did
    pub series: Option<Series>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimelineEntry {
    /// the rule's condition held on `node`, which waits out its duration
    Pending {
        at: Ms,
        node: Hostname,
    },
    Firing {
        at: Ms,
        node: Hostname,
    },
    Resolved {
        at: Ms,
        node: Hostname,
    },
    /// the phoebe on `by` pushed the alert, or its resolution
    Notified {
        at: Ms,
        by: Hostname,
        resolved: bool,
    },
}

/// `GET /api/series`: one metric of one machine or service over a range
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Series {
    pub of: Subject,
    pub metric: Metric,
    pub from: Ms,
    pub to: Ms,
    pub data: SeriesData,
    /// for a service's latency, the spans a probe failed (from `node`, or
    /// from any node for the band), as against the nulls where nothing was
    /// measured at all; empty for a machine
    pub failing: Vec<(Ms, Ms)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Subject {
    Machine(Hostname),
    Service(ServiceName),
}

/// a machine's `cpu`, `mem` and `disk` are fractions, `net` is bytes per
/// second received (`rx`) and sent (`tx`), `load` the 1-minute load
/// average; a service's `latency` is milliseconds, the slowest of its probes
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    Cpu,
    Mem,
    Disk,
    Net,
    Load,
    Latency,
}

/// points are `[t, value]`, or `[t, min, median, max]` across nodes; null
/// where nothing was measured
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SeriesData {
    Lines {
        lines: Vec<Line>,
    },
    /// a service's latency across the nodes that reach it
    Band {
        points: Vec<BandPoint>,
    },
}

pub type Point = (Ms, Option<f64>);
pub type BandPoint = (Ms, Option<f64>, Option<f64>, Option<f64>);

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Line {
    /// the metric, `rx` or `tx` for `net`, or the node a latency is from
    pub name: String,
    pub points: Vec<Point>,
}

/// `GET /api/logs`
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogPage {
    /// newest first
    pub lines: Vec<LogLine>,
    /// passed back as `before`, the next page's older lines; unset at the end
    pub before: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogLine {
    pub at: Ms,
    pub machine: Hostname,
    /// the unit that wrote it, or the launchd label or log file on darwin
    pub unit: String,
    pub level: Level,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
}

/// one message of `GET /api/logs/tail`
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LogEvent {
    /// lines that arrived since the last, oldest first
    Lines { lines: Vec<LogLine> },
    /// the session ended; the stream closes after this
    Expired,
}

/// what `POST /fixture/seek` answers: where the simulated fleet's clock is
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FixtureClock {
    /// seconds after the timeline starts
    pub t: f64,
    /// timeline seconds per real second; 0 holds the clock
    pub speed: f64,
    /// the moment it is in the simulated fleet
    pub now: Ms,
}
