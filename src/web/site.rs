//! the site: the fleet under `/api`, and the built web app beside it

use std::{convert::Infallible, path::Path, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path as Segments, Query, State},
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use futures_util::{Stream, StreamExt};
use serde::Serialize;
use tower_http::services::{ServeDir, ServeFile};

use super::{
    query::{AlertsQuery, LogPageQuery, LogQuery, SeriesQuery},
    source::Source,
    wire::{AlertId, Hello, LogEvent, SCHEMA, StreamEvent},
};

struct App {
    source: Source,
    build: String,
}

type Shared = Arc<App>;

/// every route over `source`, and the web app built into `web_dir` if given
pub fn router(source: Source, web_dir: Option<&Path>) -> Router {
    let app = Arc::new(App {
        source,
        build: web_dir.map_or_else(String::new, build),
    });
    let api = Router::new()
        .route("/hello", get(hello))
        .route("/stream", get(stream))
        .route("/series", get(series))
        .route("/logs", get(logs))
        .route("/logs/tail", get(tail))
        .route("/alerts", get(alerts))
        .route("/alerts/{id}", get(alert))
        .fallback(|| async { StatusCode::NOT_FOUND });
    let router = Router::new().nest("/api", api).with_state(app);
    match web_dir {
        Some(web_dir) => router.merge(web(web_dir)),
        None => router,
    }
}

/// the web app build, named by its `index.html`, which names every other
/// file by its content
fn build(web_dir: &Path) -> String {
    std::fs::read(web_dir.join("index.html")).map_or_else(
        |_| String::new(),
        |index| {
            // FNV-1a
            let hash = index.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ *byte as u64).wrapping_mul(0x0100_0000_01b3)
            });
            format!("{hash:012x}")[..12].to_owned()
        },
    )
}

/// the built app; unknown paths get `index.html`, so the app's own routes
/// deep-link
fn web(web_dir: &Path) -> Router {
    let index = ServeFile::new(web_dir.join("index.html"));
    Router::new().fallback_service(ServeDir::new(web_dir).fallback(index))
}

async fn hello(State(app): State<Shared>) -> Json<Hello> {
    Json(Hello {
        signin: Vec::new(),
        build: app.build.clone(),
        schema: SCHEMA,
    })
}

/// the whole fleet on connect, then again whenever it changes
async fn stream(State(app): State<Shared>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let snapshots = futures_util::stream::unfold(
        (app.source.fleet(), true),
        |(mut fleet, first)| async move {
            if !first && fleet.changed().await.is_err() {
                return None;
            }
            let snapshot = fleet.borrow_and_update().clone();
            let event = message(&StreamEvent::Snapshot((*snapshot).clone()));
            Some((event, (fleet, false)))
        },
    );
    Sse::new(snapshots.map(Ok)).keep_alive(KeepAlive::default())
}

async fn series(State(app): State<Shared>, Query(query): Query<SeriesQuery>) -> Response {
    app.source.series(&query).await.map(Json).into_response()
}

async fn logs(State(app): State<Shared>, Query(query): Query<LogPageQuery>) -> Response {
    app.source.logs(&query).await.map(Json).into_response()
}

async fn tail(State(app): State<Shared>, Query(query): Query<LogQuery>) -> Response {
    match app.source.tail(&query).await {
        Ok(lines) => Sse::new(lines.map(|event| Ok::<_, Infallible>(message(&event))))
            .keep_alive(KeepAlive::default())
            .into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

async fn alerts(State(app): State<Shared>, Query(query): Query<AlertsQuery>) -> Response {
    app.source.alerts(query).await.map(Json).into_response()
}

async fn alert(State(app): State<Shared>, Segments(id): Segments<String>) -> Response {
    app.source
        .alert(&AlertId(id))
        .await
        .map(Json)
        .into_response()
}

/// one server-sent event, named by the message's `kind`
fn message(message: &(impl Serialize + Kind)) -> Event {
    let data = serde_json::to_string(message).expect("wire types always serialize");
    Event::default().event(message.kind()).data(data)
}

/// the name a message goes by, the same as its `kind` field
trait Kind {
    fn kind(&self) -> &'static str;
}

impl Kind for StreamEvent {
    fn kind(&self) -> &'static str {
        match self {
            Self::Snapshot(_) => "snapshot",
            Self::Expired => "expired",
        }
    }
}

impl Kind for LogEvent {
    fn kind(&self) -> &'static str {
        match self {
            Self::Lines { .. } => "lines",
            Self::Expired => "expired",
        }
    }
}
