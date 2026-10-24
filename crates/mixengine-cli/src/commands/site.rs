//! `mix site`.

use std::process::ExitCode;

use clap::Subcommand;
use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    Error, ErrorCode, ProjectRef, RouteTarget, ServiceId, ServiceList, SiteCreate, SiteCreation,
    SiteDetail, SiteKind, SiteList, SiteListQuery, SiteQuery, SiteRef, SiteRemoval, SiteRoute,
    SiteShare, SiteSharing, SiteState, SiteUpdate, rpc,
};

use crate::autostart::Autostart;
use crate::client::{Client, ask, encode};
use crate::commands::project::here;
use crate::output::{emit, rendered};
use crate::parse::{for_seconds, service_id};
use crate::render;

/// `mix site …` — one subcommand per `site.*` method, and nothing that is not one.
#[derive(Debug, Subcommand)]
pub(crate) enum SiteCommand {
    /// Declare a site under a project.
    ///
    /// With nothing but a project named, whatever the `[site]` and `[[services]]` in that project's
    /// `mixengine.toml` say is used. That is how you adopt a colleague's site. With several sites in
    /// the file, `--from` says which.
    #[command(alias = "import")]
    Create {
        /// The project. Defaults to whichever project the current directory is in.
        #[arg(long, value_name = "PROJECT")]
        project: Option<String>,

        /// Which site in `mixengine.toml` to adopt, by any of its names. Needed when the file
        /// declares more than one.
        #[arg(long, value_name = "DOMAIN")]
        from: Option<String>,

        /// A domain. The first is the primary; repeat for aliases. Defaults to `<project>.test`.
        #[arg(long = "domain", value_name = "DOMAIN")]
        domains: Vec<String>,

        /// What is served, relative to the project's root. Defaults to the root itself.
        #[arg(long, value_name = "DIR")]
        doc_root: Option<String>,

        /// What serves it.
        #[arg(long, value_enum, value_name = "KIND")]
        kind: Option<SiteKindArg>,

        /// Where a `reverse-proxy` forwards to.
        #[arg(long, value_name = "URL", required_if_eq("kind", "reverse-proxy"))]
        upstream: Option<String>,

        /// The port a `node-app` listens on.
        #[arg(long, value_name = "PORT", required_if_eq("kind", "node-app"))]
        port: Option<u16>,

        /// The php-fpm pool a `php-fpm` site uses. Defaults to whatever this directory resolves to.
        #[arg(long, value_name = "SERVICE", value_parser = service_id)]
        pool: Option<ServiceId>,

        /// Forward a path prefix to an address: `/api=http://127.0.0.1:3003/xyz`. Repeatable.
        ///
        /// The upstream's path, when it has one, replaces the matched prefix.
        #[arg(long = "proxy", value_name = "PATH=URL")]
        proxy: Vec<String>,

        /// Answer a path prefix with a php-fpm pool: `/admin=php-fpm@8.3.33`, or `/admin` alone for
        /// whatever this project resolves to. Repeatable.
        #[arg(long = "php", value_name = "PATH[=POOL]")]
        php: Vec<String>,

        /// Serve a path prefix from a directory: `/assets=dist`. The prefix is stripped.
        /// Repeatable.
        #[arg(long = "files", value_name = "PATH=DIR")]
        files: Vec<String>,

        /// Remove every route this site has.
        #[arg(long, conflicts_with_all = ["proxy", "php", "files"])]
        no_routes: bool,

        /// A service the site declares, as `mariadb@main`. May be given more than once.
        #[arg(long = "service", value_name = "SERVICE", value_parser = service_id)]
        services: Vec<ServiceId>,

        /// Serve it over HTTPS too, with a certificate from this home's own authority.
        #[arg(long)]
        https: Option<bool>,

        /// Redirect the plaintext address to the HTTPS one. Needs `--https true`.
        #[arg(long)]
        https_redirect: Option<bool>,

        /// Accept a `.local` domain, which belongs to mDNS.
        #[arg(long = "i-know")]
        accept_risky_tld: bool,
    },

    /// List the sites this home has been told about.
    List {
        /// Only this project's.
        #[arg(long, value_name = "PROJECT")]
        project: Option<String>,
    },

    /// Show one, with its domains, its pool and its services.
    Show {
        #[command(flatten)]
        site: WhichSite,
    },

