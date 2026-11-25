//! a node's Prometheus over its HTTP API, its answers parsed where they
//! arrive

use std::{collections::BTreeMap, time::Duration};

use eyre::{Result, WrapErr, bail, eyre};
use serde::Deserialize;

pub type Labels = BTreeMap<String, String>;

/// one series of an instant query; a value that is not a number is dropped
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub labels: Labels,
    pub value: f64,
}

/// one series of a range query, its points in epoch milliseconds; a value
/// that is not a number is unset
#[derive(Clone, Debug, PartialEq)]
pub struct Range {
    pub labels: Labels,
    pub points: Vec<(i64, Option<f64>)>,
}

/// an alert as one Prometheus evaluates it, from `/api/v1/alerts`
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RuleAlert {
    pub labels: Labels,
    #[serde(default)]
    pub annotations: Labels,
    pub state: RuleState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleState {
    Pending,
    Firing,
    #[serde(other)]
    Inactive,
}

#[derive(Clone)]
pub struct Prometheus {
    http: reqwest::Client,
    base: String,
}

impl Prometheus {
    /// the Prometheus at `base`, each request given up after `timeout`
    pub fn new(base: String, timeout: Duration) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .expect("a plain HTTP client always builds"),
            base,
        }
    }

    /// the body of `GET <base><path>?<params>`, which must be a success
    pub async fn get(&self, path: &str, params: &[(&str, String)]) -> Result<String> {
        let response = self
            .http
            .get(format!("{}{path}", self.base))
            .query(params)
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

    pub async fn query_body(&self, promql: &str) -> Result<String> {
        self.get("/api/v1/query", &[("query", promql.to_owned())])
            .await
    }

    pub async fn query(&self, promql: &str) -> Result<Vec<Sample>> {
        vector(&self.query_body(promql).await?).wrap_err_with(|| format!("asking {promql}"))
    }

    /// `promql` at every `step` seconds from `start` to `end`, epoch ms
    pub async fn range_body(
        &self,
        promql: &str,
        start: i64,
        end: i64,
        step: i64,
    ) -> Result<String> {
        let seconds = |ms: i64| format!("{:.3}", ms as f64 / 1_000.0);
        self.get(
            "/api/v1/query_range",
            &[
                ("query", promql.to_owned()),
                ("start", seconds(start)),
                ("end", seconds(end)),
                ("step", step.max(1).to_string()),
            ],
        )
        .await
    }

    pub async fn range(&self, promql: &str, start: i64, end: i64, step: i64) -> Result<Vec<Range>> {
        matrix(&self.range_body(promql, start, end, step).await?)
            .wrap_err_with(|| format!("asking {promql}"))
    }

    pub async fn alerts_body(&self) -> Result<String> {
        self.get("/api/v1/alerts", &[]).await
    }

    pub async fn alerts(&self) -> Result<Vec<RuleAlert>> {
        rule_alerts(&self.alerts_body().await?)
    }
}

#[derive(Deserialize)]
struct Answer<T> {
    status: String,
    data: Option<T>,
    error: Option<String>,
}

fn data<T: for<'de> Deserialize<'de>>(body: &str) -> Result<T> {
    let answer: Answer<T> =
        serde_json::from_str(body).wrap_err("not an answer of Prometheus' API")?;
    match (answer.status.as_str(), answer.data) {
        ("success", Some(data)) => Ok(data),
        _ => Err(eyre!(
            "Prometheus refused: {}",
            answer.error.unwrap_or(answer.status)
        )),
    }
}

/// a query's result, its kind named beside it in any order
#[derive(Deserialize)]
struct Data {
    #[serde(rename = "resultType")]
    kind: String,
    result: serde_json::Value,
}

impl Data {
    fn of<T: for<'de> Deserialize<'de>>(self, kind: &str) -> Result<T> {
        if self.kind != kind {
            bail!("expected a {kind}, not a {}", self.kind);
        }
        serde_json::from_value(self.result).wrap_err_with(|| format!("not a {kind}"))
    }
}

#[derive(Deserialize)]
struct RawSample {
    metric: Labels,
    value: (f64, String),
}

#[derive(Deserialize)]
struct RawRange {
    metric: Labels,
    values: Vec<(f64, String)>,
}

/// a number as Prometheus writes one, unless it is NaN or infinite
fn number(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn ms(seconds: f64) -> i64 {
    (seconds * 1_000.0).round() as i64
}

/// an instant query's answer
pub fn vector(body: &str) -> Result<Vec<Sample>> {
    let samples: Vec<RawSample> = data::<Data>(body)?.of("vector")?;
    Ok(samples
        .into_iter()
        .filter_map(|raw| {
            number(&raw.value.1).map(|value| Sample {
                labels: raw.metric,
                value,
            })
        })
        .collect())
}

/// a range query's answer
pub fn matrix(body: &str) -> Result<Vec<Range>> {
    let series: Vec<RawRange> = data::<Data>(body)?.of("matrix")?;
    Ok(series
        .into_iter()
        .map(|raw| Range {
            labels: raw.metric,
            points: raw
                .values
                .into_iter()
                .map(|(t, value)| (ms(t), number(&value)))
                .collect(),
        })
        .collect())
}

/// `/api/v1/alerts`' answer
pub fn rule_alerts(body: &str) -> Result<Vec<RuleAlert>> {
    #[derive(Deserialize)]
    struct Alerts {
        alerts: Vec<RuleAlert>,
    }
    Ok(data::<Alerts>(body)?.alerts)
}
