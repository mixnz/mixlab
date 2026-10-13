//! `mix cert`: certificates and the local CA.

use std::io::Write as _;
use std::process::ExitCode;

use clap::Subcommand;
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    CaRotateReport, CaStatus, CaUninstallReport, CertIssue, CertIssueReport, CertStatusQuery,
    CertStatusReport, ElevationStatus, Error, JobOutcome, JobSummary, SiteRef, rpc,
};

use crate::autostart::Autostart;
use crate::client::{Client, ask, encode};
use crate::confirm::unanswered;
use crate::jobs::follow;
use crate::output::{emit, rendered};
use crate::{confirm, render};

/// `mix cert …` — one subcommand per `cert.*` method, and nothing that is not one.
///
/// **`ca-status` and not `status`.** `docs/features/tls.md` gives the short name to the per-site
/// diagnostics with a live TLS handshake, which is roadmap task **T53**, and names this command's
/// siblings `ca-uninstall` and `ca-rotate`. Taking the short name here would mean renaming it later,
/// or giving one command two unrelated jobs.
#[derive(Debug, Subcommand)]
pub(crate) enum CertCommand {
    /// Say what this home's certificate authority is: its name, its fingerprint, how long it has.
    ///
    /// **Not whether this machine trusts it.** That is a question about the operating system's own
    /// certificate stores rather than about the authority, this build does not yet ask it, and
    /// nothing printed here implies an answer to it.
    // Roadmap task T49 is what will answer it, and `mix cert ca-install` is where that will live.
    // Kept out of the help text above on purpose: a task number means nothing to whoever typed
    // `--help`, and clap prints every line of a doc comment.

    /// Give a site the certificate its names need, or every HTTPS site one.
    ///
    /// Idempotent: a certificate that still covers the right names, has more than thirty days left
    /// and was signed by the authority this home has now is left exactly as it is.
    Issue {
        /// One site, by any of its domains. Every HTTPS site when this is left out.
        #[arg(long, value_name = "DOMAIN")]
        site: Option<String>,
    },
    /// Say whether each site's padlock is green, by asking the server rather than the disk.
    ///
    /// Opens a real TLS connection to this home's front end for every site and reports the
    /// certificate it presents. That is the only thing a browser ever sees, and the only way to
    /// notice a server still holding a certificate that was replaced underneath it.
    ///
    /// Reads only. Nothing is issued, nothing is installed and nothing is reloaded.
    Status {
        /// One site, by any of its domains. Every site when this is left out.
        #[arg(long, value_name = "DOMAIN")]
        site: Option<String>,
    },
    CaStatus,

    /// Replace this home's certificate authority with a new one.
    ///
    /// Destructive: every browser holding a cached chain under the old authority stops accepting
    /// it, and every site's certificate is reissued. Nothing is replaced unless this machine can be
    /// made to trust the new one. Declining the prompt leaves this home exactly as it was.
    CaRotate {
        /// Answer the confirmation in advance, for a script with nobody at the keyboard.
        #[arg(long)]
        yes: bool,

        /// Start the work and print the job, rather than waiting for it to finish.
        #[arg(long = "no-wait")]
        no_wait: bool,
    },

    /// Take this home's certificate authority out of every store that trusts it.
    ///
    /// Leaves the certificate and its key on disk, and leaves every site's certificate alone. `mix
    /// doctor --repair` puts the trust back. Removing it from the system store needs an
    /// administrator; the browser databases do not.
    CaUninstall {
        /// Answer the confirmation in advance, for a script with nobody at the keyboard.
        #[arg(long)]
        yes: bool,

        /// Start the work and print the job, rather than waiting for it to finish.
        #[arg(long = "no-wait")]
        no_wait: bool,
    },
}