    /// Change what a site is.
    ///
    /// `--domain` and `--service` **replace** rather than add to what the site had: giving neither
    /// changes neither.
    Update {
        #[command(flatten)]
        site: WhichSite,

        /// A domain. The first is the primary; repeat for aliases. Replaces the whole list.
        #[arg(long = "domain", value_name = "DOMAIN")]
        domains: Vec<String>,

        /// A new doc root.
        #[arg(long, value_name = "DIR")]
        doc_root: Option<String>,

        /// A new kind.
        #[arg(long, value_enum, value_name = "KIND")]
        kind: Option<SiteKindArg>,

        /// Where a `reverse-proxy` forwards to.
        #[arg(long, value_name = "URL", required_if_eq("kind", "reverse-proxy"))]
        upstream: Option<String>,

        /// The port a `node-app` listens on.
        #[arg(long, value_name = "PORT", required_if_eq("kind", "node-app"))]
        port: Option<u16>,

        /// The php-fpm pool.
        #[arg(long, value_name = "SERVICE", value_parser = service_id)]
        pool: Option<ServiceId>,

        /// Forward a path prefix to an address: `/api=http://127.0.0.1:3003/xyz`. Replaces the
        /// whole list, together with `--php` and `--files`.
        #[arg(long = "proxy", value_name = "PATH=URL")]
        proxy: Vec<String>,

        /// Answer a path prefix with a php-fpm pool: `/admin=php-fpm@8.3.33`, or `/admin` alone for
        /// whatever this project resolves to. Replaces the whole list.
        #[arg(long = "php", value_name = "PATH[=POOL]")]
        php: Vec<String>,

        /// Serve a path prefix from a directory: `/assets=dist`. The prefix is stripped. Replaces
        /// the whole list.
        #[arg(long = "files", value_name = "PATH=DIR")]
        files: Vec<String>,

        /// Remove every route this site has.
        #[arg(long, conflicts_with_all = ["proxy", "php", "files"])]
        no_routes: bool,

        /// A service the site declares. Replaces the whole list.
        #[arg(long = "service", value_name = "SERVICE", value_parser = service_id)]
        services: Vec<ServiceId>,

        /// Whether HTTPS is declared.
        #[arg(long)]
        https: Option<bool>,

        /// Redirect the plaintext address to the HTTPS one. Needs HTTPS enabled, before or with
        /// this same update.
        #[arg(long)]
        https_redirect: Option<bool>,

        /// Serve it, or stop serving it.
        #[arg(long, value_enum, value_name = "STATE")]
        state: Option<SiteStateArg>,

        /// Accept a `.local` domain.
        #[arg(long = "i-know")]
        accept_risky_tld: bool,
    },

    /// Let the local network reach this site, and print a QR code for it.
    ///
    /// This site only: every other site keeps answering on loopback alone. The certificate gains
    /// the LAN address, and one administrator prompt asks for the firewall rule.
    Share {
        #[command(flatten)]
        site: WhichSite,

        /// Which network to share on, by the name this machine gives it.
        ///
        /// Needed only where more than one is up. MixEngine refuses to choose rather than putting a
        /// site on a network you did not mean, and names the candidates when it does.
        #[arg(long, value_name = "NAME")]
        interface: Option<String>,

        /// How long to share for: `30s`, `90m`, `2h`, `1d`, or a bare number of seconds.
        ///
        /// Measured from when the share began, so asking for a length shorter than the site has
        /// already been shared for is refused rather than ending it on the spot. Off by default: a
        /// share with no `--for` lasts until you unshare it or this machine leaves the network.
        #[arg(long = "for", value_name = "LENGTH", value_parser = for_seconds)]
        r#for: Option<u64>,
    },

    /// Take it back off the local network.
    ///
    /// Removes the firewall rule, rebinds to loopback and reissues the certificate without the
    /// address. A site that is not shared is left as it is.
    Unshare {
        #[command(flatten)]
        site: WhichSite,
    },

    /// Serve this site.
    ///
    /// This sets a flag and tells the front end to read its configuration again. Nothing is
    /// started: a site is not a process, and the services it uses have states of their own.
    Start {
        #[command(flatten)]
        site: WhichSite,
    },

