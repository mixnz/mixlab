//! What a site request is checked and resolved against before anything is written: its domains, its
//! kind and pool, and its routes.

use std::path::Path;

use mixengine_core::{domains, projects, resolve, services, sites};
use mixengine_proto::{
    Error, ErrorCode, ProjectRef, RouteTarget, RuntimeKind, ServiceId, SiteKind, SiteRef, SiteRoute,
};

use crate::error::ToWire as _;

use super::{Holder, Sites};

impl Sites {
    /// The site a reference names, or the refusal for one that names nothing.
    ///
    /// **A `Path` that reaches a project with several sites is refused, naming them** (spec D5):
    /// picking one would be picking at random, and the person typing it in a shell has a domain to
    /// hand.
    ///
    /// # Errors
    ///
    /// `not_found` for a domain nothing answers to and a directory under no project;
    /// `invalid_argument` for a directory whose project holds more than one site.
    pub(crate) async fn expect(
        &self,
        reference: &SiteRef,
    ) -> Result<(sites::SiteRecord, Holder), Error> {
        match reference {
            SiteRef::Domain(domain) => {
                let found = sites::by_domain(&self.store, &domain.to_ascii_lowercase())
                    .await
                    .map_err(|error| error.to_wire())?
                    .ok_or_else(|| {
                        Error::new(ErrorCode::NotFound, format!("no site answers to {domain}"))
                            .with_hint("`mix site list` shows what does")
                    })?;

                let holder = self.holder_of(&found).await?;

                Ok((found, holder))
            }

            SiteRef::Path(path) => {
                let project = self.project(&ProjectRef::Path(path.clone())).await?;
                let mut found = sites::records(&self.store, Some(project.id))
                    .await
                    .map_err(|error| error.to_wire())?;

                match found.len() {
                    0 => Err(Error::new(
                        ErrorCode::NotFound,
                        format!("{} has no site", project.name),
                    )
                    .with_hint(format!("`mix site create {}` declares one", project.name))),

                    1 => Ok((found.remove(0), Holder::Project(project))),

                    _ => {
                        let named = found
                            .iter()
                            .filter_map(|site| site.domains.first())
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ");

                        Err(Error::new(
                            ErrorCode::InvalidArgument,
                            format!("{} has more than one site: {named}", project.name),
                        )
                        .with_hint("name one of those domains"))
                    }
                }
            }
        }
    }

