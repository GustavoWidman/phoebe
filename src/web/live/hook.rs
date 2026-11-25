//! `POST /hook`: where the node's Alertmanager posts its notices. a page
//! alert's firing and its resolution go out as Declarative Web Push, and
//! every alert of every notice is logged as one [NoticeLine], which the
//! data plane ships to every node's Loki. the cluster sends each notice
//! from one node, so one phoebe pushes it; when that phoebe fails to load
//! what it sends with, it answers 500, and another node's sends instead.

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime},
};

use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use eyre::{Result, WrapErr, bail, ensure, eyre};
use gjallar::{
    Client, Dir, Message, Notification, Outcome, Subscription, Topic, Urgency, VapidKey, WebPush,
};
use log::{error, info, warn};
use serde::Deserialize;

use super::{
    alertmanager::{Notice, NoticeAlert, NoticeStatus},
    alerts::NoticeLine,
};

/// how long one push service may take to answer
const TIMEOUT: Duration = Duration::from_secs(5);
/// how long one send may take, retries included
const DEADLINE: Duration = Duration::from_secs(10);
/// a notice older than a day is noise
const TTL: Duration = Duration::from_secs(24 * 3_600);

/// where pushes come from and who gets them. both files are read at every
/// send, so changing them needs no restart
#[derive(Clone, Debug)]
pub struct PushSettings {
    /// the app's `https` origin: the VAPID subject, and where a tap leads
    pub origin: String,
    /// the base64url of a P-256 private scalar
    pub key_file: PathBuf,
    /// a JSON array of `PushSubscription.toJSON()` objects, each with a `name`
    pub subscriptions_file: PathBuf,
}

impl PushSettings {
    pub fn new(origin: String, key_file: PathBuf, subscriptions_file: PathBuf) -> Result<Self> {
        let Some(host) = origin.strip_prefix("https://") else {
            bail!("the origin starts with https:// ({origin:?})");
        };
        let host = host.trim_end_matches('/');
        ensure!(
            !host.is_empty() && !host.contains(['/', '?', '#', '@']),
            "the origin is https:// and a host, with no path ({origin:?})"
        );
        Ok(Self {
            origin: format!("https://{host}"),
            key_file,
            subscriptions_file,
        })
    }

    /// the key and the subscriptions, as the files hold them now
    fn load(&self) -> Result<(Client, Vec<Device>)> {
        let key = std::fs::read_to_string(&self.key_file)
            .wrap_err_with(|| format!("failed to read {}", self.key_file.display()))?;
        let key = VapidKey::from_base64url(key.trim()).map_err(|_| {
            eyre!(
                "{} does not hold a VAPID private key (the base64url of a P-256 scalar)",
                self.key_file.display()
            )
        })?;
        let devices = std::fs::read(&self.subscriptions_file)
            .wrap_err_with(|| format!("failed to read {}", self.subscriptions_file.display()))
            .and_then(|json| devices(&json))
            .wrap_err_with(|| format!("in {}", self.subscriptions_file.display()))?;
        let push = WebPush::new(key, &self.origin)?;
        Ok((
            Client::new(push).timeout(TIMEOUT).deadline(DEADLINE),
            devices,
        ))
    }
}

/// a subscription, named so a log line can say which device it is
struct Device {
    name: String,
    subscription: Subscription,
}

fn devices(json: &[u8]) -> Result<Vec<Device>> {
    #[derive(Deserialize)]
    struct Entry {
        name: String,
        #[serde(flatten)]
        subscription: serde_json::Value,
    }
    let entries: Vec<Entry> = serde_json::from_slice(json)
        .wrap_err("expected a JSON array of subscriptions, each with a name")?;
    entries
        .into_iter()
        .map(|Entry { name, subscription }| {
            let subscription = serde_json::from_value(subscription)
                .wrap_err_with(|| format!("subscription {name:?}"))?;
            Ok(Device { name, subscription })
        })
        .collect()
}

