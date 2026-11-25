mod web;

use std::{
    future::Future,
    net::SocketAddr,
    os::unix::fs::{FileTypeExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::Router;
use clap::{Args, Parser, Subcommand};
use eyre::{Result, WrapErr, ensure, eyre};
use log::{info, warn};
use tokio::{
    net::{TcpListener, UnixListener},
    signal::unix::{SignalKind, signal},
    sync::watch,
};
use tokio_util::sync::CancellationToken;

use crate::web::{
    connector::{self, Timing, Tunnel},
    fixture::Fixture,
    live::{Live, hook, hook::PushSettings, registry::Registry},
    push::Push,
    ready::{self, Pace, Stores},
    session::Sessions,
    signin::{Caller, Signin},
    site::{self, Access, Settings},
    source::Source,
    tailnet::Tailnet,
    tls::{Certificate, TlsListener},
    whois::{CAPABILITY, Whois},
};

/// Observability for a Nix fleet: every machine and service, their
/// metrics, logs and alerts
#[derive(Parser, Debug)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Serve the web app and the fleet it reads: the real one, from this
    /// node's Prometheus, Loki and Alertmanager, or a simulated one
    Web(Box<WebArgs>),
}

#[derive(Args, Debug)]
struct WebArgs {
    /// Serve a simulated fleet on a local port, open to anyone who reaches
    /// it unless --signin; POST /fixture/seek?t=<seconds>[&speed=<x>] moves
    /// its clock, and without t holds it or sets its speed where it stands
    #[arg(long)]
    fixture: bool,

    /// Where the simulated fleet is served
    #[arg(
        long,
        value_name = "ADDRESS",
        default_value = "127.0.0.1:7980",
        requires = "fixture"
    )]
    listen: SocketAddr,

    /// Ask the simulated fleet's readers to sign in, as a node does, and
    /// sign in whoever asks on the same address
    #[arg(long, requires = "fixture")]
    signin: bool,

    /// How fast the simulated fleet's clock runs; 0 holds it
    #[arg(long, default_value_t = 1.0, requires = "fixture")]
    speed: f64,

    /// The built web app, served for every path outside /api
    #[arg(long, value_name = "DIR", env = "PHOEBE_WEB_DIR")]
    web_dir: Option<PathBuf>,

    /// The fleet's registry, as every machine writes it; it names this
    /// machine, which must be a node
    #[arg(
        long,
        value_name = "FILE",
        default_value = "/etc/phoebe/fleet.json",
        conflicts_with = "fixture"
    )]
    fleet: PathBuf,

    /// The unix socket the app is served on, for cloudflared; only its
    /// owner may open it
    #[arg(long, value_name = "FILE", required_unless_present = "fixture")]
    socket: Option<PathBuf>,

    /// Where Alertmanager posts its notices, POST /hook; a loopback
    /// address, apart from the app
    #[arg(
        long,
        value_name = "ADDRESS",
        default_value = "127.0.0.1:7981",
        conflicts_with = "fixture"
    )]
    hook: SocketAddr,

    /// The app's https origin: the one sign-in lets in, where a
    /// notification leads, and the VAPID subject
    #[arg(long, value_name = "URL", required_unless_present = "fixture")]
    origin: Option<String>,

    /// The fleet's session keys, 64 hex digits a line, the first signing
    #[arg(long, value_name = "FILE", required_unless_present = "fixture")]
    session_key_file: Option<PathBuf>,

    /// A node's sign-in address, https://<name>.<tailnet>.ts.net:<port>,
    /// which the app tries in turn; repeat it for every node
    #[arg(long = "signin-url", value_name = "URL", conflicts_with = "fixture")]
    signin_urls: Vec<String>,

    /// The port signing in is served on, on this machine's tailnet
    /// addresses alone
    #[arg(long, default_value_t = 45731, conflicts_with = "fixture")]
    signin_port: u16,

    /// This machine's *.ts.net certificate, with its chain, as PEM, read
    /// again whenever it changes
    #[arg(long, value_name = "FILE", required_unless_present = "fixture")]
    signin_cert: Option<PathBuf>,

    /// The private key of --signin-cert, as PEM
    #[arg(long, value_name = "FILE", required_unless_present = "fixture")]
    signin_key: Option<PathBuf>,

    /// The app capability a tailnet policy grant gives the nodes that may
    /// sign in, toward this one
    #[arg(long, default_value = CAPABILITY, conflicts_with = "fixture")]
    capability: String,

    /// The cloudflare tunnel this node connects to while it can serve
    #[arg(
        long,
        value_name = "ID",
        requires = "tunnel_credentials_file",
        conflicts_with = "fixture"
    )]
    tunnel_id: Option<String>,

    /// The credentials JSON `cloudflared tunnel create` wrote for the tunnel
    #[arg(long, value_name = "FILE", requires = "tunnel_id")]
    tunnel_credentials_file: Option<PathBuf>,

    /// The cloudflared to connect with
    #[arg(long, value_name = "PROGRAM", default_value = "cloudflared")]
    cloudflared: PathBuf,

    /// The VAPID private key pushes are signed with, read at every send
    #[arg(
        long,
        value_name = "FILE",
        requires = "push_subscriptions_file",
        conflicts_with = "fixture"
    )]
    push_key_file: Option<PathBuf>,

    /// The devices pushes go to, a JSON array of PushSubscription.toJSON()
    /// objects each with a name, read at every send
    #[arg(long, value_name = "FILE", requires = "push_key_file")]
    push_subscriptions_file: Option<PathBuf>,

    /// The command that edits the subscription list, which the app shows
    /// beside a subscription to add
    #[arg(long, value_name = "COMMAND", requires = "push_key_file")]
    push_edit: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match Cli::parse().command {
        Command::Web(args) if args.fixture => simulate(*args).await,
        Command::Web(args) => serve(*args).await,
    }
}