    /// Every domain, normalised and checked, with a repeat inside one request refused before the
    /// table is asked — a unique-index violation would blame whichever came second.
    pub(super) fn checked(&self, domains: &[String], risky: bool) -> Result<Vec<String>, Error> {
        if domains.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a site needs at least one domain",
            )
            .with_hint("`--domain blog.test`, or leave it out and take `<project>.test`"));
        }

        let mut seen = std::collections::BTreeSet::new();
        let mut checked = Vec::with_capacity(domains.len());

        for domain in domains {
            let name = domains::normalised(domain, risky).map_err(|error| error.to_wire())?;

            if !seen.insert(name.clone()) {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!("{name} is named twice in one request"),
                ));
            }

            checked.push(name);
        }

        Ok(checked)
    }

    /// The kind with its pool decided and its payload checked.
    ///
    /// A php-fpm site that named no pool gets the one `core::resolve` answers at the project's root
    /// — pools are `php-fpm@<full version>`, so `^8.3` becomes `php-fpm@8.3.34`. Frozen there, on
    /// purpose: what a shell resolves tomorrow is not what a site was declared with.
    pub(super) async fn settled(
        &self,
        kind: &SiteKind,
        project: &projects::ProjectRecord,
    ) -> Result<SiteKind, Error> {
        match kind {
            SiteKind::PhpFpm { pool: Some(pool) } => {
                self.existing(std::slice::from_ref(pool)).await?;

                Ok(kind.clone())
            }

            SiteKind::PhpFpm { pool: None } => Ok(SiteKind::PhpFpm {
                pool: Some(self.resolved_pool(&project.root).await?),
            }),

            SiteKind::ReverseProxy { upstream } => {
                upstream_is_an_address(upstream)?;

                Ok(kind.clone())
            }

            SiteKind::Static | SiteKind::NodeApp { .. } => Ok(kind.clone()),
        }
    }

    /// The pool this project resolves to today, its row repaired if it had gone.
    ///
    /// **One call and not two**, because a php-fpm *route* asks the same question a php-fpm *site*
    /// does and the two answers may not differ — roadmap task **T135**. Frozen at the moment it is
    /// written, on [`Self::settled`]'s rule: what a shell resolves tomorrow is not what a site was
    /// declared with.
    async fn resolved_pool(&self, root: &Path) -> Result<ServiceId, Error> {
        let resolved = resolve::runtime(
            &self.store,
            &resolve::Question {
                kind: RuntimeKind::Php,
                cwd: Some(root),
                explicit: None,
            },
        )
        .await
        .map_err(|error| error.to_wire())?;

        let pool =
            ServiceId::parse(format!("php-fpm@{}", resolved.runtime.version)).map_err(|error| {
                Error::new(ErrorCode::Internal, format!("{error}"))
                    .with_hint("a resolved PHP version does not spell a service id, which is a bug")
            })?;

        self.repaired(pool).await
    }

    /// The routes this site is about to be written with: refused where they cannot be rendered, and
    /// with every php-fpm route that named no pool given the one this project resolves to —
    /// roadmap task **T135**.
    ///
    /// [`None`] in and [`None`] out: an update that says nothing about routes leaves them alone,
    /// which is not the same request as one that empties the list.
    pub(super) async fn routes(
        &self,
        project: &projects::ProjectRecord,
        declared: Option<&[SiteRoute]>,
    ) -> Result<Option<Vec<SiteRoute>>, Error> {
        let Some(declared) = declared else {
            return Ok(None);
        };

        route_list_is_writable(declared)?;

        let mut settled = Vec::with_capacity(declared.len());

        for route in declared {
            let target = match &route.target {
                RouteTarget::PhpFpm { pool: None } => RouteTarget::PhpFpm {
                    pool: Some(self.resolved_pool(&project.root).await?),
                },
                RouteTarget::PhpFpm { pool: Some(pool) } => {
                    self.existing(std::slice::from_ref(pool)).await?;

                    route.target.clone()
                }
                // **Made relative here, against the owner's root, exactly as a doc root is** — the
                // row stores a relative forward-slashed path on every system, and the renderer
                // joins it back on. A root outside the project is refused by the same function and
                // in the same words.
                RouteTarget::Static { root } => RouteTarget::Static {
                    root: sites::relative_doc_root(&project.root, root)
                        .map_err(|error| error.to_wire())?,
                },

                other => other.clone(),
            };

            settled.push(SiteRoute {
                path: route.path.clone(),
                target,
            });
        }

        Ok(Some(settled))
    }

    /// The pool this site is about to be written with, its row made again if it had gone —
    /// roadmap task **T122**.
    ///
    /// **A derived pool is the one nobody typed, so nobody can be told to fix it.** The branch
    /// above reads `runtime_installs` through `core::resolve` and spells `php-fpm@<version>` from
    /// what it finds; the `services` row that id needs is written by a *different* mechanism —
    /// `services::pools::ensure`, which runs at boot and after an install. `service.delete` can
    /// take that row away in between, and until this task nothing put it back before the next
    /// boot: PHP stayed installed, so `resolve` kept answering with a version whose pool no longer
    /// existed, and every php-fpm site creation on that home failed for as long as the daemon
    /// stayed up.
    ///
    /// So the repair is called here, where the need is discovered. It is the same idempotent call
    /// the boot makes and costs one query per runtime-backed recipe on a home that is already
    /// right — and it is reached only when the row is actually missing, which is close to never.
    ///
    /// **A repair that does not repair is still a refusal**, by the name of the pool: `core::sites`
    /// would refuse the write anyway, and saying it here keeps the sentence about the thing the
    /// caller can act on rather than about a foreign key.
    async fn repaired(&self, pool: ServiceId) -> Result<ServiceId, Error> {
        if services::record(&self.store, &pool).await.is_ok() {
            return Ok(pool);
        }

        if let Err(error) = mixengine_core::services::pools::ensure(
            &self.store,
            mixengine_platform::host().as_ref(),
            &crate::services::catalogue(),
        )
        .await
        {
            tracing::warn!(%pool, %error, "this site's pool was missing and could not be made again");
        }

        Ok(self
            .existing(std::slice::from_ref(&pool))
            .await?
            .into_iter()
            .next()
            .unwrap_or(pool))
    }
}

/// A route as the wire carries it: a static root spelled as this system spells a path, like the
/// doc root beside it — roadmap task **T191**. The column keeps `/`.
pub(super) fn natively(route: SiteRoute) -> SiteRoute {
    match route.target {
        RouteTarget::Static { root } => SiteRoute {
            path: route.path,
            target: RouteTarget::Static {
                root: mixengine_core::paths::native_relative(&root),
            },
        },
        target => SiteRoute {
            path: route.path,
            target,
        },
    }
}

