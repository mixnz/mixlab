//! `service.set`: changing a setting of a service that already exists.

use super::ServiceSummary;
use crate::ServiceId;

/// What `service.set` takes: one service, and the settings to change on it.
///
/// **One optional field per setting**, so editing overrides after create can join this method
/// later as a field beside `port` rather than as a method of its own. Today `port` is the only one,
/// and a request that names none is refused in words.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceSet {
    /// The service to change.
    pub service: ServiceId,

    /// The port it should listen on. Checked the way `service.create` checks an allocated port:
    /// not held by any row or extension, and bindable on this machine right now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

/// A port, before and after a move. `from == to` when the service was already there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PortChange {
    /// The port the service held before.
    pub from: u16,

    /// The port it holds now.
    pub to: u16,
}

/// What `service.set` answers.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceSetResult {
    /// The service as it is after the change.
    pub service: ServiceSummary,

    /// The port change, when the request named a port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<PortChange>,

    /// Whether the service was restarted to take the change. `false` for one that was stopped,
    /// which stays stopped.
    pub restarted: bool,

    /// What the daemon cannot change itself, in sentences a client prints as they are: a project's
    /// `.env`, a saved connection in MixLab, shell history. Empty when nothing moved.
    #[serde(default)]
    pub not_followed: Vec<String>,
}