/// serves a simulated fleet on a local port until SIGTERM or SIGINT
async fn simulate(args: WebArgs) -> Result<()> {
    let fixture = Fixture::new(args.speed);
    fixture.run();
    let origin = format!("http://{}", args.listen);
    let router = fixture.site(args.web_dir, &origin, args.signin)?;
    let listener = TcpListener::bind(args.listen)
        .await
        .wrap_err_with(|| format!("failed to listen on {}", args.listen))?;
    info!("serving the simulated fleet on {origin}");
    let served = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<Caller>(),
    );
    // streams never end on their own, so a signal stops serving rather than
    // waiting for them
    until(
        async { served.await.wrap_err("the site stopped") },
        terminated()?,
    )
    .await
}

/// serves the fleet from this node's stores on the origin socket, signing
/// in on this node's tailnet addresses and taking alertmanager's notices on
/// loopback, until SIGTERM or SIGINT
async fn serve(args: WebArgs) -> Result<()> {
    let missing = |flag: &str| eyre!("--{flag} is needed to serve the real fleet");
    let origin = args.origin.ok_or_else(|| missing("origin"))?;
    let socket = args.socket.ok_or_else(|| missing("socket"))?;
    let cert = args.signin_cert.ok_or_else(|| missing("signin-cert"))?;
    let key = args.signin_key.ok_or_else(|| missing("signin-key"))?;
    let session_keys = args
        .session_key_file
        .ok_or_else(|| missing("session-key-file"))?;
    ensure!(
        args.hook.ip().is_loopback(),
        "the hook takes notices from this machine's Alertmanager alone, so it listens on loopback, not {}",
        args.hook
    );
    if args.signin_urls.is_empty() {
        warn!("no --signin-url, so the app has nowhere to sign in");
    }

    let sessions = Arc::new(Sessions::load(&session_keys)?);
    let push = match (args.push_key_file, args.push_subscriptions_file) {
        (Some(key), Some(subscriptions)) => {
            Some(PushSettings::new(origin.clone(), key, subscriptions)?)
        }
        _ => None,
    };
    let registry = Registry::load(&args.fleet)?;
    let stores = Stores::of(&registry);
    let live = Live::new(registry);
    live.run().await;
    let site = site::router(
        Source::Live(live),
        Settings {
            web_dir: args.web_dir,
            access: Access::Sessions {
                sessions: sessions.clone(),
                signin: args.signin_urls,
            },
            push: push.clone().map(|settings| Push::Live {
                settings,
                edit: args.push_edit,
            }),
        },
    );
    let signin = Signin {
        sessions,
        whois: Whois::tailscale(),
        capability: args.capability,
        origin,
    }
    .router()?;

    let terminated = terminated()?;
    let listener = bind(&socket)?;
    info!("serving the fleet on {}", socket.display());
    let hook = bind_tcp(args.hook).await?;
    info!("taking alertmanager's notices on http://{}/hook", args.hook);

    let stop = CancellationToken::new();
    let tunnel = match (args.tunnel_id, args.tunnel_credentials_file) {
        (Some(id), Some(credentials_file)) => {
            let tunnel = Tunnel {
                id,
                credentials_file,
                cloudflared: args.cloudflared,
            };
            let config = connector::configure(&tunnel, &socket)?;
            let (sender, ready) = watch::channel(false);
            tokio::spawn(async move {
                ready::watch(Pace::DEFAULT, || ready::probe(&stores), sender).await;
            });
            let stop = stop.clone();
            Some(tokio::spawn(async move {
                connector::run(&tunnel.cloudflared, &config, Timing::DEFAULT, ready, stop).await;
            }))
        }
        _ => {
            info!("no tunnel, so nothing reaches the app");
            None
        }
    };

    // cloudflared finishes the requests it carries, which need the origin,
    // before the origin closes
    let done = async {
        terminated.await;
        stop.cancel();
        if let Some(tunnel) = tunnel {
            let _ = tunnel.await;
        }
    };
    let serving = async {
        tokio::select! {
            served = axum::serve(listener, site).into_future() => served.wrap_err("the site stopped"),
            served = sign_in(signin, args.signin_port, &cert, &key) => served,
            served = axum::serve(hook, hook::router(push)).into_future() => served.wrap_err("the hook stopped"),
        }
    };
    let served = until(serving, done).await;
    let _ = std::fs::remove_file(&socket);
    served
}

