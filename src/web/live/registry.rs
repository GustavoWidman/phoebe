//! the fleet's registry as every machine writes it to `/etc/phoebe/fleet.json`,
//! parsed and checked once, at start

use std::{collections::BTreeMap, path::Path};

use eyre::{Result, WrapErr, bail};
use serde::Deserialize;

use crate::web::wire::{Hostname, Os, ProbeKind, ServiceName};

#[derive(Clone, Debug, Deserialize)]
pub struct Registry {
    /// the machine this file was written on
    pub machine: Hostname,
    /// in registry order
    pub nodes: Vec<Hostname>,
    pub machines: BTreeMap<Hostname, MachineEntry>,
    pub services: BTreeMap<ServiceName, ServiceEntry>,
    pub ports: Ports,
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
    pub machine: Hostname,
    pub unit: Option<String>,
    pub probes: Vec<ProbeEntry>,
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
        let registry: Self = serde_json::from_slice(json).wrap_err("not a phoebe registry")?;
        for name in std::iter::once(&registry.machine)
            .chain(&registry.nodes)
            .chain(registry.services.values().map(|service| &service.machine))
        {
            if !registry.machines.contains_key(name) {
                bail!("{name} is named, but not among the machines");
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

    /// `http://<address>:<port>` of a machine in the registry
    pub fn url(&self, machine: &Hostname, port: u16) -> String {
        format!("http://{}:{port}", self.machines[machine].address)
    }

    /// the units of every service that has one, with their machines
    pub fn units(&self) -> impl Iterator<Item = (&ServiceName, &Hostname, &str)> {
        self.services.iter().filter_map(|(name, service)| {
            service
                .unit
                .as_deref()
                .map(|unit| (name, &service.machine, unit))
        })
    }
}
