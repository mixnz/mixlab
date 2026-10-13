//! `mix daemon`: stopping it, and the credential store it keeps passwords in.

use std::process::ExitCode;

use clap::Subcommand;
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    CredentialStore, CredentialStoreChange, CredentialStoreSet, DaemonShutdown, Error, rpc,
};

use crate::autostart::Autostart;
use crate::client::{Client, ask, encode};
use crate::output::{emit, rendered};
use crate::render;

/// `mix daemon …` — the daemon as a thing in itself, rather than as what answers about services.
///
/// `status` is deliberately not here and stays `mix status`: it is the first command anybody types,
/// and moving it would be renaming the one command that already exists to make room for a namespace.
#[derive(Debug, Subcommand)]
pub(crate) enum DaemonCommand {
    /// Stop the services this home is running, then stop the daemon.
    Stop,

    /// Choose where this home keeps its passwords from the next start: `os`, the system's
    /// credential store, or `home`, a file only your account can read.
    //
    // Roadmap task T194, ADR 0059. A release takes `home` only on a Linux machine with no
    // credential store, and the daemon refuses a switch while the current store holds anything.
    CredentialStore {
        /// `os` or `home`.
        store: CredentialStoreArg,
    },
}

/// The two stores `mix daemon credential-store` names.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum CredentialStoreArg {
    /// The system's credential store.
    Os,
    /// A file in this home, readable by your account only.
    Home,
}

impl From<CredentialStoreArg> for CredentialStore {
    fn from(store: CredentialStoreArg) -> Self {
        match store {
            CredentialStoreArg::Os => Self::Os,
            CredentialStoreArg::Home => Self::Home,
        }
    }
}

/// `mix daemon stop`: stop the services, then the daemon.
///
/// **The answer arrives before the daemon goes**, which is what makes an exit code possible here at
/// all: the walk it carries says whether everything this home was running actually stopped, and a
/// service that refused is worth a non-zero status even though the daemon stopped regardless. What
/// happens to the connection a moment later is not this command's business — the response has been
/// read by then.
///
/// **A stop that could not be ordered is the same kind of non-zero**, and for the same reason rather
/// than by analogy: the exit code here has never meant "the daemon stopped" — it means "what was
/// asked for happened" — and what `mix daemon stop` asks for is every service stopped in dependency
/// order. A daemon that could not work one out stopped them all at the same moment instead, which is
/// the arrangement the ordering exists to prevent, and exiting `0` would carry a
/// `mix daemon stop && …` past it in silence. Both halves are still on stdout in both renderings;
/// only the status changes.
pub(crate) async fn daemon_stop(endpoint: &Endpoint, json: bool) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, None).await?;
    let shutdown: DaemonShutdown = ask(&mut client, rpc::method::DAEMON_SHUTDOWN, None).await?;

    emit(&rendered(json, &shutdown, || {
        render::daemon_shutdown(&shutdown)
    }))?;

    Ok(match (&shutdown.services.failed, &shutdown.unordered) {
        (None, None) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    })
}

/// `mix daemon credential-store` — roadmap task T194.
pub(crate) async fn daemon_credential_store(
    store: CredentialStoreArg,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;
    let change: CredentialStoreChange = ask(
        &mut client,
        rpc::method::DAEMON_SET_CREDENTIAL_STORE,
        encode(&CredentialStoreSet {
            store: store.into(),
        }),
    )
    .await?;

    emit(&rendered(json, &change, || {
        render::credential_store_change(change)
    }))?;

    Ok(ExitCode::SUCCESS)
}
