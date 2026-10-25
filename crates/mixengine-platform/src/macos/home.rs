//! `~/Library/Application Support/MixEngine`, or `MixEngine-dev` for a build that is not a
//! release.

use std::path::{Path, PathBuf};

use directories::BaseDirs;

use crate::{Error, HomeDirs, Result};

/// Beside the released one.
const NAME: &str = if crate::RELEASE {
    "MixEngine"
} else {
    "MixEngine-dev"
};

#[derive(Debug, Default)]
pub(crate) struct Home;

impl HomeDirs for Home {
    fn default_home(&self) -> Result<PathBuf> {
        default_home()
    }

    /// Anything under `/Volumes/` is TCC's to gate, and the helper cannot be granted it.
    ///
    /// `/Volumes/` holds every mounted volume but the boot one's system/data group, and the boot
    /// volume's own entry there (`/Volumes/Macintosh HD`) is a symlink to `/`, which `in_full`
    /// resolves away. So a path still under `/Volumes/` once spelled the way the kernel reports it is
    /// external, network or a second volume — and TCC gates all three for a process with no
    /// responsible parent.
    fn elevated_can_read(&self, path: &Path) -> bool {
        match std::path::absolute(path) {
            Ok(absolute) => !crate::paths::in_full(&absolute).starts_with(VOLUMES),
            // Not knowing is not a reason to move somebody's home.
            Err(_) => true,
        }
    }
}

/// The platform default, with no `Host` behind it — [`crate::home::default_home`]'s answer.
pub(crate) fn default_home() -> Result<PathBuf> {
    let base = BaseDirs::new().ok_or(Error::NoHomeDirectory {
        reason: "$HOME is not set",
    })?;
    Ok(base.data_dir().join(NAME))
}

/// Where macOS mounts every volume that is not the boot volume.
const VOLUMES: &str = "/Volumes/";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_volume_under_volumes_is_one_the_helper_cannot_read() {
        assert!(!Home.elevated_can_read(Path::new("/Volumes/SSD/app/mixengine/.mixengine-home")));
    }

    #[test]
    fn the_boot_volume_is_readable_however_it_is_spelled() {
        let temporary = tempfile::tempdir().expect("a temporary directory");
        assert!(Home.elevated_can_read(temporary.path()));
        assert!(Home.elevated_can_read(&temporary.path().join("not-yet")));

        // The boot volume's own entry in `/Volumes/` is a symlink to `/` on every Mac this runs on;
        // skipped rather than failed on one where it is named otherwise.
        let boot = Path::new("/Volumes/Macintosh HD");
        if boot.is_symlink() {
            assert!(Home.elevated_can_read(&boot.join("Users")));
        }
    }
}