    /// Stop serving this site, keeping the declaration.
    Stop {
        #[command(flatten)]
        site: WhichSite,
    },

    /// Forget a site. The files are left exactly as they are.
    Delete {
        #[command(flatten)]
        site: WhichSite,
    },
}

/// Which site a command is about.
///
/// **The default is the directory you are in**, on
/// [`WhichProject`](crate::commands::project::WhichProject)'s rule: with no argument `mix` sends
/// the working directory and the daemon walks up to the nearest registered project, then to its
/// site. A project holding several is refused there, naming them — which is a sentence this
/// client only prints.
#[derive(Debug, clap::Args)]
pub(crate) struct WhichSite {
    /// Any of the site's domains. Defaults to the site of whichever project you are in.
    #[arg(value_name = "DOMAIN")]
    domain: Option<String>,
}

/// What serves a site, as a person types it.
///
/// `SiteKindArg` rather than `Kind`: that name is already the runtime filter three commands take,
/// and one word meaning two things in one file is a rename waiting to go wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum SiteKindArg {
    /// PHP through a php-fpm pool.
    PhpFpm,
    /// Files, and nothing running.
    Static,
    /// Everything forwarded to an address you already have listening.
    ReverseProxy,
    /// A node process you run, on a port.
    NodeApp,
}

/// Whether the web server should serve a site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(crate) enum SiteStateArg {
    /// Serve it.
    Enabled,
    /// Declare it and do not serve it.
    Disabled,
}

