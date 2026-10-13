//! `mix domain` — roadmap task **T46**.

use std::process::ExitCode;

use clap::Subcommand;
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    DomainAdd, DomainRemove, DomainStatusQuery, DomainStatusReport, Error, SiteDetail, SiteRef, rpc,
};

use crate::autostart::Autostart;
use crate::client::{Client, ask, encode};
use crate::output::{emit, rendered};
use crate::render;

/// `mix domain …` — one subcommand per `domain.*` method, and nothing that is not one.
#[derive(Debug, Subcommand)]
pub(crate) enum DomainCommand {
    /// Give a site one more name.
    ///
    /// The new name is an alias: the site's primary domain is unchanged, because that is what its
    /// canonical URL and its certificate are named after.
    Add {
        /// The name to add.
        #[arg(value_name = "DOMAIN")]
        domain: String,

        /// Any of the site's existing domains.
        #[arg(long, value_name = "DOMAIN")]
        site: String,

        /// Accept `.local`, which belongs to mDNS and works until somebody plugs in a printer.
        #[arg(long = "i-know")]
        accept_risky_tld: bool,
    },

    /// Take one name away.
    ///
    /// Refused for a site's last domain and for its primary; `mix site update` reorders, and the
    /// first `--domain` it is given becomes the primary.
    Remove {
        /// The name to take away. It names its own site.
        #[arg(value_name = "DOMAIN")]
        domain: String,
    },

    /// What actually happens to a name, as four facts that can fail one at a time.
    Status {
        /// One name, or every name this home declares.
        #[arg(value_name = "DOMAIN")]
        domain: Option<String>,
    },
}

/// `mix domain` — roadmap task **T46**.
pub(crate) async fn domain(
    command: DomainCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    match command {
        DomainCommand::Add {
            domain,
            site,
            accept_risky_tld,
        } => {
            let add = DomainAdd {
                site: SiteRef::Domain(site),
                domain,
                accept_risky_tld,
            };
            let detail: SiteDetail =
                ask(&mut client, rpc::method::DOMAIN_ADD, encode(&add)).await?;
            emit(&rendered(json, &detail, || render::site_detail(&detail)))?;
        }

        DomainCommand::Remove { domain } => {
            let remove = DomainRemove { domain };
            let detail: SiteDetail =
                ask(&mut client, rpc::method::DOMAIN_REMOVE, encode(&remove)).await?;
            emit(&rendered(json, &detail, || render::site_detail(&detail)))?;
        }

        DomainCommand::Status { domain } => {
            let query = DomainStatusQuery { domain };
            let report: DomainStatusReport =
                ask(&mut client, rpc::method::DOMAIN_DNS_STATUS, encode(&query)).await?;
            emit(&rendered(json, &report, || render::domain_status(&report)))?;
        }
    }

    Ok(ExitCode::SUCCESS)
}
