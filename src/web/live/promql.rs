//! every series phoebe reads from Prometheus, by what it means. this is the
//! one place PromQL is written: the rest of the source names a [Named] series and
//! the labels to narrow it to

use super::registry::Registry;

/// a series and the PromQL behind it. the snapshot asks for every one of
/// [Named::SNAPSHOT] at once; the charts and alerts ask for the others
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Named {
    /// 1 while a machine's node exporter answers its scrape, 0 while not
    Up,
    /// the last minute within a week a machine answered
    LastUp,
    /// the last minute within a week a machine did not answer
    LastDown,
    /// the first minute within a week a machine was scraped at all
    FirstScraped,
    BootTime,
    /// `node_uname_info`, whose `exported_machine` is the architecture
    Uname,
    Cores,
    MemBytes,
    /// the size of the root filesystem
    DiskBytes,
    /// the fraction of cpu time spent working
    Cpu,
    /// the fraction of memory in use
    Mem,
    /// the fraction of the root filesystem in use
    Disk,
    Load,
    /// bytes per second received on the machine's own interfaces
    Rx,
    /// bytes per second sent on the machine's own interfaces
    Tx,
    /// how many units, or launchd jobs, are active
    UnitsActive,
    /// each unit, or launchd job, that failed, by machine and name
    UnitsFailed,
    /// when each failed unit's UnitFailed alert became active
    FailedSince,
    /// 1 while a node's prometheus, loki, alertmanager or blackbox exporter
    /// answers, by machine and job
    Stores,
    /// each registry service's unit as 1 in its current state and 0 in the
    /// others, by machine, name and state
    UnitState,
    /// the last minute within a week each [Named::UnitState] was 0
    UnitLeft,
    /// 1 while a probe succeeds from the node in `prober`
    ProbeOk,
    ProbeSeconds,
    /// the last minute within a week a probe succeeded
    ProbeLastOk,
    /// the last minute within a week a probe failed
    ProbeLastFailed,
    /// the first minute within a week a probe ran
    ProbeFirst,
    /// the earliest certificate expiry a probe was served, as any node sees it
    CertExpiry,
    /// milliseconds the slowest of a service's probes took from each
    /// prober; unset while one of them fails
    Latency,
    /// every alert firing, by its labels
    Firing,
    /// every alert pending or firing, by its labels and `alertstate`
    Alerts,
    /// 1 for every alert that fired within the window, by its labels
    FiredWithin,
}

impl Named {
    pub const SNAPSHOT: [Self; 25] = [
        Self::Up,
        Self::LastUp,
        Self::LastDown,
        Self::FirstScraped,
        Self::BootTime,
        Self::Uname,
        Self::Cores,
        Self::MemBytes,
        Self::DiskBytes,
        Self::Cpu,
        Self::Mem,
        Self::Disk,
        Self::Load,
        Self::UnitsActive,
        Self::UnitsFailed,
        Self::FailedSince,
        Self::Stores,
        Self::UnitState,
        Self::UnitLeft,
        Self::ProbeOk,
        Self::ProbeSeconds,
        Self::ProbeLastOk,
        Self::ProbeLastFailed,
        Self::ProbeFirst,
        Self::CertExpiry,
    ];

    /// its name, which recorded answers are kept under
    pub fn name(self) -> String {
        format!("{self:?}").to_lowercase()
    }