/// the push an alert of a notice makes, if it pages
fn message(alert: &NoticeAlert, origin: &str) -> Result<Option<Message>> {
    if alert.labels.get("severity").map(String::as_str) != Some("page") {
        return Ok(None);
    }
    let (at, urgency) = match alert.status {
        NoticeStatus::Firing => (alert.starts_at, Urgency::High),
        NoticeStatus::Resolved => (alert.ends_at, Urgency::Normal),
    };
    let notification = Notification::new(
        summary(alert),
        format!("{origin}/alerts/{}", alert.fingerprint),
    )
    // the resolution replaces the firing on the device
    .tag(&alert.fingerprint.0)
    .body(body(alert))
    .timestamp(SystemTime::UNIX_EPOCH + Duration::from_millis(at.0.max(0) as u64))
    .lang("en")
    .dir(Dir::Ltr);
    Ok(Some(
        Message::declarative(notification)?
            .ttl(TTL)
            .urgency(urgency)
            // and replaces it at the push service while it waits there
            .topic(Topic::digest(&alert.fingerprint.0)),
    ))
}

/// the line under the summary: what the alert is doing
fn body(alert: &NoticeAlert) -> String {
    let rule = alert
        .labels
        .get("alertname")
        .map_or("an alert", String::as_str);
    match alert.status {
        NoticeStatus::Firing => format!("{rule}, firing"),
        NoticeStatus::Resolved => format!(
            "Resolved after {}",
            lasted(alert.ends_at.0 - alert.starts_at.0)
        ),
    }
}

fn summary(alert: &NoticeAlert) -> String {
    alert
        .annotations
        .get("summary")
        .cloned()
        .unwrap_or_else(|| alert.labels.get("alertname").cloned().unwrap_or_default())
}

/// a duration in the largest unit that fits it, as a person says it
fn lasted(ms: i64) -> String {
    let minutes = (ms / 60_000).max(1);
    let (count, unit) = match minutes {
        0..60 => (minutes, "minute"),
        60..1_440 => (minutes / 60, "hour"),
        _ => (minutes / 1_440, "day"),
    };
    format!("{count} {unit}{}", if count == 1 { "" } else { "s" })
}

/// the hook, for the Alertmanager on this machine alone: it listens on
/// loopback, apart from the site
pub fn router(push: Option<PushSettings>) -> Router {
    Router::new()
        .route("/hook", post(hook))
        .with_state(Arc::new(push))
}

async fn hook(
    State(push): State<Arc<Option<PushSettings>>>,
    Json(notice): Json<Notice>,
) -> StatusCode {
    let sent = tokio::task::spawn_blocking(move || send(push.as_ref().as_ref(), &notice)).await;
    match sent {
        Ok(Ok(())) => StatusCode::OK,
        Ok(Err(failure)) => {
            error!("{failure:#}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
        Err(panic) => {
            error!("the hook failed: {panic}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

/// pushes every paging alert of the notice and logs every alert of it.
/// failing to load the key or the subscriptions fails the notice, so
/// Alertmanager tries again, or another node does
fn send(push: Option<&PushSettings>, notice: &Notice) -> Result<()> {
    let pages: Vec<(&NoticeAlert, Message)> = match push {
        Some(push) => notice
            .alerts
            .iter()
            .filter_map(|alert| {
                message(alert, &push.origin)
                    .transpose()
                    .map(|m| m.map(|m| (alert, m)))
            })
            .collect::<Result<_>>()?,
        None => Vec::new(),
    };
    let loaded = match (push, pages.is_empty()) {
        (Some(push), false) => Some(push.load()?),
        _ => None,
    };

    for alert in &notice.alerts {
        let mut line = NoticeLine {
            alert: alert.fingerprint.clone(),
            resolved: alert.status == NoticeStatus::Resolved,
            rule: alert.labels.get("alertname").cloned().unwrap_or_default(),
            summary: summary(alert),
            devices: 0,
            delivered: 0,
        };
        let page = pages.iter().find(|(page, _)| std::ptr::eq(*page, alert));
        if let (Some((_, message)), Some((client, devices))) = (page, &loaded) {
            let outcomes =
                client.send_all(devices.iter().map(|device| &device.subscription), message);
            line.devices = devices.len();
            for (device, outcome) in devices.iter().zip(outcomes) {
                match outcome {
                    Ok(Outcome::Delivered) => line.delivered += 1,
                    Ok(Outcome::Gone { status }) => warn!(
                        "{} dropped its subscription ({status}); take it off the list",
                        device.name
                    ),
                    Ok(other) => warn!("{} was not pushed to: {other:?}", device.name),
                    Err(error) => warn!("{} was not pushed to: {error}", device.name),
                }
            }
        }
        info!("{}", line.text());
    }
    Ok(())
}
