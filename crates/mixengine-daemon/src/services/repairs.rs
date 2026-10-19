//! The repairs a daemon start and a runtime install both make: every installed runtime's service,
//! the activation port each wakeable service needs, and the address held for each.
//!
//! Idempotent, and each one logs its own failure and returns nothing a caller could propagate:
//! that is how "nothing here fails the start, and nothing here undoes the install" is kept by the
//! types rather than by each caller remembering it. What differs between the two callers is only
//! what a person reading `daemon.log` is told, which is [`Occasion`]'s whole purpose.

use std::sync::Arc;

use mixengine_core::{Paths, Store};
use mixengine_proto::{PackageVersion, RuntimeKind, ServiceId};

use super::Registry;

/// Why a repair is being made, which decides what it says about itself.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Occasion<'a> {
    /// A daemon start, repairing whatever the home holds.
    Start,

    /// Right after this runtime was recorded.
    Install {
        kind: RuntimeKind,
        version: &'a PackageVersion,
    },
}

/// What one repair did: the services it touched, or why it could not.
type Repaired = mixengine_core::Result<Vec<ServiceId>>;

/// What holding the addresses did.
type Held = Result<Vec<ServiceId>, Box<mixengine_core::Error>>;

/// Give every installed runtime the service its recipe says it should have.
#[warn(clippy::cognitive_complexity)]
pub(crate) async fn give_runtimes_their_pools(store: &Store, occasion: Occasion<'_>) {
    let created = mixengine_core::services::pools::ensure(
        store,
        mixengine_platform::host().as_ref(),
        &super::catalogue(),
    )
    .await;

    match occasion {
        Occasion::Start => pools_at_start(created),
        Occasion::Install { kind, version } => pools_after_install(created, kind, version),
    }
}

#[warn(clippy::cognitive_complexity)]
fn pools_at_start(created: Repaired) {
    match created {
        Ok(created) if created.is_empty() => {
            tracing::debug!("every installed runtime already has the service it needs");
        }
        Ok(created) => tracing::info!(pools = ?created, "installed runtimes were given services"),
        Err(error) => tracing::warn!(%error, "could not give every installed runtime its service"),
    }
}

#[warn(clippy::cognitive_complexity)]
fn pools_after_install(created: Repaired, kind: RuntimeKind, version: &PackageVersion) {
    match created {
        Ok(created) if created.is_empty() => {}
        Ok(created) => {
            tracing::info!(pools = ?created, "the new runtime was given its service");
        }
        Err(error) => tracing::warn!(
            kind = kind.as_str(),
            version = version.as_str(),
            %error,
            "this runtime was installed but could not be given its service; the next daemon \
             start will try again"
        ),
    }
}

/// Allocate a port for every service whose activator needs one. `false` if that failed.
#[warn(clippy::cognitive_complexity)]
pub(crate) async fn give_activation_ports(store: &Store, occasion: Occasion<'_>) -> bool {
    let given = mixengine_core::services::activation::ensure(
        store,
        mixengine_platform::host().as_ref(),
        &super::catalogue(),
    )
    .await;
    let worked = given.is_ok();

    match occasion {
        Occasion::Start => ports_at_start(given),
        Occasion::Install { .. } => ports_after_install(given),
    }

    worked
}

#[warn(clippy::cognitive_complexity)]
fn ports_at_start(given: Repaired) {
    match given {
        Ok(given) if given.is_empty() => {
            tracing::debug!("every service that can be woken already has its port");
        }
        Ok(given) => tracing::info!(services = ?given, "services were given activation ports"),
        Err(error) => tracing::warn!(%error, "could not give every wakeable service a port"),
    }
}

#[warn(clippy::cognitive_complexity)]
fn ports_after_install(given: Repaired) {
    if let Err(error) = given {
        tracing::warn!(
            %error,
            "a new pool could not be given an activation port; the next daemon start will try \
             again"
        );
    }
}

/// Hold an address for every service a request can start, for as long as this daemon runs.
#[warn(clippy::cognitive_complexity)]
pub(crate) async fn hold_activation_addresses(
    services: Arc<Registry>,
    paths: &Paths,
    store: &Store,
    occasion: Occasion<'_>,
) {
    let host = mixengine_platform::host();
    let held = super::activate::hold_all(services, paths, store, host.as_ref()).await;

    match occasion {
        Occasion::Start => held_at_start(held),
        Occasion::Install { .. } => held_after_install(held),
    }
}

#[warn(clippy::cognitive_complexity)]
fn held_at_start(held: Held) {
    match held {
        Ok(held) if held.is_empty() => {
            tracing::debug!("no service in this home can be started by a request");
        }
        Ok(held) => tracing::info!(services = ?held, "holding an address for each of these"),
        Err(error) => {
            tracing::warn!(%error, "could not hold an address for every wakeable service");
        }
    }
}

#[warn(clippy::cognitive_complexity)]
fn held_after_install(held: Held) {
    match held {
        Ok(held) if held.is_empty() => {}
        Ok(held) => tracing::info!(services = ?held, "a request can now start these services"),
        Err(error) => tracing::warn!(
            %error,
            "a new pool cannot be started by a request until this daemon is restarted"
        ),
    }
}
