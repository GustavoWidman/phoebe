//! `phoebe web`: the fleet as the web app reads it. the shapes it sends are
//! in [wire]; the site ([site]) answers every route from a [source]: the
//! real fleet's stores ([live]), or a simulated fleet ([fixture]).
//!
//! a browser reads it with a session ([session]) it signs in for on the
//! tailnet ([signin], [whois], [tls]), and turns notifications on through
//! `/api/push` ([push]). while the node can serve ([ready]) it connects to
//! the cloudflare tunnel that brings visitors to it ([connector]).

pub mod connector;
pub mod fixture;
pub mod live;
pub mod push;
pub mod query;
pub mod ready;
pub mod session;
pub mod signin;
pub mod site;
pub mod source;
pub mod tailnet;
pub mod tls;
pub mod whois;
pub mod wire;

#[cfg(test)]
mod test;
