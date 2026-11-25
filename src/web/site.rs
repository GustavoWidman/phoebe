//! the site cloudflared reaches: the built web app, and the fleet under
//! `/api` for a browser that proves its session on every request. signing
//! in is not a route of this site ([super::signin])

use std::{
    convert::Infallible,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

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
fn build(web_dir: &Path) -> String {
    std::fs::read(web_dir.join("index.html")).map_or_else(
        |_| String::new(),
        |index| format!("{:012x}", fnv(&index))[..12].to_owned(),
    )
}

/// FNV-1a of `bytes`
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// the built app; unknown paths get `index.html`, so the app's own routes
/// deep-link. the build writes a `.br` and a `.gz` beside every text file,
/// which go out as they are to a browser that takes them
fn web(web_dir: &Path) -> Router {
    let index = ServeFile::new(web_dir.join("index.html"))
        .precompressed_br()
        .precompressed_gzip();
    let files = ServeDir::new(web_dir)
        .precompressed_br()
        .precompressed_gzip()
        .fallback(index);
    let web_dir: Arc<Path> = web_dir.into();
    Router::new()
        .fallback_service(files)
        .layer(middleware::from_fn(move |request: Request, next: Next| {
            caching(web_dir.clone(), request, next)
        }))
}

/// hashed build files never change, so browsers and cloudflare keep them a
/// year. everything else, `index.html` and the service worker first, is
/// checked on every load against an ETag of its content: every file in the
/// nix store has the same mtime, so a validator built from mtime and size
/// calls a new file of the same size unchanged, and a cache left to its own
/// rules keeps serving an old service worker, which keeps serving an old app
async fn caching(web_dir: Arc<Path>, mut request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    if path.starts_with("/_app/immutable/") {
        let mut response = next.run(request).await;
        if response.status().is_success() {
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            );
        }
        return response;
    }

    let validator = request.headers_mut().remove(header::IF_NONE_MATCH);
    request.headers_mut().remove(header::IF_MODIFIED_SINCE);
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.remove(header::LAST_MODIFIED);
    headers.remove(header::ETAG);
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    if !response.status().is_success() {
        return response;
    }

    let encoding = response
        .headers()
        .get(header::CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let Some(etag) = etag(&web_dir, &path, encoding.as_deref()) else {
        return response;
    };
    if validator.is_some_and(|validator| validator.as_bytes() == etag.as_bytes()) {
        let mut unchanged = StatusCode::NOT_MODIFIED.into_response();
        let headers = unchanged.headers_mut();
        headers.insert(header::ETAG, etag);
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        return unchanged;
    }
    response.headers_mut().insert(header::ETAG, etag);
    response
}

/// an ETag of the bytes `path` sends in `encoding`: its file, or the `.br` or
/// `.gz` copy beside it, and `index.html` for a path only the app knows
fn etag(web_dir: &Path, path: &str, encoding: Option<&str>) -> Option<HeaderValue> {
    let relative = path.trim_start_matches('/');
    if relative.split('/').any(|part| part == ".." || part == ".") {
        return None;
    }
    let mut file = web_dir.join(relative);
    if file.is_dir() {
        file = file.join("index.html");
    }
    if !file.is_file() {
        file = web_dir.join("index.html");
    }
    let mut sent = file.into_os_string();
    match encoding {
        Some("br") => sent.push(".br"),
        Some("gzip") => sent.push(".gz"),
        _ => {}
    }
    let bytes = std::fs::read(sent).ok()?;
    HeaderValue::from_str(&format!("\"{:016x}\"", fnv(&bytes))).ok()
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
