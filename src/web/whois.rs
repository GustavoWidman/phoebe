//! who is calling on the tailnet. WireGuard authenticates a peer's tailnet
//! address, and `tailscale whois` names the login and node behind it, with
//! the app capabilities the tailnet policy grants that node toward this
//! one. a node signs in when the policy grants it phoebe's capability.

use std::{
    collections::{BTreeSet, HashMap},
    net::IpAddr,
    sync::Mutex,
    time::{Duration, Instant},
};

use eyre::{Result, WrapErr, eyre};
use serde::Deserialize;

use super::tailnet::tailscale;

/// the capability a tailnet policy grant gives the nodes that may sign in,
/// unless `--capability` names another
pub const CAPABILITY: &str = "r3dlust.com/cap/phoebe";

/// who a tailnet peer is
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    /// the user's login, `tagged-devices` for a tagged node
    pub login: String,
    /// the node's MagicDNS name, without the tailnet's domain
    pub device: String,
    /// the app capabilities granted to it toward this node
    pub capabilities: BTreeSet<String>,
}

impl Peer {
    /// `tailscale whois --json`'s answer
    pub fn parse(json: &[u8]) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Answer {
            node: Node,
            user_profile: Profile,
            /// capability name to its values, `null` when none is granted
            #[serde(default)]
            cap_map: Option<HashMap<String, serde_json::Value>>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Node {
            name: String,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Profile {
            login_name: String,
        }

        let answer: Answer =
            serde_json::from_slice(json).wrap_err("tailscale whois answered something else")?;
        let device = answer
            .node
            .name
            .split('.')
            .next()
            .filter(|label| !label.is_empty())
            .ok_or_else(|| eyre!("tailscale whois named no node"))?;
        Ok(Self {
            login: answer.user_profile.login_name,
            device: device.to_owned(),
            capabilities: answer.cap_map.unwrap_or_default().into_keys().collect(),
        })
    }

    /// whether the tailnet policy grants it `capability` toward this node
    pub fn granted(&self, capability: &str) -> bool {
        self.capabilities.contains(capability)
    }
}

/// how a peer address is put a name to
pub enum Whois {
    /// this machine's tailscale, each answer kept for [Whois::KEEP]
    Tailscale(Mutex<HashMap<IpAddr, (Instant, Option<Peer>)>>),
    /// everyone is this peer: a simulated fleet has no tailnet
    Fixed(Peer),
}

impl Whois {
    /// how long an answer stands; revoking a grant takes this long to bite
    pub const KEEP: Duration = Duration::from_secs(60);

    pub fn tailscale() -> Self {
        Self::Tailscale(Mutex::default())
    }

    /// who is at `address`, `None` for an address tailscale knows no peer
    /// at; an error when tailscale cannot be asked
    pub async fn ask(&self, address: IpAddr) -> Result<Option<Peer>> {
        let known = match self {
            Self::Fixed(peer) => return Ok(Some(peer.clone())),
            Self::Tailscale(known) => known,
        };
        if let Some((_, peer)) = known
            .lock()
            .unwrap()
            .get(&address)
            .filter(|(asked, _)| asked.elapsed() < Self::KEEP)
        {
            return Ok(peer.clone());
        }

        let output = tailscale(&["whois", "--json", &address.to_string()])
            .await
            .ok_or_else(|| eyre!("tailscale did not answer"))?;
        let said = String::from_utf8_lossy(&output.stderr);
        let peer = match output.status.success() {
            true => Some(Peer::parse(&output.stdout)?),
            false if said.contains("peer not found") => None,
            false => return Err(eyre!("tailscale whois failed: {}", said.trim())),
        };
        known
            .lock()
            .unwrap()
            .insert(address, (Instant::now(), peer.clone()));
        Ok(peer)
    }
}