/// `mix cert …`: one call, one rendering.
///
/// **Every state exits zero, including `absent` and `unusable`.** This reports; `mix doctor` is what
/// carries a verdict. A reporting command with a failing exit is one nobody can put in front of an
/// `&&` without thinking about it, and there is nothing here a person asked to change.
pub(crate) async fn cert(
    command: CertCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    match command {
        CertCommand::CaStatus => {
            let status: CaStatus = ask(&mut client, rpc::method::CERT_CA_STATUS, None).await?;
            emit(&rendered(json, &status, || {
                render::cert::ca_status(&status)
            }))?;

            Ok(ExitCode::SUCCESS)
        }

        CertCommand::Status { site } => {
            let request = CertStatusQuery {
                site: site.map(SiteRef::Domain),
            };
            let report: CertStatusReport =
                ask(&mut client, rpc::method::CERT_STATUS, encode(&request)).await?;
            emit(&rendered(json, &report, || {
                render::cert::cert_status(&report)
            }))?;

            Ok(ExitCode::SUCCESS)
        }

        CertCommand::Issue { site } => {
            let request = CertIssue {
                site: site.map(SiteRef::Domain),
            };
            let report: CertIssueReport =
                ask(&mut client, rpc::method::CERT_ISSUE, encode(&request)).await?;
            emit(&rendered(json, &report, || {
                render::cert::cert_issue(&report)
            }))?;

            Ok(ExitCode::SUCCESS)
        }

        CertCommand::CaRotate { yes, no_wait } => {
            if !yes
                && !agreed(
                    &mut client,
                    "this will replace this home's certificate authority. Every site's certificate \
                     is reissued under the new one, and every browser holding a cached chain under \
                     the old one stops accepting it until it re-reads the store.",
                    json,
                )
                .await?
            {
                return Ok(ExitCode::SUCCESS);
            }

            let started: JobSummary = ask(&mut client, rpc::method::CERT_CA_ROTATE, None).await?;

            job_answering(&mut client, started, no_wait, json, |result| {
                serde_json::from_value::<CaRotateReport>(result)
                    .ok()
                    .map(|report| render::cert::ca_rotate(&report))
            })
            .await
        }

        CertCommand::CaUninstall { yes, no_wait } => {
            if !yes
                && !agreed(
                    &mut client,
                    "this will take this home's certificate authority out of every store on this \
                     machine that trusts it. The certificate and its key stay on disk, and \
                     `mix doctor --repair` puts the trust back.",
                    json,
                )
                .await?
            {
                return Ok(ExitCode::SUCCESS);
            }

            let started: JobSummary =
                ask(&mut client, rpc::method::CERT_CA_UNINSTALL, None).await?;

            job_answering(&mut client, started, no_wait, json, |result| {
                serde_json::from_value::<CaUninstallReport>(result)
                    .ok()
                    .map(|report| render::cert::ca_uninstall(&report))
            })
            .await
        }
    }
}

/// Say what a command is about to change, name anything else already queued, and ask.
///
/// **T64's rule, adapted to a command that queues its own work.** `mix elevation grant` can read the
/// batch before it asks, because the batch is already there; `cert.ca_uninstall` builds its own
/// batch inside the job, so what this puts in front of a person is the command's own sentence — plus
/// whatever was *already* waiting, because one grant spends one prompt on all of it and a person who
/// typed a certificate command should not discover afterwards that their hosts file moved.
async fn agreed(client: &mut Client, what: &str, json: bool) -> Result<bool, Error> {
    if json {
        return Err(unanswered());
    }

    let waiting: ElevationStatus = ask(client, rpc::method::ELEVATION_STATUS, None).await?;

    let also = match waiting.pending.is_empty() {
        true => String::new(),
        false => format!(
            "\n\nthis machine is also holding these, and one prompt covers them all:\n{}",
            render::elevation::elevation_prompt(&waiting)
        ),
    };

    match confirm::ask(&format!("{what}{also}\n\ncontinue? [y/N] ")) {
        confirm::Answer::Yes => Ok(true),

        confirm::Answer::No => {
            let _ = writeln!(std::io::stderr(), "nothing was changed");
            Ok(false)
        }

        confirm::Answer::Unanswerable => Err(unanswered()),
    }
}

/// Follow a job to its end and render whatever it produced, or print the job when it produced
/// nothing this command knows how to read.
///
/// **The result is decoded here and not in [`render::job::job_status`].** That function tries several
/// types in turn against one `serde_json::Value`, and T54's two reports are similar enough — an
/// `outcome` and a `status` apiece — that adding them to that chain is how a rotation gets rendered
/// as a removal. A command knows its own type.
async fn job_answering(
    client: &mut Client,
    started: JobSummary,
    no_wait: bool,
    json: bool,
    render_result: impl Fn(serde_json::Value) -> Option<String>,
) -> Result<ExitCode, Error> {
    if no_wait {
        emit(&rendered(json, &started, || {
            render::job::job_status(&started)
        }))?;
        return Ok(ExitCode::SUCCESS);
    }

    let finished = follow(client, started, json).await?;

    match finished.outcome.clone() {
        Some(JobOutcome::Succeeded { result }) => match render_result(result.clone()) {
            Some(said) => emit(&rendered(json, &result, || said))?,
            None => emit(&rendered(json, &finished, || {
                render::job::job_status(&finished)
            }))?,
        },
        _ => emit(&rendered(json, &finished, || {
            render::job::job_status(&finished)
        }))?,
    }

    Ok(match render::job::job_succeeded(&finished) {
        true => ExitCode::SUCCESS,
        false => ExitCode::FAILURE,
    })
}
