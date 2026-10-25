//! Dial the daemon.
//!
//! `mixengine_platform::ipc::Connection` does the whole of it: on Windows it waits out a pipe
//! that is between instances and refuses one another account holds *before* a byte is written; on
//! Unix it opens the socket file under `run/`. What used to be here — a named-pipe dial, a retry
//! loop, an owner check read through `windows-sys` — was that crate's code kept in step by hand.

use mixengine_platform::ipc::{Connection, Endpoint};

use crate::error::AppError;

use super::endpoint;

/// One open connection to the daemon: `AsyncRead + AsyncWrite`, which `rpc`, `events`, `logs` and
/// `metrics` wrap in `hyper`.
pub type Io = Connection;

/// The endpoint this machine's daemon listens on, rendered the way the OS names it.
///
/// Tests only: the window itself never shows the address — every error that needs it carries it
/// in `endpoint` — and the one reader left is `rpc`'s test, which compares what it dialled.
#[cfg(test)]
pub fn current_address() -> Result<String, AppError> {
    endpoint::endpoint().map(|endpoint| endpoint.to_string())
}

/// Open a connection to the daemon of this machine's home.
pub async fn connect() -> Result<Io, AppError> {
    let endpoint = endpoint::endpoint()?;
    dial(&endpoint).await
}

/// Open a connection to a known endpoint.
///
/// Three outcomes the UI draws differently, carried on three keys: nothing listening
/// (`error.mixengineUnreachable`), something listening that belongs to another account
/// (`error.mixenginePipeOwner`), and a home that cannot be named at all
/// (`error.mixengineNoHome`, raised before this is reached).
pub async fn dial(endpoint: &Endpoint) -> Result<Io, AppError> {
    Connection::connect(endpoint)
        .await
        .map_err(|error| match error {
            mixengine_platform::Error::EndpointNotOurs { address, account } => {
                err!(
                    "error.mixenginePipeOwner",
                    endpoint = address,
                    owner = account
                )
            }
            other => err!(
                "error.mixengineUnreachable",
                endpoint = endpoint.to_string(),
                message = other
            ),
        })
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use mixengine_platform::ipc::{Accepted, Endpoint, Listener};

    use super::*;

    /// The real transport on this OS, with no daemon to build: a `Listener` bound on the endpoint
    /// of a temporary home answers one exchange, and `dial` reaches it through the code the
    /// window uses.
    #[tokio::test]
    async fn dials_a_listener_bound_on_the_endpoint_of_a_temporary_home() {
        let home = tempfile::tempdir().expect("a temporary home");
        let run = home.path().join("run");
        std::fs::create_dir_all(&run).expect("run/");
        let endpoint = Endpoint::in_run_dir(&run).expect("an endpoint");

        let mut listener = Listener::bind(&endpoint).expect("bound");
        let server = tokio::spawn(async move {
            match listener.accept().await.expect("accepted") {
                Accepted::Trusted(mut connection) => {
                    let mut request = [0u8; 4];
                    connection.read_exact(&mut request).await.expect("read");
                    assert_eq!(&request, b"ping");
                    connection.write_all(b"pong").await.expect("write");
                }
                Accepted::Untrusted(peer) => panic!("our own connection was refused: {peer:?}"),
            }
        });

        let mut io = dial(&endpoint).await.expect("dialled");
        io.write_all(b"ping").await.expect("write");
        let mut reply = [0u8; 4];
        io.read_exact(&mut reply).await.expect("read");
        assert_eq!(&reply, b"pong");
        server.await.expect("the server task");
    }

    /// An absent daemon is a readable `AppError` naming the address it tried — the symptom of a
    /// wrong endpoint is "no daemon found", and the only way to compare by eye is to see the name.
    #[tokio::test]
    async fn an_absent_daemon_is_an_error_that_names_the_address() {
        let home = tempfile::tempdir().expect("a temporary home");
        let run = home.path().join("run");
        std::fs::create_dir_all(&run).expect("run/");
        let endpoint = Endpoint::in_run_dir(&run).expect("an endpoint");

        let error = dial(&endpoint)
            .await
            .expect_err("an absent daemon must not connect");
        assert_eq!(error.code, "error.mixengineUnreachable", "{error:?}");
        assert_eq!(
            error.params.get("endpoint"),
            Some(&endpoint.to_string()),
            "{error:?}"
        );
    }

    /// The address is the platform's rendering, not a copy of its naming rule.
    #[test]
    fn this_machine_has_an_address_of_the_platform_shape() {
        let Ok(address) = current_address() else {
            // A machine with neither `HOME` nor `LOCALAPPDATA` is one to skip, not one to fail.
            return;
        };
        if cfg!(windows) {
            assert!(address.starts_with(r"\\.\pipe\mixengine."), "{address}");
        } else {
            assert!(address.contains("/run/"), "{address}");
        }
    }
}
