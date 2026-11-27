//! the simulated fleet's logs: what each machine writes while it is up, at
//! moments drawn from its seed, and the story's own lines

use std::sync::OnceLock;

use super::{
    fleet::{MACHINES, MachineSpec, SERVICES},
    signal::{hash, mix},
    story::{
        CERT_RENEWED, CERT_RETRY, DESKTOP_UP, GC, LAB_PROBE_FAILS, LAB_PROBE_OK, LAPTOP_SLEEPS,
        LAPTOP_WAKES, LAVALINK_FAILED, LAVALINK_OOM, LAVALINK_RESTARTED, MINUTE, ROUTE_LOST,
        ROUTE_RESTORED, SECOND, brt, unit, up,
    },
};
use crate::web::wire::{Hostname, Level, LogLine, Ms, UnitState};

/// a line, and where it sorts: by time, then by `id`
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: u64,
    pub line: LogLine,
}

impl Entry {
    pub fn key(&self) -> (i64, u64) {
        (self.line.at.0, self.id)
    }
}

/// which lines a reader wants
#[derive(Clone, Debug, Default)]
pub struct Filter {
    pub machine: Option<&'static str>,
    /// lines from this unit, or that name it
    pub unit: Option<String>,
    /// lines that name one of these hosts
    pub hosts: Vec<&'static str>,
    /// lowercase, found in the line's unit or text
    pub q: Option<String>,
}

impl Filter {
    fn wants(&self, line: &LogLine) -> bool {
        self.machine.is_none_or(|machine| line.machine.0 == machine)
            && self
                .unit
                .as_ref()
                .is_none_or(|unit| line.unit == *unit || line.text.contains(unit.as_str()))
            && (self.hosts.is_empty() || self.hosts.iter().any(|host| line.text.contains(host)))
            && self.q.as_ref().is_none_or(|q| {
                line.text.to_lowercase().contains(q) || line.unit.to_lowercase().contains(q)
            })
    }
}

/// every line in `[from, to]` that `filter` wants, oldest first
pub fn between(from: i64, to: i64, filter: &Filter) -> Vec<Entry> {
    let mut entries: Vec<Entry> = MACHINES
        .iter()
        .filter(|machine| filter.machine.is_none_or(|name| name == machine.name))
        .flat_map(|machine| background(machine, from, to))
        .chain(
            scripted()
                .iter()
                .filter(|entry| (from..=to).contains(&entry.line.at.0))
                .cloned(),
        )
        .filter(|entry| filter.wants(&entry.line))
        .collect();
    entries.sort_by_key(Entry::key);
    entries
}

/// lines come in slots of this length, each holding up to two
const SLOT: i64 = 15 * SECOND;

/// what `machine` writes in `[from, to]` while nothing in particular happens
fn background(machine: &'static MachineSpec, from: i64, to: i64) -> impl Iterator<Item = Entry> {
    let templates = templates(machine.name);
    let seed = machine.name.bytes().fold(0u64, |seed, byte| {
        seed.wrapping_mul(31).wrapping_add(byte as u64)
    });
    (from.div_euclid(SLOT)..=to.div_euclid(SLOT))
        .flat_map(move |slot| {
            let count = match hash(seed, slot) {
                r if r < 0.35 => 0,
                r if r < 0.8 => 1,
                _ => 2,
            };
            (0..count).map(move |k| (slot, k))
        })
        .filter_map(move |(slot, k)| {
            let draw = mix(seed, slot * 4 + k + 1);
            let at = slot * SLOT + (draw % SLOT as u64) as i64;
            let template = &templates[(draw >> 20) as usize % templates.len()];
            let running =
                (from..=to).contains(&at) && up(machine, at) && alive(machine, template.0, at);
            running.then(|| Entry {
                id: draw,
                line: LogLine {
                    at: Ms(at),
                    machine: Hostname::from(machine.name),
                    unit: template.0.into(),
                    level: template.1,
                    text: fill(template.2, draw),
                },
            })
        })
}

/// whether the unit writing a line is running: a registry service's unit
/// writes nothing while it is not active
fn alive(machine: &MachineSpec, name: &str, at: i64) -> bool {
    SERVICES
        .iter()
        .find(|service| service.machine == machine.name && service.unit == Some(name))
        .is_none_or(|service| unit(service, 0, at).0 == UnitState::Active)
}

/// `{h}` becomes a short hash and `{n}` a digit, both drawn from `draw`
fn fill(template: &str, draw: u64) -> String {
    let digits = "0123456789abcdefghijklmnopqrstuvwxyz".as_bytes();
    let short: String = (0..6)
        .map(|i| digits[((draw >> (i * 6)) % 36) as usize] as char)
        .collect();
    let digit = ((draw >> 40) % 9 + 1).to_string();
    template.replace("{h}", &short).replace("{n}", &digit)
}

type Template = (&'static str, Level, &'static str);

