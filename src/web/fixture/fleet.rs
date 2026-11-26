//! the simulated fleet's registry: the real fleet's machines, services and
//! probes, in the order `services.phoebe.fleet` lists them

use crate::web::wire::{Os, ProbeKind};

pub const SERVED_BY: &str = "lab";

/// the nodes, in registry order
pub const NODES: [&str; 4] = ["altar", "lab", "oracle-1", "oracle-2"];

pub struct MachineSpec {
    pub name: &'static str,
    pub os: Os,
    pub always_on: bool,
    pub arch: &'static str,
    pub cores: u32,
    pub mem_gib: u64,
    pub disk_gb: u64,
    /// how many units or launchd jobs run while it is up
    pub units: u32,
}

pub const MACHINES: [MachineSpec; 6] = [
    MachineSpec {
        name: "altar",
        os: Os::Darwin,
        always_on: true,
        arch: "aarch64",
        cores: 10,
        mem_gib: 32,
        disk_gb: 1000,
        units: 87,
    },
    MachineSpec {
        name: "desktop-nixos",
        os: Os::NixOs,
        always_on: false,
        arch: "x86_64",
        cores: 16,
        mem_gib: 32,
        disk_gb: 1000,
        units: 176,
    },
    MachineSpec {
        name: "lab",
        os: Os::NixOs,
        always_on: true,
        arch: "x86_64",
        cores: 16,
        mem_gib: 64,
        disk_gb: 2000,
        units: 198,
    },
    MachineSpec {
        name: "laptop-mac",
        os: Os::Darwin,
        always_on: false,
        arch: "aarch64",
        cores: 12,
        mem_gib: 36,
        disk_gb: 1000,
        units: 92,
    },
    MachineSpec {
        name: "oracle-1",
        os: Os::NixOs,
        always_on: true,
        arch: "aarch64",
        cores: 4,
        mem_gib: 24,
        disk_gb: 200,
        units: 214,
    },
    MachineSpec {
        name: "oracle-2",
        os: Os::NixOs,
        always_on: true,
        arch: "aarch64",
        cores: 4,
        mem_gib: 24,
        disk_gb: 200,
        units: 231,
    },
];

pub struct ServiceSpec {
    pub name: &'static str,
    pub machine: &'static str,
    pub unit: Option<&'static str>,
    pub probes: &'static [ProbeSpec],
    /// what answering costs it, in milliseconds, past the network
    pub overhead_ms: f64,
    pub heartbeat: Option<HeartbeatSpec>,
}

/// a batch job's heartbeat: it runs every `every_ms`, healthy each time
pub struct HeartbeatSpec {
    pub metric: &'static str,
    pub every_ms: i64,
    pub within_ms: i64,
}

pub struct ProbeSpec {
    pub kind: ProbeKind,
    pub target: &'static str,
    pub tls: bool,
}

const fn http(target: &'static str) -> ProbeSpec {
    ProbeSpec {
        kind: ProbeKind::Http,
        target,
        tls: false,
    }
}

const fn over_tls(kind: ProbeKind, target: &'static str) -> ProbeSpec {
    ProbeSpec {
        kind,
        target,
        tls: true,
    }
}

const fn tcp(target: &'static str) -> ProbeSpec {
    ProbeSpec {
        kind: ProbeKind::Tcp,
        target,
        tls: false,
    }
}

