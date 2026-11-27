//! the fleet's registry as every machine writes it to `/etc/phoebe/fleet.json`,
//! parsed and checked once, at start

use std::{collections::BTreeMap, path::Path};

use eyre::{Result, WrapErr, bail};
use serde::Deserialize;

use crate::web::wire::{Hostname, Os, ProbeKind, ServiceId, ServiceName};

#[derive(Clone, Debug)]
pub struct Registry {
    /// the machine this file was written on
    pub machine: Hostname,
    /// in registry order
    pub nodes: Vec<Hostname>,
    pub machines: BTreeMap<Hostname, MachineEntry>,
    /// by name, then by machine
    pub services: BTreeMap<ServiceId, ServiceEntry>,
    pub ports: Ports,
}

/// the registry as the file holds it, its services a list
#[derive(Deserialize)]
struct File {
    machine: Hostname,
    nodes: Vec<Hostname>,
    machines: BTreeMap<Hostname, MachineEntry>,
    services: Vec<Listed>,
    ports: Ports,
}

#[derive(Deserialize)]
struct Listed {
    name: ServiceName,
    machine: Hostname,
    #[serde(flatten)]
    entry: ServiceEntry,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineEntry {
    pub os: Os,
    pub always_on: bool,
    /// its tailnet address
    pub address: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServiceEntry {
    pub unit: Option<String>,
    pub probes: Vec<ProbeEntry>,
    pub heartbeat: Option<HeartbeatEntry>,
}

/// a gauge on the service's machine, holding the unix time the service
/// last knew itself healthy
#[derive(Clone, Debug, Deserialize)]
pub struct HeartbeatEntry {
    /// a Prometheus metric name, checked as one at parse
    pub metric: String,
    /// seconds the gauge may fall behind
    pub within: u64,
}

/// whether `name` is a Prometheus metric name, `[a-zA-Z_:][a-zA-Z0-9_:]*`,
/// which PromQL takes as it is
fn is_metric_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == ':')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
}

#[derive(Clone, Debug, Deserialize)]
pub struct ProbeEntry {
    pub kind: ProbeKind,
    pub target: String,
    tls: bool,
}

impl ProbeEntry {
    /// whether it speaks TLS from the first byte: an https URL, or a port
    /// the registry says starts with TLS
    pub fn tls(&self) -> bool {
        match self.kind {
            ProbeKind::Http => self.target.starts_with("https://"),
            _ => self.tls,
        }
    }

    /// the host it reaches, which log lines about the service name
    pub fn host(&self) -> &str {
        let rest = self
            .target
            .split_once("://")
            .map_or(self.target.as_str(), |(_, rest)| rest);
        let authority = rest.split(['/', '?']).next().unwrap_or(rest);
        authority
            .rsplit_once(':')
            .map_or(authority, |(host, _)| host)
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Ports {
    pub prometheus: u16,
    pub loki: u16,
    pub alertmanager: u16,
}

impl Registry {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read(path).wrap_err_with(|| format!("failed to read {}", path.display()))?;
        Self::parse(&text).wrap_err_with(|| format!("in {}", path.display()))
    }

    pub fn parse(json: &[u8]) -> Result<Self> {
        let file: File = serde_json::from_slice(json).wrap_err("not a phoebe registry")?;
        let mut services = BTreeMap::new();
        for Listed {
            name,
            machine,
            entry,
        } in file.services
        {
            let id = ServiceId { name, machine };
            if services.contains_key(&id) {
                bail!("{id} is registered twice, but a machine runs at most one service of each name");
            }
            services.insert(id, entry);
        }
        let registry = Self {
            machine: file.machine,
            nodes: file.nodes,
            machines: file.machines,
            services,
            ports: file.ports,
        };
        for name in std::iter::once(&registry.machine)
            .chain(&registry.nodes)
            .chain(registry.services.keys().map(|id| &id.machine))
        {
            if !registry.machines.contains_key(name) {
                bail!("{name} is named, but not among the machines");
            }
        }
        for (id, service) in &registry.services {
            if let Some(heartbeat) = &service.heartbeat
                && !is_metric_name(&heartbeat.metric)
            {
                bail!(
                    "the heartbeat of {id}, {:?}, is not a metric name",
                    heartbeat.metric
                );
            }
        }
        if !registry.nodes.contains(&registry.machine) {
            bail!(
                "{} is not a node, so it has no stores to read",
                registry.machine
            );
        }
        Ok(registry)
    }

    pub fn machine(&self, name: &Hostname) -> Option<&MachineEntry> {
        self.machines.get(name)
    }

    pub fn service(&self, id: &ServiceId) -> Option<&ServiceEntry> {
        self.services.get(id)
    }

    /// `http://<address>:<port>` of a machine in the registry
    pub fn url(&self, machine: &Hostname, port: u16) -> String {
        format!("http://{}:{port}", self.machines[machine].address)
    }

    /// the units of every service that has one
    pub fn units(&self) -> impl Iterator<Item = (&ServiceId, &str)> {
        self.services
            .iter()
            .filter_map(|(id, service)| service.unit.as_deref().map(|unit| (id, unit)))
    }

    /// the heartbeats of every service that has one
    pub fn heartbeats(&self) -> impl Iterator<Item = (&ServiceId, &HeartbeatEntry)> {
        self.services
            .iter()
            .filter_map(|(id, service)| service.heartbeat.as_ref().map(|heartbeat| (id, heartbeat)))
    }
}
