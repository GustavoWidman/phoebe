mod web;

use std::{net::SocketAddr, path::PathBuf};

use clap::{Args, Parser, Subcommand};
use eyre::{Result, WrapErr, ensure};
use log::info;
use tokio::{
    net::TcpListener,
    signal::unix::{SignalKind, signal},
};

use crate::web::{
    fixture::Fixture,
    live::{Live, hook, hook::PushSettings, registry::Registry},
    site,
    source::Source,
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
    Web(WebArgs),
}

#[derive(Args, Debug)]
struct WebArgs {
    /// Serve a simulated fleet, open to anyone on the listening address;
    /// POST /fixture/seek?t=<seconds>[&speed=<x>] moves its clock, and
    /// without t holds it or sets its speed where it stands
    #[arg(long)]
    fixture: bool,

    /// Where the app is served
    #[arg(long, value_name = "ADDRESS", default_value = "127.0.0.1:7980")]
    listen: SocketAddr,

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

    /// Where Alertmanager posts its notices, POST /hook; a loopback
    /// address, apart from the app
    #[arg(
        long,
        value_name = "ADDRESS",
        default_value = "127.0.0.1:7981",
        conflicts_with = "fixture"
    )]
    hook: SocketAddr,

    /// The app's https origin, where a notification leads; pushes go out
    /// only with it and both push files
    #[arg(long, value_name = "URL", requires_all = ["push_key_file", "push_subscriptions_file"], conflicts_with = "fixture")]
    origin: Option<String>,

    /// The VAPID private key pushes are signed with, read at every send
    #[arg(long, value_name = "FILE", requires = "origin")]
    push_key_file: Option<PathBuf>,

    /// The devices pushes go to, a JSON array of PushSubscription.toJSON()
    /// objects each with a name, read at every send
    #[arg(long, value_name = "FILE", requires = "origin")]
    push_subscriptions_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match Cli::parse().command {
        Command::Web(args) => web(args).await,
    }
}

/// serves the fleet until SIGTERM or SIGINT
async fn web(args: WebArgs) -> Result<()> {
    let (router, hook) = if args.fixture {
        let fixture = Fixture::new(args.speed);
        fixture.run();
        let router = site::router(Source::Fixture(fixture.clone()), args.web_dir.as_deref())
            .merge(fixture.routes());
        (router, None)
    } else {
        ensure!(
            args.hook.ip().is_loopback(),
            "the hook takes notices from this machine's Alertmanager alone, so it listens on loopback, not {}",
            args.hook
        );
        let push = match (
            args.origin,
            args.push_key_file,
            args.push_subscriptions_file,
        ) {
            (Some(origin), Some(key), Some(subscriptions)) => {
                Some(PushSettings::new(origin, key, subscriptions)?)
            }
            _ => None,
        };
        let live = Live::new(Registry::load(&args.fleet)?);
        live.run();
        let router = site::router(Source::Live(live), args.web_dir.as_deref());
        (router, Some((args.hook, hook::router(push))))
    };

    let listener = bind(args.listen).await?;
    let what = if args.fixture {
        "the simulated fleet"
    } else {
        "the fleet"
    };
    info!("serving {what} on http://{}", args.listen);
    let hook = async {
        match hook {
            Some((address, router)) => {
                let listener = bind(address).await?;
                info!("taking alertmanager's notices on http://{address}/hook");
                axum::serve(listener, router)
                    .await
                    .wrap_err("the hook stopped")
            }
            None => std::future::pending().await,
        }
    };

    // streams never end on their own, so a signal stops serving rather than
    // waiting for them
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    tokio::select! {
        served = axum::serve(listener, router).into_future() => served.wrap_err("the site stopped"),
        hooked = hook => hooked,
        _ = terminate.recv() => Ok(()),
        _ = interrupt.recv() => Ok(()),
    }
}

async fn bind(address: SocketAddr) -> Result<TcpListener> {
    TcpListener::bind(address)
        .await
        .wrap_err_with(|| format!("failed to listen on {address}"))
}