pub const SERVICES: [ServiceSpec; 17] = [
    ServiceSpec {
        name: "adguard",
        machine: "oracle-2",
        unit: Some("adguardhome.service"),
        probes: &[
            http("https://dns.r3dlust.com/dns-query?dns=AAABAAABAAAAAAAAB2V4YW1wbGUDY29tAAABAAE"),
            over_tls(ProbeKind::Tcp, "dns.r3dlust.com:853"),
        ],
        overhead_ms: 0.6,
        heartbeat: None,
    },
    ServiceSpec {
        name: "copyparty",
        machine: "oracle-2",
        unit: Some("copyparty.service"),
        probes: &[http("https://files.r3dlust.com")],
        overhead_ms: 5.0,
        heartbeat: None,
    },
    ServiceSpec {
        name: "derp",
        machine: "oracle-2",
        unit: Some("tailscale-derper.service"),
        probes: &[http("https://derp.r3dlust.com")],
        overhead_ms: 0.3,
        heartbeat: None,
    },
    ServiceSpec {
        name: "dns-sync",
        machine: "oracle-1",
        unit: None,
        probes: &[],
        overhead_ms: 0.0,
        heartbeat: Some(HeartbeatSpec {
            metric: "dns_sync_healthy_timestamp_seconds",
            every_ms: 5 * 60_000,
            within_ms: 3_600_000,
        }),
    },
    ServiceSpec {
        name: "heimdall",
        machine: "oracle-1",
        unit: Some("heimdall.service"),
        probes: &[http("https://heimdall.r3dlust.com/api/health")],
        overhead_ms: 3.0,
        heartbeat: None,
    },
    ServiceSpec {
        name: "lavalink",
        machine: "oracle-1",
        unit: Some("lavalink.service"),
        probes: &[],
        overhead_ms: 0.0,
        heartbeat: None,
    },
    ServiceSpec {
        name: "mail",
        machine: "oracle-2",
        unit: Some("postfix.service"),
        probes: &[
            over_tls(ProbeKind::Smtp, "mail.r3dlust.com:465"),
            over_tls(ProbeKind::Imap, "mail.r3dlust.com:993"),
            http("https://mail.r3dlust.com"),
        ],
        overhead_ms: 2.0,
        heartbeat: None,
    },
    ServiceSpec {
        name: "mcsmanager",
        machine: "oracle-2",
        unit: Some("arion-mcsmanager.service"),
        probes: &[http("https://minecraft.r3dlust.com")],
        overhead_ms: 6.0,
        heartbeat: None,
    },
    ServiceSpec {
        name: "nfs",
        machine: "lab",
        unit: Some("nfs-server.service"),
        probes: &[tcp("lab.tail4a3ea.ts.net:2049")],
        overhead_ms: 0.2,
        heartbeat: None,
    },
    ServiceSpec {
        name: "niks3",
        machine: "oracle-1",
        unit: Some("niks3.service"),
        probes: &[http("https://cache.r3dlust.com/nix-cache-info")],
        overhead_ms: 4.0,
        heartbeat: None,
    },
    ServiceSpec {
        name: "portfolio",
        machine: "oracle-2",
        unit: None,
        probes: &[
            http("https://r3dlust.com"),
            http("https://blog.r3dlust.com"),
            http("https://guswid.com"),
            http("https://blog.guswid.com"),
        ],
        overhead_ms: 3.0,
        heartbeat: None,
    },
    ServiceSpec {
        name: "rocky-bot",
        machine: "oracle-1",
        unit: Some("rocky-bot.service"),
        probes: &[],
        overhead_ms: 0.0,
        heartbeat: None,
    },
    ServiceSpec {
        name: "rustfs",
        machine: "lab",
        unit: Some("rustfs.service"),
        probes: &[http("http://lab.tail4a3ea.ts.net:9000/health")],
        overhead_ms: 1.0,
        heartbeat: None,
    },
    ServiceSpec {
        name: "tailscale-proxy",
        machine: "oracle-2",
        unit: None,
        probes: &[http("https://tailscale.r3dlust.com/key")],
        overhead_ms: 2.0,
        heartbeat: None,
    },
    ServiceSpec {
        name: "taproot",
        machine: "lab",
        unit: Some("taproot.service"),
        probes: &[http("https://lab.tail4a3ea.ts.net:8317/healthz")],
        overhead_ms: 1.5,
        heartbeat: None,
    },
    ServiceSpec {
        name: "teamspeak6",
        machine: "oracle-1",
        unit: Some("podman-teamspeak6.service"),
        probes: &[tcp("rockyclan.com:30033")],
        overhead_ms: 0.5,
        heartbeat: None,
    },
    ServiceSpec {
        name: "yt-cipher",
        machine: "oracle-1",
        unit: Some("podman-yt-cipher.service"),
        probes: &[],
        overhead_ms: 0.0,
        heartbeat: None,
    },
];

pub fn machine(name: &str) -> Option<&'static MachineSpec> {
    MACHINES.iter().find(|machine| machine.name == name)
}

pub fn service(name: &str) -> Option<&'static ServiceSpec> {
    SERVICES.iter().find(|service| service.name == name)
}

impl ProbeSpec {
    /// whether it speaks TLS, as an https URL does
    pub fn certified(&self) -> bool {
        self.tls || self.target.starts_with("https://")
    }

    /// the host it reaches
    pub fn host(&self) -> &'static str {
        let rest = self
            .target
            .split_once("://")
            .map_or(self.target, |(_, rest)| rest);
        rest.split(['/', ':', '?']).next().unwrap_or(rest)
    }
}