pub(crate) async fn site(
    command: SiteCommand,
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
) -> Result<ExitCode, Error> {
    let mut client = Client::connect(endpoint, autostart).await?;

    match command {
        SiteCommand::Create {
            project,
            from,
            domains,
            doc_root,
            kind,
            upstream,
            port,
            pool,
            proxy,
            php,
            files,
            no_routes,
            services,
            https,
            https_redirect,
            accept_risky_tld,
        } => {
            let create = SiteCreate {
                project: whose(project)?,
                from,
                domains: (!domains.is_empty()).then_some(domains),
                doc_root,
                kind: site_kind(kind, upstream, port, pool)?,
                services: (!services.is_empty()).then_some(services),
                routes: site_routes(&proxy, &php, &files, no_routes)?,
                https,
                https_redirect,
                accept_risky_tld,
            };
            let creation: SiteCreation =
                ask(&mut client, rpc::method::SITE_CREATE, encode(&create)).await?;
            emit(&rendered(json, &creation, || {
                render::site::site_detail(&creation.site)
            }))?;
        }

        SiteCommand::List { project } => {
            let query = SiteListQuery {
                project: project.map(ProjectRef::Name),
            };
            let list: SiteList = ask(&mut client, rpc::method::SITE_LIST, encode(&query)).await?;
            emit(&rendered(json, &list, || render::site::site_list(&list)))?;
        }

        SiteCommand::Show { site } => {
            let query = SiteQuery {
                site: which_site(site)?,
            };
            let detail: SiteDetail =
                ask(&mut client, rpc::method::SITE_SHOW, encode(&query)).await?;
            // **And why it cannot start, where that is so.** Asked of the listing the daemon
            // already answers; `--json` stays the site alone.
            let services: ServiceList = ask(&mut client, rpc::method::SERVICE_LIST, None).await?;
            emit(&rendered(json, &detail, || {
                render::site::site_detail(&detail)
                    + &render::site::site_failures(&detail, &services)
            }))?;
        }

        SiteCommand::Start { site } => {
            let query = SiteQuery {
                site: which_site(site)?,
            };
            let detail: SiteDetail =
                ask(&mut client, rpc::method::SITE_START, encode(&query)).await?;
            emit(&rendered(json, &detail, || {
                render::site::site_detail(&detail)
            }))?;
        }

        SiteCommand::Stop { site } => {
            let query = SiteQuery {
                site: which_site(site)?,
            };
            let detail: SiteDetail =
                ask(&mut client, rpc::method::SITE_STOP, encode(&query)).await?;
            emit(&rendered(json, &detail, || {
                render::site::site_detail(&detail)
            }))?;
        }

        SiteCommand::Update {
            site,
            domains,
            doc_root,
            kind,
            upstream,
            port,
            pool,
            proxy,
            php,
            files,
            no_routes,
            services,
            https,
            https_redirect,
            state,
            accept_risky_tld,
        } => {
            let update = SiteUpdate {
                site: which_site(site)?,
                domains: (!domains.is_empty()).then_some(domains),
                doc_root,
                kind: site_kind(kind, upstream, port, pool)?,
                services: (!services.is_empty()).then_some(services),
                routes: site_routes(&proxy, &php, &files, no_routes)?,
                https,
                https_redirect,
                state: state.map(|state| match state {
                    SiteStateArg::Enabled => SiteState::Enabled,
                    SiteStateArg::Disabled => SiteState::Disabled,
                }),
                accept_risky_tld,
            };
            let detail: SiteDetail =
                ask(&mut client, rpc::method::SITE_UPDATE, encode(&update)).await?;
            emit(&rendered(json, &detail, || {
                render::site::site_detail(&detail)
            }))?;
        }

        SiteCommand::Delete { site } => {
            let query = SiteQuery {
                site: which_site(site)?,
            };
            let removal: SiteRemoval =
                ask(&mut client, rpc::method::SITE_DELETE, encode(&query)).await?;
            emit(&rendered(json, &removal, || {
                render::site::site_removal(&removal)
            }))?;
        }

        SiteCommand::Share {
            site,
            interface,
            r#for,
        } => {
            let request = SiteShare {
                site: which_site(site)?,
                interface,
                for_seconds: r#for,
            };
            let sharing: SiteSharing =
                ask(&mut client, rpc::method::SITE_SHARE, encode(&request)).await?;
            emit(&rendered(json, &sharing, || {
                render::site::site_shared(&sharing)
            }))?;
        }

        SiteCommand::Unshare { site } => {
            let query = SiteQuery {
                site: which_site(site)?,
            };
            ask::<()>(&mut client, rpc::method::SITE_UNSHARE, encode(&query)).await?;
            emit(&rendered(json, &(), || {
                "no longer shared on the local network\n".to_owned()
            }))?;
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// The four flags a kind is spelled with on a command line, as the one value the API takes.
///
/// Assembly rather than logic: which flags a kind needs is decided by clap's `required_if_eq`, and
/// what a kind *means* is the daemon's. What this does is put a tagged enum back together out of
/// the flat arguments a shell can carry.
fn site_kind(
    kind: Option<SiteKindArg>,
    upstream: Option<String>,
    port: Option<u16>,
    pool: Option<ServiceId>,
) -> Result<Option<SiteKind>, Error> {
    let missing = |flag: &str, because: &str| {
        Error::new(ErrorCode::InvalidArgument, format!("{flag} {because}"))
    };

    Ok(match kind {
        // `--pool` on its own says php-fpm without saying it, which is the only kind a pool
        // belongs to; nothing named at all leaves the whole decision to the daemon.
        None => pool.map(|pool| SiteKind::PhpFpm { pool: Some(pool) }),
        Some(SiteKindArg::PhpFpm) => Some(SiteKind::PhpFpm { pool }),
        Some(SiteKindArg::Static) => Some(SiteKind::Static),
        Some(SiteKindArg::ReverseProxy) => Some(SiteKind::ReverseProxy {
            upstream: upstream.ok_or_else(|| missing("--upstream", "says where to forward to"))?,
        }),
        Some(SiteKindArg::NodeApp) => Some(SiteKind::NodeApp {
            port: port.ok_or_else(|| missing("--port", "says where the node process listens"))?,
        }),
    })
}

/// The three route flags as the one list the API takes.
///
/// **The whole list in one request, and never a read-modify-write.** A client that fetched a site's
/// routes, changed one and sent them back would be holding business logic and racing another writer;
/// these flags build the list a person typed and hand it over, which is the same thing `--domain`
/// and `--service` already do.
///
/// Assembly rather than logic, on [`site_kind`]'s rule: what a route *means* is the daemon's, and a
/// path it refuses comes back in the daemon's own words.
fn site_routes(
    proxy: &[String],
    php: &[String],
    files: &[String],
    no_routes: bool,
) -> Result<Option<Vec<SiteRoute>>, Error> {
    if no_routes {
        if !proxy.is_empty() || !php.is_empty() || !files.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "--no-routes and a route in the same command".to_owned(),
            )
            .with_hint("leave --no-routes off to declare routes; it is what empties the list"));
        }

        return Ok(Some(Vec::new()));
    }

    if proxy.is_empty() && php.is_empty() && files.is_empty() {
        return Ok(None);
    }

    // Split on the **first** `=`: a query is not part of an address, so a URL here has none, and a
    // directory that holds one is a directory this daemon will refuse by name.
    let split = |value: &str, flag: &str, what: &str| -> Result<(String, String), Error> {
        value
            .split_once('=')
            .map(|(path, rest)| (path.to_owned(), rest.to_owned()))
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::InvalidArgument,
                    format!("{value} is not a {flag} route"),
                )
                .with_hint(format!("<path>={what}, as in {flag} /api=…"))
            })
    };

    let mut routes = Vec::with_capacity(proxy.len() + php.len() + files.len());

    for value in proxy {
        let (path, upstream) = split(value, "--proxy", "an address")?;

        routes.push(SiteRoute {
            path,
            target: RouteTarget::Proxy { upstream },
        });
    }

    for value in php {
        // A pool is optional: `--php /admin` leaves the daemon to resolve the one this project
        // resolves to, exactly as a php-fpm site with no `--pool` does.
        let (path, pool) = match value.split_once('=') {
            Some((path, pool)) => {
                let pool = service_id(pool).map_err(|because| {
                    Error::new(ErrorCode::InvalidArgument, because)
                        .with_hint("a pool such as php-fpm@8.3.33, or no `=` at all to resolve one")
                })?;

                (path.to_owned(), Some(pool))
            }
            None => (value.clone(), None),
        };

        routes.push(SiteRoute {
            path,
            target: RouteTarget::PhpFpm { pool },
        });
    }

    for value in files {
        let (path, root) = split(value, "--files", "a directory")?;

        routes.push(SiteRoute {
            path,
            target: RouteTarget::Static { root },
        });
    }

    Ok(Some(routes))
}

