//! what a browser asks for, parsed from its query string at the edge, and
//! why an answer can be refused

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Deserializer, de::Error};

use super::wire::{Hostname, Metric, ServiceName, Subject};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Range {
    #[serde(rename = "1h")]
    Hour,
    #[serde(rename = "6h")]
    SixHours,
    #[serde(rename = "24h")]
    Day,
    #[serde(rename = "7d")]
    Week,
}

impl Range {
    pub fn ms(self) -> i64 {
        const HOUR: i64 = 3_600_000;
        match self {
            Self::Hour => HOUR,
            Self::SixHours => 6 * HOUR,
            Self::Day => 24 * HOUR,
            Self::Week => 7 * 24 * HOUR,
        }
    }
}

/// `GET /api/series?of=machine:<name>|service:<name>&metric=&range=[&node=]`
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct SeriesQuery {
    #[serde(deserialize_with = "subject")]
    pub of: Subject,
    pub metric: Metric,
    pub range: Range,
    /// for a service's latency: from this node alone, rather than the band
    /// across all of them
    pub node: Option<Hostname>,
}

fn subject<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Subject, D::Error> {
    let of = String::deserialize(deserializer)?;
    match of.split_once(':') {
        Some(("machine", name)) => Ok(Subject::Machine(Hostname::from(name))),
        Some(("service", name)) => Ok(Subject::Service(ServiceName::from(name))),
        _ => Err(D::Error::custom(format!(
            "`of` is machine:<name> or service:<name>, not {of}"
        ))),
    }
}

/// which lines `GET /api/logs` and `GET /api/logs/tail` answer with; every
/// one given narrows them
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct LogQuery {
    pub machine: Option<Hostname>,
    /// lines its unit wrote, or that name it
    pub unit: Option<String>,
    /// lines its unit wrote on its machine; for a service with no unit,
    /// lines that name one of its probes' hosts
    pub service: Option<ServiceName>,
    /// found in a line's text or unit, ignoring case
    pub q: Option<String>,
}

/// `GET /api/logs`: a page of lines, older than `before` if given
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct LogPageQuery {
    #[serde(flatten)]
    pub lines: LogQuery,
    /// the `before` of the previous page
    pub before: Option<String>,
    /// a query flattened into another arrives as text, numbers included
    #[serde(default = "LogPageQuery::default_limit", deserialize_with = "number")]
    pub limit: usize,
}

fn number<'de, D: Deserializer<'de>>(deserializer: D) -> Result<usize, D::Error> {
    let text = String::deserialize(deserializer)?;
    text.parse()
        .map_err(|_| D::Error::custom(format!("`limit` is a number, not {text}")))
}

impl LogPageQuery {
    pub const MAX_LIMIT: usize = 1_000;

    fn default_limit() -> usize {
        100
    }
}

/// `GET /api/alerts?range=`, 24 hours unless given
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct AlertsQuery {
    #[serde(default = "AlertsQuery::default_range")]
    pub range: Range,
}

impl AlertsQuery {
    fn default_range() -> Range {
        Range::Day
    }
}

/// why a request has no answer
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// it asks for something that cannot exist, such as a machine's latency
    Invalid(String),
    /// it names something the fleet does not have
    Missing(String),
    /// a store it needs did not answer
    Unavailable(String),
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        match self {
            Self::Invalid(reason) => (StatusCode::BAD_REQUEST, reason).into_response(),
            Self::Missing(reason) => (StatusCode::NOT_FOUND, reason).into_response(),
            Self::Unavailable(reason) => (StatusCode::SERVICE_UNAVAILABLE, reason).into_response(),
        }
    }
}
