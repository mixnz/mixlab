//! The repairs a daemon start and a runtime install both make: every installed runtime's service,
//! the activation port each wakeable service needs, and the address held for each.
//!
//! Idempotent, and each one logs its own failure and returns nothing a caller could propagate:
//! that is how "nothing here fails the start, and nothing here undoes the install" is kept by the
//! types rather than by each caller remembering it. What differs between the two callers is only
//! what a person reading `daemon.log` is told, which is [`Occasion`]'s whole purpose.

use std::sync::Arc;

use mixengine_core::{Paths, Store};
use mixengine_proto::{PackageVersion, RuntimeKind};

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

/// Give every installed runtime the service its recipe says it should have.
#[warn(clippy::cognitive_complexity)]
pub(crate) async fn give_runtimes_their_pools(store: &Store, occasion: Occasion<'_>) {
    let created = mixengine_core::services::pools::ensure(
        store,
        mixengine_platform::host().as_ref(),
        &super::catalogue(),
    )
    .await;

    match (created, occasion) {
        (Ok(created), Occasion::Start) if created.is_empty() => {
            tracing::debug!("every installed runtime already has the service it needs");
        }
        (Ok(created), Occasion::Start) => {
            tracing::info!(pools = ?created, "installed runtimes were given services");
        }
        (Err(error), Occasion::Start) => {
            tracing::warn!(%error, "could not give every installed runtime its service");
        }

        (Ok(created), Occasion::Install { .. }) if created.is_empty() => {}
        (Ok(created), Occasion::Install { .. }) => {
            tracing::info!(pools = ?created, "the new runtime was given its service");
        }
        (Err(error), Occasion::Install { kind, version }) => tracing::warn!(
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

    match (given, occasion) {
        (Ok(given), Occasion::Start) if given.is_empty() => {
            tracing::debug!("every service that can be woken already has its port");
            true
        }
        (Ok(given), Occasion::Start) => {
            tracing::info!(services = ?given, "services were given activation ports");
            true
        }
        (Err(error), Occasion::Start) => {
            tracing::warn!(%error, "could not give every wakeable service a port");
            false
        }

        (Ok(_), Occasion::Install { .. }) => true,
        (Err(error), Occasion::Install { .. }) => {
            tracing::warn!(
                %error,
                "a new pool could not be given an activation port; the next daemon start will try \
                 again"
            );
            false
        }
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

    match (held, occasion) {
        (Ok(held), Occasion::Start) if held.is_empty() => {
            tracing::debug!("no service in this home can be started by a request");
        }
        (Ok(held), Occasion::Start) => {
            tracing::info!(services = ?held, "holding an address for each of these");
        }
        (Err(error), Occasion::Start) => {
            tracing::warn!(%error, "could not hold an address for every wakeable service");
        }

        (Ok(held), Occasion::Install { .. }) if held.is_empty() => {}
        (Ok(held), Occasion::Install { .. }) => {
            tracing::info!(services = ?held, "a request can now start these services");
        }
        (Err(error), Occasion::Install { .. }) => tracing::warn!(
            %error,
            "a new pool cannot be started by a request until this daemon is restarted"
        ),
    }
}
