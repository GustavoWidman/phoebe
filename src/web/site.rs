//! the site cloudflared reaches: the built web app, and the fleet under
//! `/api` for a browser that proves its session on every request. signing
//! in is not a route of this site ([super::signin])

use std::{convert::Infallible, path::PathBuf, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::{FromRequestParts, OriginalUri, Path as Segments, Query, Request, State},
    http::{HeaderValue, StatusCode, header, request::Parts},
    middleware::{self, Next},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use futures_util::{Stream, StreamExt, stream::BoxStream};
use log::warn;
use serde::Serialize;
use tower_http::services::{ServeDir, ServeFile};

use super::{
    push::Push,
    query::{AlertsQuery, LogPageQuery, LogQuery, SeriesQuery},
    session::Sessions,
    source::Source,
    wire::{AlertId, Hello, LogEvent, Ms, SCHEMA, StreamEvent},
};

/// who may read the fleet
#[derive(Clone)]
pub enum Access {
    /// a browser signed in on the tailnet, at one of `signin`, the sign-in
    /// addresses `/api/hello` offers
    Sessions {
        sessions: Arc<Sessions>,
        signin: Vec<String>,
    },
    /// everyone: a simulated fleet has nothing to hide
    Open,
}

/// how the site runs
pub struct Settings {
    /// the built web app; without one only `/api` answers
    pub web_dir: Option<PathBuf>,
    pub access: Access,
    /// what `/api/push` answers; without it, 404, and the app offers no
    /// notifications
    pub push: Option<Push>,
}

struct App {
    source: Source,
    build: String,
    access: Access,
    push: Option<Push>,
}

type Shared = Arc<App>;

/// every route over `source`
pub fn router(source: Source, settings: Settings) -> Router {
    let app = Arc::new(App {
        source,
        build: settings.web_dir.as_deref().map_or_else(String::new, build),
        access: settings.access,
        push: settings.push,
    });
    let api = Router::new()
        .route("/hello", get(hello))
        .route("/stream", get(stream))
        .route("/series", get(series))
        .route("/logs", get(logs))
        .route("/logs/tail", get(tail))
        .route("/alerts", get(alerts))
        .route("/alerts/{id}", get(alert))
        .route("/push", get(push))
        .fallback(|_: Signed| async { StatusCode::NOT_FOUND });
    let mut router = Router::new().nest("/api", api).with_state(app);
    if let Some(web_dir) = &settings.web_dir {
        router = router.merge(web(web_dir));
    }
    router.layer(middleware::from_fn(guard))
}

/// the web app build, named by its `index.html`, which names every other
/// file by its content
fn build(web_dir: &std::path::Path) -> String {
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
/// deep-link. the build writes a `.br` and a `.gz` beside every text file,
/// which go out as they are to a browser that takes them
fn web(web_dir: &std::path::Path) -> Router {
    let index = ServeFile::new(web_dir.join("index.html"))
        .precompressed_br()
        .precompressed_gzip();
    Router::new().fallback_service(
        ServeDir::new(web_dir)
            .precompressed_br()
            .precompressed_gzip()
            .fallback(index),
    )
}

/// what every response carries
async fn guard(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("frame-ancestors 'none'"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

/// where the app signs in, and what it is talking to; the one thing under
/// `/api` that answers before signing in, and it carries no fleet data
async fn hello(State(app): State<Shared>) -> Json<Hello> {
    let signin = match &app.access {
        Access::Sessions { signin, .. } => signin.clone(),
        Access::Open => Vec::new(),
    };
    Json(Hello {
        signin,
        build: app.build.clone(),
        schema: SCHEMA,
    })
}

/// a request [Access] lets in: holding one means its session checked and
/// its proof is over this very request
struct Signed {
    /// when its session ends, if it does
    expires: Option<Ms>,
}

impl FromRequestParts<Shared> for Signed {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, app: &Shared) -> Result<Self, Self::Rejection> {
        let sessions = match &app.access {
            Access::Open => return Ok(Self { expires: None }),
            Access::Sessions { sessions, .. } => sessions,
        };
        // the browser signed the path it asked for, `/api` included
        let uri = parts
            .extensions
            .get::<OriginalUri>()
            .map_or(&parts.uri, |original| &original.0);
        let path = uri.path_and_query().map_or("/", |path| path.as_str());
        let claims = sessions
            .authenticate(&parts.method, path, &parts.headers, Ms::now())
            .map_err(|_| StatusCode::UNAUTHORIZED)?;
        Ok(Self {
            expires: Some(claims.exp),
        })
    }
}

/// the whole fleet on connect, then again whenever it changes, until the
/// session ends
async fn stream(signed: Signed, State(app): State<Shared>) -> Response {
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
    events(expiring(
        snapshots.boxed(),
        signed.expires,
        message(&StreamEvent::Expired),
    ))
}

async fn series(
    _: Signed,
    State(app): State<Shared>,
    Query(query): Query<SeriesQuery>,
) -> Response {
    app.source.series(&query).await.map(Json).into_response()
}

async fn logs(_: Signed, State(app): State<Shared>, Query(query): Query<LogPageQuery>) -> Response {
    app.source.logs(&query).await.map(Json).into_response()
}

async fn tail(
    signed: Signed,
    State(app): State<Shared>,
    Query(query): Query<LogQuery>,
) -> Response {
    match app.source.tail(&query).await {
        Ok(lines) => events(expiring(
            lines.map(|event| message(&event)).boxed(),
            signed.expires,
            message(&LogEvent::Expired),
        )),
        Err(refusal) => refusal.into_response(),
    }
}

async fn alerts(
    _: Signed,
    State(app): State<Shared>,
    Query(query): Query<AlertsQuery>,
) -> Response {
    app.source.alerts(query).await.map(Json).into_response()
}

async fn alert(_: Signed, State(app): State<Shared>, Segments(id): Segments<String>) -> Response {
    app.source
        .alert(&AlertId(id))
        .await
        .map(Json)
        .into_response()
}

async fn push(_: Signed, State(app): State<Shared>) -> Response {
    let Some(push) = &app.push else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match push.view().await {
        Ok(view) => Json(view).into_response(),
        Err(error) => {
            warn!("cannot tell the app which devices get notifications: {error:#}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "the subscription list could not be read",
            )
                .into_response()
        }
    }
}

fn events(events: impl Stream<Item = Event> + Send + 'static) -> Response {
    Sse::new(events.map(Ok::<_, Infallible>))
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// `events` until `expires`, when `last` is sent and the stream ends
fn expiring(
    events: BoxStream<'static, Event>,
    expires: Option<Ms>,
    last: Event,
) -> BoxStream<'static, Event> {
    let Some(expires) = expires else {
        return events;
    };
    let left = Duration::from_millis(expires.0.saturating_sub(Ms::now().0).max(0) as u64);
    let deadline = Box::pin(tokio::time::sleep(left));
    futures_util::stream::unfold(Some((events, deadline, last)), |state| async move {
        let (mut events, mut deadline, last) = state?;
        tokio::select! {
            event = events.next() => Some((event?, Some((events, deadline, last)))),
            () = &mut deadline => Some((last, None)),
        }
    })
    .boxed()
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
