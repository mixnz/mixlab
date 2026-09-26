//! Recording what an earlier home left on disk — roadmap task **T182f**.
//!
//! Design: `docs/specs/2026-09-27-t182f-a-reinstall-finds-what-the-last-one-kept-design.md`.

use mixengine_core::Store;
use mixengine_core::adopt::marker;
use mixengine_core::adopt::walk::{self, Claimed, Evidence, Found};
use mixengine_proto::Error;

use crate::error::ToWire as _;
use crate::jobs::JobHandle;

/// An install found its directory already there: record it when its own marker names this install
/// with the hash the index offers now — spec D2.
///
/// The hash is what makes it *this* build rather than something else under the same name: a marker
/// alone says what was installed, and the index says what this install would have put there.
///
/// # Errors
///
/// The wire form of [`mixengine_core::Error::UnrecordedInstall`] when there is no marker, when it
/// names another install, or when its hash is not the one offered; and the errors of recording it.
pub(crate) async fn claim_offered(
    store: &Store,
    found: &Found,
    offered_sha256: &str,
    handle: &JobHandle,
) -> Result<Claimed, Error> {
    let offered = marker::read(&found.path)
        .is_some_and(|marker| marker.names(&found.subject) && marker.sha256() == offered_sha256);

    if !offered {
        return Err(mixengine_core::Error::UnrecordedInstall {
            path: found.path.clone(),
            reason: "its marker is missing or names another build".to_owned(),
        }
        .to_wire());
    }

    let claimed = walk::claim(store, found, &Evidence::Marker)
        .await
        .map_err(|error| error.to_wire())?;

    handle.progress(100, "found on disk, recorded").await;

    Ok(claimed)
}
