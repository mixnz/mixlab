//! One module per `mix` command: its clap definitions, its handler, and what only it calls.
//! [`crate::run`] dispatches to each.

pub(crate) mod autostart;
pub(crate) mod cert;
pub(crate) mod daemon;
pub(crate) mod database;
pub(crate) mod domain;
pub(crate) mod elevation;
pub(crate) mod home;
pub(crate) mod job;
pub(crate) mod logs;
pub(crate) mod metrics;
pub(crate) mod path;
pub(crate) mod status;
