//! where the site's fleet comes from: every route asks the source, so the
//! real fleet and a simulated one answer through the same site

use std::sync::Arc;

use futures_util::{StreamExt, stream::BoxStream};
use tokio::sync::watch;

use super::{
    fixture::Fixture,
    query::{AlertsQuery, LogPageQuery, LogQuery, Refusal, SeriesQuery},
    wire::{Alert, AlertDetail, AlertId, LogEvent, LogPage, Series, Snapshot},
};

#[derive(Clone)]
pub enum Source {
    Fixture(Arc<Fixture>),
}

impl Source {
    /// the fleet as it is now, changed whenever it changes
    pub fn fleet(&self) -> watch::Receiver<Arc<Snapshot>> {
        match self {
            Self::Fixture(fixture) => fixture.fleet(),
        }
    }

    pub async fn series(&self, query: &SeriesQuery) -> Result<Series, Refusal> {
        match self {
            Self::Fixture(fixture) => fixture.series(query),
        }
    }

    pub async fn logs(&self, query: &LogPageQuery) -> Result<LogPage, Refusal> {
        match self {
            Self::Fixture(fixture) => fixture.logs(query),
        }
    }

    pub async fn tail(&self, query: &LogQuery) -> Result<BoxStream<'static, LogEvent>, Refusal> {
        match self {
            Self::Fixture(fixture) => Ok(fixture.tail(query)?.boxed()),
        }
    }

    pub async fn alerts(&self, query: AlertsQuery) -> Result<Vec<Alert>, Refusal> {
        match self {
            Self::Fixture(fixture) => Ok(fixture.alerts(query.range.ms())),
        }
    }

    pub async fn alert(&self, id: &AlertId) -> Result<AlertDetail, Refusal> {
        match self {
            Self::Fixture(fixture) => fixture.alert(id),
        }
    }
}
