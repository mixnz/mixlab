//! One module per `mix` command: its clap definitions, its handler, and what only it calls.
//! [`crate::run`] dispatches to each.

pub(crate) mod autostart;
pub(crate) mod blueprint;
pub(crate) mod cert;
pub(crate) mod daemon;
pub(crate) mod database;
pub(crate) mod disk;
pub(crate) mod doctor;
pub(crate) mod domain;
pub(crate) mod elevation;
pub(crate) mod extension;
pub(crate) mod home;
pub(crate) mod job;
pub(crate) mod logs;
pub(crate) mod metrics;
pub(crate) mod package;
pub(crate) mod path;
pub(crate) mod project;
pub(crate) mod runtime;
pub(crate) mod self_update;
pub(crate) mod service;
pub(crate) mod site;
pub(crate) mod status;
pub(crate) mod uninstall;
