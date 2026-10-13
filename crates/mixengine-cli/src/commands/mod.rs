//! One module per `mix` command: its clap definitions, its handler, and what only it calls.
//! [`crate::run`] dispatches to each.

pub(crate) mod daemon;
pub(crate) mod domain;
pub(crate) mod path;
pub(crate) mod status;