    /// the PromQL of the series, narrowed to the label matchers in `on`
    /// (`machine="lab"`, or nothing), over windows of `window` seconds where
    /// it is a rate
    pub fn promql(self, registry: &Registry, on: &str, window: u64) -> String {
        let window = window.max(60);
        // a selector of `metric` with the matchers in `on` and those in `extra`
        let sel = |metric: &str, extra: &str| {
            let matchers: Vec<&str> = [on, extra].into_iter().filter(|m| !m.is_empty()).collect();
            format!("{metric}{{{}}}", matchers.join(", "))
        };
        // the last minute within a week that `condition` held
        let last = |condition: String| format!("max_over_time(timestamp({condition})[7d:1m])");
        let units = units(registry);

        match self {
            Self::Up => sel("up", r#"job="node""#),
            Self::LastUp => last(format!("{} == 1", sel("up", r#"job="node""#))),
            Self::LastDown => last(format!("{} == 0", sel("up", r#"job="node""#))),
            Self::FirstScraped => format!(
                "min_over_time(timestamp({})[7d:1m])",
                sel("up", r#"job="node""#)
            ),
            Self::BootTime => sel("node_boot_time_seconds", ""),
            // a machine's hardware as last scraped, so one that is off
            // still shows it
            Self::Uname => format!("last_over_time({}[7d])", sel("node_uname_info", "")),
            Self::Cores => format!(
                "count by (machine) (last_over_time({}[7d]))",
                sel("node_cpu_seconds_total", r#"mode="idle""#)
            ),
            Self::MemBytes => format!(
                "last_over_time({}[7d]) or last_over_time({}[7d])",
                sel("node_memory_MemTotal_bytes", ""),
                sel("node_memory_total_bytes", "")
            ),
            Self::DiskBytes => format!(
                "last_over_time({}[7d])",
                sel("node_filesystem_size_bytes", r#"mountpoint="/""#)
            ),
            // idle, iowait and steal are not work; dividing by all of it,
            // rather than counting cores, holds up on virtual machines whose
            // idle counter runs faster than the clock
            Self::Cpu => format!(
                "sum by (machine) (rate({}[{window}s])) / sum by (machine) (rate({}[{window}s]))",
                sel("node_cpu_seconds_total", r#"mode!~"idle|iowait|steal""#),
                sel("node_cpu_seconds_total", "")
            ),
            // what darwin's Activity Monitor calls memory used: app memory,
            // wired and compressed
            Self::Mem => format!(
                "(1 - {} / {}) or (({} - {} + {} + {}) / {})",
                sel("node_memory_MemAvailable_bytes", ""),
                sel("node_memory_MemTotal_bytes", ""),
                sel("node_memory_internal_bytes", ""),
                sel("node_memory_purgeable_bytes", ""),
                sel("node_memory_wired_bytes", ""),
                sel("node_memory_compressed_bytes", ""),
                sel("node_memory_total_bytes", "")
            ),
            // on darwin, / is the system volume, whose container's free
            // space is the data volume's too
            Self::Disk => format!(
                "1 - {} / {}",
                sel("node_filesystem_avail_bytes", r#"mountpoint="/""#),
                sel("node_filesystem_size_bytes", r#"mountpoint="/""#)
            ),
            Self::Load => sel("node_load1", ""),
            Self::Rx => format!(
                "sum by (machine) (rate({}[{window}s]))",
                sel("node_network_receive_bytes_total", OWN_INTERFACES)
            ),
            Self::Tx => format!(
                "sum by (machine) (rate({}[{window}s]))",
                sel("node_network_transmit_bytes_total", OWN_INTERFACES)
            ),
            Self::UnitsActive => format!(
                "count by (machine) ({} == 1) or count by (machine) ({} == 0)",
                sel("node_systemd_unit_state", r#"state="active""#),
                sel("phoebe_launchd_job_failed", "")
            ),
            Self::UnitsFailed => format!(
                "{} == 1 or {} == 1",
                sel("node_systemd_unit_state", r#"state="failed""#),
                sel("phoebe_launchd_job_failed", "")
            ),
            Self::FailedSince => sel("ALERTS_FOR_STATE", r#"alertname="UnitFailed""#),
            Self::Stores => sel("up", r#"job=~"prometheus|loki|alertmanager|blackbox""#),
            Self::UnitState => unit_state(&sel, &units),
            Self::UnitLeft => last(format!("({}) == 0", unit_state(&sel, &units))),
            Self::ProbeOk => sel("probe_success", ""),
            Self::ProbeSeconds => sel("probe_duration_seconds", ""),
            Self::ProbeLastOk => last(format!("{} == 1", sel("probe_success", ""))),
            Self::ProbeLastFailed => last(format!("{} == 0", sel("probe_success", ""))),
            Self::ProbeFirst => format!(
                "min_over_time(timestamp({})[7d:1m])",
                sel("probe_success", "")
            ),
            Self::CertExpiry => format!(
                "min by (service, target) ({})",
                sel("probe_ssl_earliest_cert_expiry", "")
            ),
            Self::Latency => format!(
                "max by (prober) ({}) * 1000 unless on (prober) (min by (prober) ({}) == 0)",
                sel("probe_duration_seconds", ""),
                sel("probe_success", "")
            ),
            Self::Firing => sel("ALERTS", r#"alertstate="firing""#),
            Self::Alerts => sel("ALERTS", ""),
            Self::FiredWithin => format!(
                "max_over_time({}[{window}s])",
                sel("ALERTS", r#"alertstate="firing""#)
            ),
        }
    }
}

/// the interfaces whose traffic is the machine's own: not loopback, and not
/// the tunnels, bridges and container links whose bytes also cross a
/// physical interface
const OWN_INTERFACES: &str = r#"device!~"lo[0-9]*|veth.*|docker.*|br-.*|virbr.*|podman.*|cni.*|tailscale.*|utun.*|awdl.*|llw.*|bridge.*|gif.*|stf.*|anpi.*|ap[0-9]+""#;

/// `name=~"<every registry unit>"`
fn units(registry: &Registry) -> String {
    let names: Vec<String> = registry
        .units()
        .map(|(_, _, unit)| escape_regex(unit))
        .collect();
    format!("name=~{}", quote(&names.join("|")))
}

/// systemd's unit states as they are, and a launchd job's as `failed` or
/// `active`, so both read the same way
fn unit_state(sel: &impl Fn(&str, &str) -> String, units: &str) -> String {
    let launchd = sel("phoebe_launchd_job_failed", units);
    format!(
        r#"{} or label_replace({launchd}, "state", "failed", "", "") or label_replace(1 - {launchd}, "state", "active", "", "")"#,
        sel("node_systemd_unit_state", units)
    )
}

/// a PromQL or LogQL string literal: both take Go's escapes, which JSON's are
pub fn quote(text: &str) -> String {
    serde_json::to_string(text).expect("a string always serializes")
}

/// `text` as a regular expression that matches it alone
pub fn escape_regex(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if r"\.+*?()|[]{}^$".contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// `label="value"`, quoted
pub fn matcher(label: &str, value: &str) -> String {
    format!("{label}={}", quote(value))
}
