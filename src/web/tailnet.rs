//! this machine's tailscale, through its CLI: whether it is in the tailnet,
//! and at which addresses. any local user may ask; no operator rights are
//! needed.

use std::{net::IpAddr, process::Output, time::Duration};

use serde::{Deserialize, Deserializer};

/// how long tailscale may take to answer
const PATIENCE: Duration = Duration::from_secs(5);

/// what the tailscale CLI says to `args`, wherever it is installed; `None`
/// when none answers in time
pub async fn tailscale(args: &[&str]) -> Option<Output> {
    for program in [
        "tailscale",
        "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
    ] {
        let output = tokio::process::Command::new(program)
            .args(args)
            .kill_on_drop(true)
            .output();
        if let Ok(Ok(output)) = tokio::time::timeout(PATIENCE, output).await {
            return Some(output);
        }
    }
    None
}

/// `tailscale status --json --peers=false`, as much of it as says whether
/// this machine is in touch with the coordination server and its relays,
/// and at which addresses
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Tailnet {
    #[serde(default)]
    backend_state: String,
    #[serde(rename = "Self")]
    this: Option<Node>,
    /// tailscale's warnings, in its own words
    #[serde(default, deserialize_with = "empty_if_null")]
    health: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Node {
    #[serde(rename = "TailscaleIPs", default, deserialize_with = "empty_if_null")]
    tailscale_ips: Vec<IpAddr>,
    #[serde(default)]
    online: bool,
}

/// tailscale writes an empty list as `null`, as it does for every one of
/// them while logged out
fn empty_if_null<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}

impl Tailnet {
    /// what tailscale's warnings say when it cannot reach its coordination
    /// server or its home relay, or has no network at all. tailscale names
    /// them only in prose, so these are phrases from its `health` package
    /// (1.104), shared by every such warning and by no other.
    const CUT_OFF: [&str; 3] = ["coordination server", "relay server", "network is down"];

    /// the tailnet as this machine's tailscale sees it, `None` when no
    /// tailscale answers
    pub async fn read() -> Option<Self> {
        Self::parse(
            &tailscale(&["status", "--json", "--peers=false"])
                .await?
                .stdout,
        )
    }

    pub fn parse(status: &[u8]) -> Option<Self> {
        serde_json::from_slice(status).ok()
    }

    /// why this machine is not fully in the tailnet, `None` when tailscale
    /// runs, holds its long poll to the coordination server and warns about
    /// neither that server nor its relays. `Self.Online` is that poll, so it
    /// drops at once; tailscale holds back its warning about it for minutes.
    pub fn trouble(&self) -> Option<String> {
        if self.backend_state != "Running" {
            return Some(format!("tailscale is {}", self.backend_state));
        }
        if !self.this.as_ref().is_some_and(|this| this.online) {
            return Some("tailscale is out of touch with its coordination server".to_owned());
        }
        self.health
            .iter()
            .find(|warning| Self::CUT_OFF.iter().any(|phrase| warning.contains(phrase)))
            .map(|warning| format!("tailscale warns: {warning}"))
    }

    /// this machine's own tailnet addresses
    pub fn addresses(&self) -> Vec<IpAddr> {
        self.this
            .as_ref()
            .map(|this| this.tailscale_ips.clone())
            .unwrap_or_default()
    }
}
