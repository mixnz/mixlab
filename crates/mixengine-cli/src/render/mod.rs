//! What `mix` puts on screen.
//!
//! The two renderings are deliberately not the same information twice at different widths. `--json`
//! is a contract: whatever the daemon answered, serialised, with the client's own identity beside it
//! so a captured file says which `mix` produced it (`docs/features/client-surface.md` calls this "copy
//! diagnostics"). The human one is a person's answer to "is it up, and which one am I talking to",
//! and leaves out anything they would not read.
//!
//! No colour, and no dependency for one. Nearly every line `mix` prints ends up pasted into a bug
//! report or an issue, and escape codes there are noise — the daemon makes the same call about its
//! own log file, which is coloured on stderr and never in `daemon.log`.

pub(crate) mod autostart;
pub(crate) mod blueprint;
pub(crate) mod catalogue;
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
pub(crate) mod metrics;
pub(crate) mod package;
pub(crate) mod path;
pub(crate) mod prerequisites;
pub(crate) mod project;
pub(crate) mod runtime;
pub(crate) mod self_update;
pub(crate) mod service;
pub(crate) mod site;
mod size;
pub(crate) mod status;
mod table;
mod time;
pub(crate) mod uninstall;
pub(crate) mod upgrades;