fn templates(machine: &str) -> &'static [Template] {
    use Level::{Debug, Info};
    match machine {
        "altar" => &[
            ("org.nixos.agents", Info, "session started in ~/GitHub/nix"),
            ("org.nixos.agents", Info, "tool call: cargo build, 4{n}s"),
            (
                "org.nixos.coeusd",
                Debug,
                "poll: darwin generation is current",
            ),
            (
                "org.nixos.nix-daemon",
                Info,
                "copied 1{n} paths from cache.r3dlust.com",
            ),
        ],
        "desktop-nixos" => &[
            (
                "coeusd.service",
                Debug,
                "poll: generation 26.05.20261124 is current",
            ),
            (
                "NetworkManager.service",
                Info,
                "<info> dhcp4 (enp5s0): lease renewed",
            ),
            (
                "tailscaled.service",
                Info,
                "magicsock: home DERP is now 'sao'",
            ),
        ],
        "lab" => &[
            (
                "ferrisync-bench.service",
                Info,
                "run 14/30 iter {n}: 1.{n}2 GiB/s, p99 {n}.1 ms",
            ),
            (
                "rustfs.service",
                Info,
                "PUT ferrisync/run-14/{h}.tar.zst 200",
            ),
            ("taproot.service", Info, "sync ok, 0 changes"),
            (
                "nix-daemon.service",
                Info,
                "building '/nix/store/{h}-ferrisync-0.9.2.drv'",
            ),
            (
                "netbird.service",
                Info,
                "peer altar connected, relayed=false",
            ),
            (
                "nfs-server.service",
                Info,
                "client 100.119.88.94 renewed its lease",
            ),
            (
                "coeusd.service",
                Debug,
                "poll: generation 26.05.20261125 is current",
            ),
        ],
        "laptop-mac" => &[
            (
                "org.nixos.coeusd",
                Debug,
                "poll: darwin generation is current",
            ),
            (
                "org.nixos.tailscaled",
                Info,
                "magicsock: endpoints changed: 2 endpoints",
            ),
            (
                "org.nixos.nix-daemon",
                Info,
                "copied {n} paths from cache.r3dlust.com",
            ),
        ],
        "oracle-1" => &[
            (
                "upstreamer.service",
                Info,
                "GET /nix-cache-info 200 2ms host=cache.r3dlust.com",
            ),
            (
                "upstreamer.service",
                Info,
                "GET /{h}.narinfo 200 3ms host=cache.r3dlust.com",
            ),
            ("niks3.service", Info, "narinfo hit {h}-ripgrep-14.1.1"),
            ("niks3.service", Info, "upload 3 paths, 41.2 MiB, from lab"),
            ("heimdall.service", Info, "GET /api/health 200 1ms"),
            (
                "lavalink.service",
                Info,
                "player update: {n} players, 0 frames lost",
            ),
            ("rocky-bot.service", Info, "gateway heartbeat ack 4{n}ms"),
            (
                "tailscale-derper.service",
                Info,
                "derp: {n} clients connected",
            ),
            (
                "tailscaled.service",
                Debug,
                "magicsock: endpoints changed: 3 endpoints",
            ),
            (
                "coeusd.service",
                Debug,
                "poll: generation 26.05.20261124 is current",
            ),
            ("podman-yt-cipher.service", Info, "decipher ok, 1{n}ms"),
            (
                "postgresql.service",
                Info,
                "checkpoint complete: wrote 2{n}4 buffers",
            ),
        ],
        _ => &[
            ("upstreamer.service", Info, "GET / 200 1ms host=r3dlust.com"),
            (
                "upstreamer.service",
                Info,
                "GET /blog/ 200 2ms host=r3dlust.com",
            ),
            (
                "postfix.service",
                Info,
                "connect from mail-ej1-f41.google.com[209.85.218.41]",
            ),
            (
                "rspamd.service",
                Info,
                "scanned message, score 0.{n}/15, action no action",
            ),
            (
                "dovecot.service",
                Info,
                "imap-login: Login: user=<me>, method=PLAIN, TLS",
            ),
            (
                "adguardhome.service",
                Info,
                "dns: 4{n}2 queries in the last minute, 1{n}% blocked",
            ),
            ("copyparty.service", Info, "GET /pub/ 200 from 100.88.1.4"),
            (
                "tailscale-derper.service",
                Info,
                "derp: {n} clients connected",
            ),
            (
                "coeusd.service",
                Debug,
                "poll: generation 26.05.20261124 is current",
            ),
        ],
    }
}