/// Which site, defaulting to the directory this `mix` was run in.
fn which_site(site: WhichSite) -> Result<SiteRef, Error> {
    match site.domain {
        Some(domain) => Ok(SiteRef::Domain(domain)),
        None => Ok(SiteRef::Path(here(None)?.display().to_string())),
    }
}

/// Which project a `--project` names, defaulting to the directory this `mix` was run in.
fn whose(project: Option<String>) -> Result<ProjectRef, Error> {
    match project {
        Some(name) => Ok(ProjectRef::Name(name)),
        None => Ok(ProjectRef::Path(here(None)?.display().to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixengine_proto::RouteTarget;

    /// Three flags build the whole list in one request — no read-modify-write in the client, so no
    /// race and no business logic here.
    #[test]
    fn three_flags_build_one_route_list() {
        let routes = site_routes(
            &["/api=http://127.0.0.1:3003/xyz".to_owned()],
            &["/admin=php-fpm@8.3.33".to_owned(), "/old".to_owned()],
            &["/assets=dist".to_owned()],
            false,
        )
        .expect("a list")
        .expect("some routes");

        assert_eq!(routes.len(), 4);
        assert_eq!(routes[0].path, "/api");
        assert_eq!(
            routes[0].target,
            RouteTarget::Proxy {
                upstream: "http://127.0.0.1:3003/xyz".to_owned()
            }
        );
        assert_eq!(
            routes[2].target,
            RouteTarget::PhpFpm { pool: None },
            "a `--php` with no pool leaves the resolution to the daemon, as a site does"
        );
        assert_eq!(
            routes[3].target,
            RouteTarget::Static {
                root: "dist".to_owned()
            }
        );

        // Nothing typed leaves the list alone; `--no-routes` empties it. Two different requests.
        assert_eq!(site_routes(&[], &[], &[], false).expect("nothing"), None);
        assert_eq!(
            site_routes(&[], &[], &[], true).expect("cleared"),
            Some(Vec::new())
        );

        // `--no-routes` beside a route is refused rather than ordered. clap refuses it first; this
        // keeps the function honest for any caller that is not clap.
        assert!(site_routes(&["/a=http://h:1".to_owned()], &[], &[], true).is_err());

        // A value with no `=` is a proxy route with nowhere to go.
        assert!(site_routes(&["/a".to_owned()], &[], &[], false).is_err());
    }
}
