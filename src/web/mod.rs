//! `phoebe web`: the fleet as the web app reads it. the shapes it sends are
//! in [wire]; the site ([site]) answers every route from a [source], here a
//! simulated fleet ([fixture]).

pub mod fixture;
pub mod query;
pub mod site;
pub mod source;
pub mod wire;

#[cfg(test)]
mod test;
