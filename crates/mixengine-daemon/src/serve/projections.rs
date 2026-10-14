//! What a start projects out of tables compiled into this binary: `bin/` onto the disk,
//! the blueprint gallery into the database. Both run before the endpoint is bound.

use mixengine_core::{Paths, Store};

use crate::shims;

/// A re-scan and not only a refresh: a tool installed into a runtime while the daemon was
/// stopped is in no table yet, and this start is the moment it is found (T185b).
pub(super) async fn refresh_bin(shims: &shims::Shims) {
    match shims.rescan().await {
        Ok(refreshed) if refreshed.written.is_empty() && refreshed.removed.is_empty() => {
            tracing::debug!(commands = refreshed.commands.len(), "bin/ is up to date");
        }
        Ok(refreshed) => {
            tracing::info!(
                written = ?refreshed.written,
                removed = ?refreshed.removed,
                refused = ?refreshed.refused,
                "filled bin/ with one shim per command"
            );

            // **Named rather than resolved silently** — roadmap task T130, the design's §A.4. Two
            // installed packages wanting one name is settled by a total order, and somebody who
            // typed `mysql` and reached the other product's client has to be able to find out why.
            for conflict in &refreshed.conflicts {
                tracing::info!(
                    command = %conflict.name,
                    won = %conflict.won,
                    lost = ?conflict.lost,
                    "more than one installed package claims this command"
                );
            }
        }
        Err(error) => tracing::warn!(
            %error,
            "could not fill bin/; the commands in it may be missing or out of date"
        ),
    }
}

/// **The gallery is a projection of a compiled-in table into the database**, exactly as `bin/`
/// above is one onto the disk and `etc/` is one out of it — roadmap task T79, its design's D5.
/// A home whose gallery was deleted is repaired by starting the daemon.
///
/// Nothing here fails the start, for the reason the shims do not: a gallery that could not be
/// written leaves a daemon that works and a missing blueprint somebody can see, where refusing
/// to start would leave them with no daemon at all.
pub(super) async fn seed_gallery(store: &Store, paths: &Paths) {
    match mixengine_core::blueprints::gallery::seed(store, paths).await {
        Ok(seeded) if seeded.written.is_empty() && seeded.rendered.is_empty() => {
            tracing::debug!(blueprints = seeded.left.len(), "the gallery is up to date");
        }
        Ok(seeded) => tracing::info!(
            written = ?seeded.written,
            rendered = ?seeded.rendered,
            "seeded the blueprint gallery"
        ),
        Err(error) => tracing::warn!(
            %error,
            "could not seed the blueprint gallery; some built-in blueprints may be missing"
        ),
    }
}