/// A proxy target is an address: an absolute `http`/`https` URL with a host, a path allowed.
///
/// A query or a fragment is refused because a proxy target carrying one is a typo the renderer
/// would copy into a configuration file, where it would be silently ignored.
pub(super) fn upstream_is_an_address(upstream: &str) -> Result<(), Error> {
    let refusal = |because: &str| {
        Error::new(
            ErrorCode::InvalidArgument,
            format!("{upstream} is not something to forward to: {because}"),
        )
        .with_hint("an address such as http://127.0.0.1:8080")
    };

    let (scheme, rest) = upstream
        .split_once("://")
        .ok_or_else(|| refusal("it has no scheme"))?;

    if !matches!(scheme, "http" | "https") {
        return Err(refusal("only http and https are forwarded to"));
    }

    if rest.contains('?') || rest.contains('#') {
        return Err(refusal("a query or a fragment is not part of an address"));
    }

    let host = rest.split('/').next().unwrap_or_default();

    if host.is_empty() {
        return Err(refusal("it has no host"));
    }

    // **Rendered verbatim into a Caddyfile and into `nginx.conf`** — the T135 design, D10. So what
    // may appear here is a whitelist rather than a list of characters somebody remembered, and a
    // newline is the whole of the injection: `http://h\n}\nadmin 0.0.0.0:2019 {` is a valid string,
    // a valid JSON member, and a second server on Caddy's admin port.
    //
    // **Refused rather than escaped**, on the rule the four checks above already follow: an address
    // that needs escaping to survive a configuration file is an address somebody mistyped, and
    // quoting it would make the mistake reach a server instead of a person.
    if upstream.chars().any(|character| {
        character.is_control()
            || character.is_whitespace()
            || !character.is_ascii()
            || matches!(
                character,
                '`' | '"' | '\'' | '{' | '}' | '\\' | ';' | '<' | '>' | '|'
            )
    }) {
        return Err(refusal("it carries a character no address does"));
    }

    Ok(())
}

/// A route path is an absolute prefix, matched at segment boundaries — the T135 design, D10.
///
/// **The alphabet is a whitelist**, because this string is rendered into a Caddy `path` matcher and
/// into an nginx `location` regex. `.` survives it and *is* a regex metacharacter, which is why the
/// renderings escape rather than trust — the design's D5, and the one thing this function
/// deliberately does not solve.
pub(super) fn route_path_is_a_prefix(path: &str) -> Result<(), Error> {
    let refusal = |because: &str| {
        Error::new(
            ErrorCode::InvalidArgument,
            format!("{path} is not a route path: {because}"),
        )
        .with_hint("a prefix such as /api, matched at a segment boundary")
    };

    if path.len() > 255 {
        return Err(refusal("it is longer than 255 bytes"));
    }

    let Some(rest) = path.strip_prefix('/') else {
        return Err(refusal("it does not begin with /"));
    };

    if rest.is_empty() {
        return Err(refusal(
            "what answers / is the site's kind, which every site has",
        ));
    }

    for segment in rest.split('/') {
        if segment.is_empty() {
            return Err(refusal("it has an empty segment"));
        }

        if segment == "." || segment == ".." {
            return Err(refusal("a dot segment is not part of a prefix"));
        }

        if !segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '%'))
        {
            return Err(refusal("a segment carries a character no path prefix does"));
        }
    }

    Ok(())
}

/// The whole list, which is where a duplicate and a flood are visible — the T135 design, D10.
///
/// **A cap, because a site's routes are rendered into every one of its server blocks** and a list
/// nobody bounded is a configuration file nobody bounded. Thirty-two is a number chosen to be
/// larger than any hand-written site and smaller than an accident.
pub(super) fn route_list_is_writable(routes: &[SiteRoute]) -> Result<(), Error> {
    if routes.len() > 32 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{} routes is more than a site may declare", routes.len()),
        )
        .with_hint("at most 32"));
    }

    let mut seen = std::collections::BTreeSet::new();

    for route in routes {
        route_path_is_a_prefix(&route.path)?;

        if let RouteTarget::Proxy { upstream } = &route.target {
            upstream_is_an_address(upstream)?;
        }

        if !seen.insert(route.path.as_str()) {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("{} is declared twice", route.path),
            )
            .with_hint("one path answers once"));
        }
    }

    Ok(())
}
