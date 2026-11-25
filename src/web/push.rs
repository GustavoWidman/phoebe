//! `/api/push`: what the app needs to turn notifications on for a device,
//! and to tell whether phoebe already pushes to it. the key is the public
//! half of the one pushes are signed with, and the list is the one they go
//! to, both read on every request as on every send, so an edit reaches the
//! app without a restart. the answer names subscriptions by a hash of their
//! endpoint, so it reveals none of them.

use eyre::Result;
use sha2::{Digest, Sha256};

use super::{live::hook::PushSettings, wire::PushView};

/// where `/api/push`'s answer comes from
#[derive(Clone, Debug)]
pub enum Push {
    /// the node's: what it pushes with, and the command that edits the list
    Live {
        settings: PushSettings,
        edit: Option<String>,
    },
    /// always the same answer
    Fixed(PushView),
}

impl Push {
    pub async fn view(&self) -> Result<PushView> {
        match self {
            Self::Fixed(view) => Ok(view.clone()),
            Self::Live { settings, edit } => {
                let settings = settings.clone();
                let (key, devices) = tokio::task::spawn_blocking(move || {
                    Ok::<_, eyre::Report>((settings.key()?, settings.devices()?))
                })
                .await??;
                Ok(PushView {
                    key: key.public_key(),
                    subscribed: devices
                        .iter()
                        .map(|device| endpoint_hash(device.subscription.endpoint()))
                        .collect(),
                    edit: edit.clone(),
                })
            }
        }
    }
}

/// the first 16 hex digits of the SHA-256 of `endpoint`, which the app
/// computes the same way for its own subscription
pub fn endpoint_hash(endpoint: &str) -> String {
    Sha256::digest(endpoint.as_bytes())[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