/// the story's lines, oldest first
fn scripted() -> &'static [Entry] {
    static SCRIPTED: OnceLock<Vec<Entry>> = OnceLock::new();
    SCRIPTED.get_or_init(|| {
        use Level::{Error, Info, Warn};
        let oom = "java.lang.OutOfMemoryError: Java heap space";
        let exited = "lavalink.service: Main process exited, code=exited, status=1/FAILURE";
        let mut lines: Vec<(i64, &str, &str, Level, String)> = vec![
            (brt(13, 20, 0), "oracle-2", "systemd", Info, "Starting Nix Garbage Collector...".into()),
            (GC, "oracle-2", "nix-gc.service", Info, "deleting garbage... 1841 store paths deleted, 39.2 GiB freed".into()),
            (LAPTOP_SLEEPS - 2 * SECOND, "laptop-mac", "system.log", Info, "kernel[0]: PMRD: Sleep reason: Clamshell Sleep".into()),
            (brt(14, 56, 12), "oracle-1", "lavalink.service", Warn, "Java heap 1.96 GiB of 2.00 GiB in use, GC overhead 41%".into()),
            (LAVALINK_OOM, "oracle-1", "lavalink.service", Error, oom.into()),
            (LAVALINK_OOM + 300, "oracle-1", "systemd", Error, exited.into()),
            (LAVALINK_FAILED, "oracle-1", "systemd", Error, "lavalink.service: Start request repeated too quickly.".into()),
            (LAVALINK_FAILED + 200, "oracle-1", "systemd", Error, "lavalink.service: Failed with result 'exit-code'.".into()),
            (LAVALINK_FAILED + 400, "oracle-1", "systemd", Error, "Failed to start Lavalink audio node.".into()),
            (LAVALINK_RESTARTED - SECOND, "oracle-1", "systemd", Info, "Starting Lavalink audio node...".into()),
            (LAVALINK_RESTARTED, "oracle-1", "systemd", Info, "Started Lavalink audio node.".into()),
            (LAVALINK_RESTARTED + 11 * SECOND, "oracle-1", "rocky-bot.service", Info, "lavalink node 'main' connected, resuming 2 players".into()),
            (CERT_RETRY, "oracle-2", "tailscale-derper.service", Warn, "acme: renewing derp.r3dlust.com failed: 429 rate limited, retrying in 44m".into()),
            (ROUTE_LOST, "lab", "netbird.service", Warn, "route 0.0.0.0/0 via peer oracle-2 lost, falling back to direct".into()),
            (ROUTE_RESTORED, "lab", "netbird.service", Info, "route 0.0.0.0/0 via peer oracle-2 restored".into()),
            (LAB_PROBE_OK, "lab", "prometheus-blackbox-exporter.service", Info, "probe https://r3dlust.com ok, 200 in 27ms".into()),
            (CERT_RENEWED - 2 * SECOND, "oracle-2", "tailscale-derper.service", Info, "acme: renewing derp.r3dlust.com".into()),
            (CERT_RENEWED, "oracle-2", "tailscale-derper.service", Info, "acme: certificate for derp.r3dlust.com obtained, valid until 2027-02-23".into()),
            (LAPTOP_WAKES + SECOND, "laptop-mac", "system.log", Info, "kernel[0]: PMRD: Wake reason: EC.LidOpen".into()),
            (LAPTOP_WAKES + 4 * SECOND, "laptop-mac", "org.nixos.tailscaled", Info, "magicsock: home DERP is now 'sao'".into()),
        ];
        // restarted four times, out of memory 16 seconds into each
        for restart in 1..=4 {
            let at = brt(14, 57, 8) + (restart - 1) * 20_500;
            let counter = format!("lavalink.service: Scheduled restart job, restart counter is at {restart}.");
            lines.push((at, "oracle-1", "systemd", Warn, counter));
            lines.push((at + 16 * SECOND, "oracle-1", "lavalink.service", Error, oom.into()));
            lines.push((at + 16 * SECOND + 300, "oracle-1", "systemd", Error, exited.into()));
        }
        lines.extend(every(brt(14, 59, 10), LAVALINK_RESTARTED, 150 * SECOND).map(|at| {
            let text = "lavalink node 'main' unreachable: connection refused 127.0.0.1:2333";
            (at, "oracle-1", "rocky-bot.service", Warn, text.into())
        }));
        lines.extend(every(LAB_PROBE_FAILS, LAB_PROBE_OK, MINUTE).map(|at| {
            let text = "probe https://r3dlust.com: context deadline exceeded after 10s";
            (at, "lab", "prometheus-blackbox-exporter.service", Error, text.into())
        }));
        for (from, to) in DESKTOP_UP {
            lines.push((from + 14 * SECOND, "desktop-nixos", "systemd", Info, "Startup finished in 3.1s (firmware) + 2.4s (loader) + 1.9s (kernel) + 6.8s (userspace) = 14.2s.".into()));
            lines.push((to - 3 * SECOND, "desktop-nixos", "systemd-logind.service", Info, "Power key pressed short.".into()));
        }

        let mut entries: Vec<Entry> = lines
            .into_iter()
            .enumerate()
            .map(|(i, (at, machine, unit, level, text))| Entry {
                id: mix(0x5c21_97ed, i as i64),
                line: LogLine {
                    at: Ms(at),
                    machine: Hostname::from(machine),
                    unit: unit.into(),
                    level,
                    text,
                },
            })
            .collect();
        entries.sort_by_key(Entry::key);
        entries
    })
}

/// `from`, then every `period` before `to`
fn every(from: i64, to: i64, period: i64) -> impl Iterator<Item = i64> {
    (0..)
        .map(move |k| from + k * period)
        .take_while(move |at| *at < to)
}
