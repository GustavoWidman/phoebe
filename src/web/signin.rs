//! signing in, a site of its own served only on this node's tailnet
//! addresses, never through cloudflared, so nothing from the internet
//! reaches it whatever it carries. the web app, loaded from the public
//! origin, calls it cross-origin: `/auth/ping` tells it it is on the
//! tailnet, and `/auth/session` turns the caller's tailnet identity and the
//! public key of a key pair it cannot export into a session.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Json, Router,
    extract::{ConnectInfo, State, connect_info::Connected},
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
    serve::IncomingStream,
};
use eyre::{Result, WrapErr};
use log::{info, warn};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;

use super::{
    session::{DeviceKey, Proof, Sessions, fresh, signature},
    tls::TlsListener,
    whois::Whois,
    wire::{Ms, SignIn, SignedIn, Viewer},
};

/// the address a sign-in came from, which WireGuard vouches for
#[derive(Clone, Copy, Debug)]
pub struct Caller(pub SocketAddr);

impl Connected<IncomingStream<'_, TlsListener>> for Caller {
    fn connect_info(stream: IncomingStream<'_, TlsListener>) -> Self {
        Self(*stream.remote_addr())
    }
}

/// a simulated fleet signs in over plain TCP
impl Connected<IncomingStream<'_, TcpListener>> for Caller {
    fn connect_info(stream: IncomingStream<'_, TcpListener>) -> Self {
        Self(*stream.remote_addr())
    }
}

pub struct Signin {
    pub sessions: Arc<Sessions>,
    pub whois: Whois,
    /// the app capability a node must be granted toward this one
    pub capability: String,
    /// the origin the web app is served from, the only one let in
    pub origin: String,
}

impl Signin {
    /// the sign-in routes, open to the public origin alone
    pub fn router(self) -> Result<Router> {
        let cors = CorsLayer::new()
            .allow_origin(
                HeaderValue::from_str(&self.origin)
                    .wrap_err_with(|| format!("{} is not an origin", self.origin))?,
            )
            .allow_methods([Method::GET, Method::POST])
            .allow_headers([header::CONTENT_TYPE])
            .allow_private_network(true);

        Ok(Router::new()
            .route("/auth/ping", get(|| async { StatusCode::NO_CONTENT }))
            .route("/auth/session", post(session))
            .layer(cors)
            .with_state(Arc::new(self)))
    }
}

async fn session(
    State(signin): State<Arc<Signin>>,
    ConnectInfo(Caller(peer)): ConnectInfo<Caller>,
    Json(request): Json<SignIn>,
) -> Response {
    let refused = |status: StatusCode, why: &'static str| (status, why).into_response();
    let now = Ms::now();
    if fresh(request.ts, now).is_err() {
        return refused(
            StatusCode::BAD_REQUEST,
            "signed too far from now: is this device's clock right?",
        );
    }
    let Ok(key) = DeviceKey::from_jwk(&request.key) else {
        return refused(StatusCode::BAD_REQUEST, "not a P-256 public key");
    };
    let signed = signature(&request.sig)
        .is_ok_and(|sig| key.verifies(&Proof::signin(&signin.origin, request.ts), &sig));
    if !signed {
        return refused(StatusCode::BAD_REQUEST, "the signature is not the key's");
    }

    let address = peer.ip().to_canonical();
    let peer = match signin.whois.ask(address).await {
        Ok(Some(peer)) => peer,
        Ok(None) => return refused(StatusCode::FORBIDDEN, "not a tailnet peer"),
        Err(error) => {
            warn!("could not tell who {address} is: {error:#}");
            return refused(StatusCode::SERVICE_UNAVAILABLE, "tailscale did not answer");
        }
    };
    if !peer.granted(&signin.capability) {
        info!("refused {} on {}: not granted", peer.login, peer.device);
        return refused(StatusCode::FORBIDDEN, "the tailnet policy grants no access");
    }

    let session = signin.sessions.issue(peer.login, peer.device, key, now);
    info!(
        "signed {} in on {}",
        session.claims.sub, session.claims.device
    );
    Json(SignedIn {
        token: session.token,
        viewer: Viewer {
            login: session.claims.sub,
            device: session.claims.device,
            expires_at: session.claims.exp,
        },
    })
    .into_response()
}
