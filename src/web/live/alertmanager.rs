//! the node's Alertmanager, which holds what the whole cluster fires, and
//! the notices it posts to the hook

use std::{collections::BTreeMap, time::Duration};

use eyre::{Result, WrapErr, bail};
use serde::{Deserialize, Deserializer};

use super::prometheus::Labels;
use crate::web::wire::{AlertId, Ms};

/// an alert as the cluster holds it, from `/api/v2/alerts`
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterAlert {
    pub fingerprint: AlertId,
    pub labels: Labels,
    #[serde(default)]
    pub annotations: BTreeMap<String, String>,
    /// when the first node fired it
    #[serde(deserialize_with = "timestamp")]
    pub starts_at: Ms,
    /// a resolved alert ends in the past, a firing one when it would
    /// resolve if no node fired it again
    #[serde(deserialize_with = "timestamp")]
    pub ends_at: Ms,
}

/// one alert of a notice Alertmanager posts to its webhook
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoticeAlert {
    pub status: NoticeStatus,
    pub fingerprint: AlertId,
    pub labels: Labels,
    #[serde(default)]
    pub annotations: BTreeMap<String, String>,
    #[serde(deserialize_with = "timestamp")]
    pub starts_at: Ms,
    #[serde(deserialize_with = "timestamp")]
    pub ends_at: Ms,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoticeStatus {
    Firing,
    Resolved,
}

/// the body of a webhook post, version 4
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Notice {
    pub alerts: Vec<NoticeAlert>,
}

/// an RFC 3339 time, as Alertmanager writes them; its zero time, which
/// leaves an end unset, reads as 0
fn timestamp<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Ms, D::Error> {
    let text = String::deserialize(deserializer)?;
    let at: jiff::Timestamp = text.parse().map_err(serde::de::Error::custom)?;
    Ok(Ms(at.as_millisecond().max(0)))
}

#[derive(Clone)]
pub struct Alertmanager {
    http: reqwest::Client,
    base: String,
}

impl Alertmanager {
    pub fn new(base: String, timeout: Duration) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .expect("a plain HTTP client always builds"),
            base,
        }
    }

    pub async fn alerts_body(&self) -> Result<String> {
        let response = self
            .http
            .get(format!("{}/api/v2/alerts", self.base))
            .send()
            .await
            .wrap_err_with(|| format!("{} did not answer", self.base))?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            bail!("{} answered {status}: {}", self.base, body.trim());
        }
        Ok(body)
    }

    /// every alert the cluster fires at `now`
    pub async fn firing(&self, now: i64) -> Result<Vec<ClusterAlert>> {
        firing(&self.alerts_body().await?, now)
    }
}

/// the alerts of `/api/v2/alerts` that still fire at `now`
pub fn firing(body: &str, now: i64) -> Result<Vec<ClusterAlert>> {
    let alerts: Vec<ClusterAlert> =
        serde_json::from_str(body).wrap_err("not an answer of Alertmanager's API")?;
    Ok(alerts
        .into_iter()
        .filter(|alert| alert.ends_at.0 > now)
        .collect())
}