/// serves `router` on each of this machine's tailnet addresses, once
/// tailscale has them and the certificate exists, trying again until then
async fn sign_in(router: Router, port: u16, cert: &Path, key: &Path) -> Result<()> {
    let listeners = loop {
        match listen_on_tailnet(port, cert, key).await {
            Ok(listeners) => break listeners,
            Err(error) => warn!("not signing in yet: {error:#}"),
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    };
    let served = listeners.into_iter().map(|listener| {
        let router = router.clone();
        async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<Caller>(),
            )
            .await
        }
    });
    futures_util::future::try_join_all(served)
        .await
        .wrap_err("signing in stopped")?;
    Ok(())
}

/// TLS listeners on every tailnet address of this machine that binds
async fn listen_on_tailnet(port: u16, cert: &Path, key: &Path) -> Result<Vec<TlsListener>> {
    let tailnet = Tailnet::read()
        .await
        .ok_or_else(|| eyre!("tailscale did not answer"))?;
    let certificate = Certificate::load(cert, key)?;
    let mut listeners = Vec::new();
    for address in tailnet.addresses() {
        let address = SocketAddr::new(address, port);
        match TcpListener::bind(address).await {
            Ok(tcp) => {
                listeners.push(TlsListener::new(tcp, certificate.acceptor())?);
                info!("signing in on https://{address}");
            }
            Err(error) => warn!("failed to listen on {address}: {error}"),
        }
    }
    match listeners.is_empty() {
        true => Err(eyre!("no tailnet address to listen on")),
        false => Ok(listeners),
    }
}

/// binds `socket` for its owner alone, replacing a socket a previous run left
fn bind(socket: &Path) -> Result<UnixListener> {
    if std::fs::symlink_metadata(socket).is_ok_and(|meta| meta.file_type().is_socket()) {
        std::fs::remove_file(socket)
            .wrap_err_with(|| format!("failed to remove {}", socket.display()))?;
    }
    let listener = UnixListener::bind(socket)
        .wrap_err_with(|| format!("failed to listen on {}", socket.display()))?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))
        .wrap_err_with(|| format!("failed to restrict {}", socket.display()))?;
    Ok(listener)
}

async fn bind_tcp(address: SocketAddr) -> Result<TcpListener> {
    TcpListener::bind(address)
        .await
        .wrap_err_with(|| format!("failed to listen on {address}"))
}

/// resolves on SIGTERM or SIGINT, whose handlers are installed at once
fn terminated() -> Result<impl Future<Output = ()>> {
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    Ok(async move {
        tokio::select! {
            _ = terminate.recv() => {},
            _ = interrupt.recv() => {},
        }
    })
}

/// runs `serving` until it stops, or until `done` resolves
async fn until(
    serving: impl Future<Output = Result<()>>,
    done: impl Future<Output = ()>,
) -> Result<()> {
    tokio::select! {
        served = serving => served,
        () = done => Ok(()),
    }
}
