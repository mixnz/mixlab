//! One module per `mix` command: its clap definitions, its handler, and what only it calls.
//! [`crate::run`] dispatches to each.

pub(crate) mod status;
