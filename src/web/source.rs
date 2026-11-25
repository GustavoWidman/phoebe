//! where the site's fleet comes from: every route asks the source, so the
//! real fleet and a simulated one answer through the same site

use std::sync::Arc;

use futures_util::{StreamExt, stream::BoxStream};
use tokio::sync::watch;

use super::{
    fixture::Fixture,
    live::Live,
    query::{AlertsQuery, LogPageQuery, LogQuery, Refusal, SeriesQuery},
    wire::{Alert, AlertDetail, AlertId, LogEvent, LogPage, Series, Snapshot},
};

#[derive(Clone)]
pub enum Source {
    Fixture(Arc<Fixture>),
    Live(Arc<Live>),
}

impl Source {
    /// the fleet as it is now, changed whenever it changes
    pub fn fleet(&self) -> watch::Receiver<Arc<Snapshot>> {
        match self {
            Self::Fixture(fixture) => fixture.fleet(),
            Self::Live(live) => live.fleet(),
        }
    }

    pub async fn series(&self, query: &SeriesQuery) -> Result<Series, Refusal> {
        match self {
            Self::Fixture(fixture) => fixture.series(query),
            Self::Live(live) => live.series(query).await,
        }
    }

    pub async fn logs(&self, query: &LogPageQuery) -> Result<LogPage, Refusal> {
        match self {
            Self::Fixture(fixture) => fixture.logs(query),
            Self::Live(live) => live.logs(query).await,
        }
    }

    pub async fn tail(&self, query: &LogQuery) -> Result<BoxStream<'static, LogEvent>, Refusal> {
        match self {
            Self::Fixture(fixture) => Ok(fixture.tail(query)?.boxed()),
            Self::Live(live) => Ok(live.tail(query).await?.boxed()),
        }
    }

    pub async fn alerts(&self, query: AlertsQuery) -> Result<Vec<Alert>, Refusal> {
        match self {
            Self::Fixture(fixture) => Ok(fixture.alerts(query.range.ms())),
            Self::Live(live) => live.alerts(query.range.ms()).await,
        }
    }

    pub async fn alert(&self, id: &AlertId) -> Result<AlertDetail, Refusal> {
        match self {
            Self::Fixture(fixture) => fixture.alert(id),
            Self::Live(live) => live.alert(id).await,
        }
    }
}
