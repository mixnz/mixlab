//! `mix database`: databases, their accounts, and the client that opens one.

use std::process::ExitCode;

use clap::Subcommand;
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    CredentialStore, DaemonStatus, DatabaseAccount, DatabaseClientQuery, DatabaseClientReport,
    DatabaseCreate, DatabaseCredentials, DatabaseCredentialsQuery, DatabaseHandoff, DatabaseOpen,
    Error, ServiceId, rpc,
};

use crate::autostart::Autostart;
use crate::client::{Client, ask, encode};
use crate::output::{emit, rendered};
use crate::parse::service_id;
use crate::render;

/// `mix database …` — one subcommand per `database.*` method this build has.
///
/// `list` and `drop` are deliberately absent: nothing has asked for either, and dropping a database
/// is a decision with data behind it rather than the other half of a pair.
#[derive(Debug, Subcommand)]
pub(crate) enum DatabaseCommand {
    /// Make a database and the account that reaches it.
    ///
    /// The instance is started if it is not running. Nothing prints the password: it is put in this
    /// machine's credential store, and what is printed is where.
    Create {
        /// Which instance: `mariadb@main`, `postgres@shop`.
        #[arg(value_name = "SERVICE", value_parser = service_id)]
        service: ServiceId,

        /// The database's name.
        #[arg(long, value_name = "NAME")]
        name: String,

        /// The account's name. The database's own when nobody says.
        #[arg(long, value_name = "ACCOUNT")]
        user: Option<String>,

        /// Choose the account's password instead of generating one.
        ///
        /// With a value, that is the password. Without one, `mix` prompts and reads one line from
        /// standard input, so this also works piped: `echo secret | mix database create …
        /// --password`. It is not shown on any command line MixEngine itself runs afterwards: it
        /// goes into this machine's credential store the same way a generated password does.
        ///
        /// With an existing account of ours, this changes what is stored, and the server is
        /// realigned to it the same way it already is when a password drifts.
        #[arg(long, num_args = 0..=1, default_missing_value = "", value_name = "VALUE")]
        password: Option<String>,
    },

    /// Where this instance could be opened, and with what.
    ///
    /// MixLab, the window this MixEngine installed, when the install has one; nothing on a
    /// headless install.
    ///
    /// Reads only: starts nothing, opens nothing. "No client" is an answer, not a failure.
    Client {
        /// Which instance: `mariadb@main`, `redis@main`.
        #[arg(value_name = "SERVICE", value_parser = service_id)]
        service: ServiceId,
    },

    /// The password MixEngine holds for one account.
    ///
    /// Reads only: starts nothing. Prints the password itself, and the last line of the plain
    /// rendering is the value alone, so a script can read it with `tail -1`. This is the only `mix
    /// database` command whose whole purpose is to print a credential.
    Credentials {
        /// Which instance: `mariadb@main`, `postgres@shop`.
        #[arg(value_name = "SERVICE", value_parser = service_id)]
        service: ServiceId,

        /// The account to read. The server's administrator when nobody says.
        #[arg(long, value_name = "ACCOUNT")]
        user: Option<String>,
    },

    /// Open this instance in MixLab, the window this MixEngine installed.
    ///
    /// MixLab need not be running: a copy already open takes the connection as a new tab and this
    /// command says so, and one that is not open is started.
    ///
    /// The instance is started if it is not running. The account's password is read from this
    /// machine's credential store at that moment and handed to the client in its own environment.
    /// It is never printed and never put in an argument. Exits 1 on an install with no window (a
    /// headless install), and says so.
    Open {
        /// Which instance.
        #[arg(value_name = "SERVICE", value_parser = service_id)]
        service: ServiceId,

        /// The account to sign in as. The server's administrator when nobody says.
        #[arg(long, value_name = "ACCOUNT")]
        user: Option<String>,

        /// A database to open at.
        #[arg(long, value_name = "NAME")]
        database: Option<String>,
    },
}

/// `mix database …` — make a database and the account that reaches it, hand it to a client, or
/// read what password is stored for one.
///
/// **`create`, `client` and `open` never print a password**: what comes back is the address the
/// credential is stored under, because a password on a terminal is a password in scrollback, in a
/// tmux buffer and in a CI log. Handing one to a program that needs it is `open`, and the daemon
/// puts it in that program's environment alone. `credentials` is the one exception: its whole
/// purpose is to print one, for a project's `.env`.
pub(crate) async fn database(
    command: DatabaseCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    match command {
        DatabaseCommand::Create {
            service,
            name,
            user,
            password,
        } => {
            let password = match password.as_deref() {
                None => None,
                Some(value) if !value.is_empty() => Some(value.to_owned()),
                Some(_) => match crate::confirm::read_password(&format!(
                    "password for {} on {service}: ",
                    user.as_deref().unwrap_or(&name)
                )) {
                    Some(line) => Some(line),
                    None => {
                        eprintln!("nobody to ask; pass `--password <value>` or pipe one line in");
                        return Ok(ExitCode::from(1));
                    }
                },
            };

            let create = DatabaseCreate {
                service,
                database: name,
                user,
                password,
            };
            let account: DatabaseAccount =
                ask(&mut client, rpc::method::DATABASE_CREATE, encode(&create)).await?;
            let store = credential_store(&mut client, json).await;

            emit(&rendered(json, &account, || {
                render::database::database_created(&account, store)
            }))?;
        }

        DatabaseCommand::Client { service } => {
            let report: DatabaseClientReport = ask(
                &mut client,
                rpc::method::DATABASE_CLIENT,
                encode(&DatabaseClientQuery { service }),
            )
            .await?;

            emit(&rendered(json, &report, || {
                render::database::database_client(&report)
            }))?;
        }

        DatabaseCommand::Open {
            service,
            user,
            database,
        } => {
            let handoff: DatabaseHandoff = ask(
                &mut client,
                rpc::method::DATABASE_OPEN,
                encode(&DatabaseOpen {
                    service,
                    user,
                    database,
                }),
            )
            .await?;

            emit(&rendered(json, &handoff, || {
                render::database::database_opened(&handoff)
            }))?;

            // A client that did not open is exit 1 for `mix service start`'s reason: `mix database
            // open db && …` is a sentence about a client having opened. The answer is a state and
            // was printed as one; the code is what a script reads.
            if handoff.launched.is_none() {
                return Ok(ExitCode::from(1));
            }
        }

        DatabaseCommand::Credentials { service, user } => {
            let answer: DatabaseCredentials = ask(
                &mut client,
                rpc::method::DATABASE_CREDENTIALS,
                encode(&DatabaseCredentialsQuery { service, user }),
            )
            .await?;
            let store = credential_store(&mut client, json).await;

            emit(&rendered(json, &answer, || {
                render::database::database_credentials(&answer, store)
            }))?;
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// Which store the daemon keeps passwords in, for a sentence that says where one is. Asked only for
/// a person: `--json` hands back the daemon's own answer and needs no more. A daemon that cannot
/// say, or predates the member, is answered as it always was.
async fn credential_store(client: &mut Client, json: bool) -> Option<CredentialStore> {
    if json {
        return None;
    }
    ask::<DaemonStatus>(client, rpc::method::DAEMON_STATUS, None)
        .await
        .ok()
        .and_then(|status| status.credentials)
        .map(|credentials| credentials.store)
}
