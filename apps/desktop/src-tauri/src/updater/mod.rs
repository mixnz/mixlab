//! MixLab's own updater — roadmap task **T187**,
//! `docs/specs/2026-09-26-t187-mixlab-updates-itself-design.md`.
//!
//! **Nothing here names a MixEngine crate** (ADR 0056 rule 3; `tests/layering.rs` scans for it).
//! The one thing this module needs from MixEngine, stopping and starting a daemon that is already
//! running, it asks `crate::modules::mixengine::for_update` for.

pub mod commands;
pub mod feed;
pub mod handover;
pub mod install;
pub mod lock;
pub mod placement;
pub mod ready;
pub mod records;
pub mod stage;
pub mod swap;

/// Where the feed is published: the stable release-asset redirect, not the rate-limited API.
pub const FEED_URL: &str = "https://github.com/mixnz/mixlab/releases/latest/download/latest.json";

/// The key every release is signed with. Held equal to `packaging/updates.pub` by a test.
pub const PUBLIC_KEY: &str = "RWSELKuybM79fmLhYywhXv8mdDvB3LPCC3TyHTdm2svTti6clLkck051";

/// This machine, spelled the way the feed spells it: `std::env::consts` already matches it.
pub fn host() -> (&'static str, &'static str) {
    (std::env::consts::OS, std::env::consts::ARCH)
}
