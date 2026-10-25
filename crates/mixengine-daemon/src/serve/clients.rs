//! The clients of everything signed this daemon reads — the package index, the extension
//! registry, the update feed — and the services built on them. Each of them fails the start
//! rather than the first call that needs it.

use std::path::Path;
use std::sync::Arc;

use super::Shared;
use crate::args::Sources;
use crate::{adopt, background, packages, runtimes, updates};

/// What [`build_clients`] hands `api::Api::new`.
pub(super) struct Clients {
    pub(super) registry:
        mixengine_core::index::Client<mixengine_core::extensions::registry::Registry>,
    pub(super) runtimes: Arc<runtimes::Runtimes>,
    pub(super) packages: Arc<packages::Packages>,
    pub(super) updates: Arc<updates::Updates>,
}

/// One transport for every signed document this daemon reads — package index, extension registry,
/// update feed. `Fetcher`'s own comment already argues this for the index client and the installer
/// that shares its cache directory: built once, handed to everything that needs it, rather than
/// once per namespace. A `reqwest::Client` is the same shape of thing, cheap to clone and meant to
/// be reused across hosts rather than built once per host, so it fails the start here beside the
/// keys rather than three times over.
pub(super) async fn build_clients(
    shared: &Shared<'_>,
    sources: &Sources,
    daemon_exe: &Path,
    adopted: &mixengine_core::adopt::walk::Walked,
    background: &background::Background,
) -> anyhow::Result<Clients> {
    let Shared {
        paths,
        store,
        host,
        events,
        jobs,
        services,
        shims,
        ..
    } = *shared;

    let transport =
        mixengine_core::index::default_transport().map_err(|error| anyhow::anyhow!("{error}"))?;

    // **Fails the start rather than the first call.** What can go wrong here is a public key that
    // is not one — the compiled-in constant, or an `--index-key` somebody pasted half of — and a
    // daemon that will refuse every install for the rest of its life should say so while the person
    // who started it is still watching.
    let fetcher = runtimes::Fetcher::new(paths, &sources.index, transport.clone())
        .map_err(|error| anyhow::anyhow!("{error}"))?;

    // The signed extension registry's client. Built beside the fetcher for its reason: a
    // compiled-in key that is not a key should fail the start rather than the first install.
    // `Extensions` itself is built by `Api::new`, after the `Sites` it holds.
    let registry = mixengine_core::extensions::registry::client(
        &sources.index.registry_url(),
        &sources.index.public_key,
        paths.cache(),
        transport.clone(),
    )
    .map_err(|error| anyhow::anyhow!("{error}"))?;
    let runtimes = runtimes::Runtimes::new(
        paths,
        store,
        Arc::clone(jobs),
        Arc::clone(&fetcher),
        Arc::clone(services),
    );
    let packages = packages::Packages::new(paths, store, Arc::clone(jobs), Arc::clone(&fetcher));
    runtimes.keeps_bin(Arc::clone(shims));

    // **And what an earlier home left without a marker.** It needs the index, which may be a
    // network fetch, so it runs in the background and never holds a start up. Only when the first
    // pass left something: on an ordinary start there is nothing to ask the index.
    if !adopted.left.is_empty() {
        let (store, paths) = (store.clone(), paths.clone());
        let (runtimes, packages, shims) = (
            Arc::clone(&runtimes),
            Arc::clone(&packages),
            Arc::clone(shims),
        );
        let fetcher = Arc::clone(&fetcher);
        // The index is asked for the kinds the first pass left, and no others.
        let names: Vec<String> = adopted
            .left
            .iter()
            .map(|left| left.found.subject.name().to_owned())
            .collect();

        background.spawn("adopt left installs", async move {
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            let catalogue = match fetcher.index.kinds(&names).await {
                Ok(catalogue) => catalogue,
                Err(error) => {
                    tracing::info!(
                        %error,
                        "installs an earlier home left without a marker wait for the next start: \
                         the package index could not be read"
                    );
                    return;
                }
            };

            let walked = adopt::with_index(&store, &paths, &catalogue.index).await;
            if walked.claimed.is_empty() {
                return;
            }

            for claimed in walked.claimed {
                let settled = match claimed {
                    mixengine_core::adopt::walk::Claimed::Runtime(summary) => {
                        let (kind, version) = (summary.kind, summary.version.clone());
                        runtimes.after_recorded(kind, &version, summary).await
                    }
                    mixengine_core::adopt::walk::Claimed::Package(summary) => {
                        packages.after_recorded(&summary)
                    }
                };
                if let Err(error) = settled {
                    tracing::warn!(%error, "a recorded install could not be given what it needs");
                }
            }

            if let Err(error) = shims.runtimes_changed().await {
                tracing::warn!(%error, "bin/ could not be refreshed after recording installs");
            }
        });
    }

    if sources.index.url != mixengine_core::index::DEFAULT_URL {
        // Worth a line of its own: from here on this daemon trusts a publisher that is not us, and
        // the log is where somebody debugging a refused signature will look for that fact.
        tracing::info!(
            url = sources.index.url,
            "reading the package index from somewhere other than the published one"
        );
    }

    // The update feed's client, and the reading of where this daemon's own binary is. Built here on
    // the fetcher's reasoning: a compiled-in key that is not a key, or an `--update-key` somebody
    // pasted half of, should fail the start rather than the first `mix self-update`.
    let updates = updates::Updates::new(
        paths,
        store,
        &sources.feed,
        Some(daemon_exe),
        Arc::clone(host),
        events.clone(),
        transport,
    )
    .map_err(|error| anyhow::anyhow!("{error}"))?;

    // The install completes itself from this version's staged payload before the update records are
    // read, which `restore_after_update` does later, in `started::after_an_update`. `bin/` was
    // filled by `projections::refresh_bin` without what was missing, so when anything was added it
    // is filled again.
    if !updates.complete_install().await.is_empty()
        && let Err(error) = shims.runtimes_changed().await
    {
        tracing::warn!(%error, "bin/ could not be refreshed after the install completed itself");
    }

    if sources.feed.url != mixengine_core::updates::DEFAULT_URL {
        // Worth its own line, and worth more than the index's: from here on this daemon would take
        // the binaries it runs as itself from a publisher that is not us.
        tracing::warn!(
            url = sources.feed.url,
            "reading the update feed from somewhere other than the published one"
        );
    }

    Ok(Clients {
        registry,
        runtimes,
        packages,
        updates,
    })
}
