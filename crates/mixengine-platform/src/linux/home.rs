//! `$XDG_DATA_HOME/mixengine`, falling back to `~/.local/share/mixengine`; `mixengine-dev` for
//! a build that is not a release.

use std::path::PathBuf;

use directories::BaseDirs;

use crate::{Error, HomeDirs, Result};

/// Beside the released one. Lowercase, like everything else under `$XDG_DATA_HOME`.
const NAME: &str = if crate::RELEASE {
    "mixengine"
} else {
    "mixengine-dev"
};

#[derive(Debug, Default)]
pub(crate) struct Home;

impl HomeDirs for Home {
    fn default_home(&self) -> Result<PathBuf> {
        default_home()
    }
}

/// The platform default, with no `Host` behind it — [`crate::home::default_home`]'s answer.
pub(crate) fn default_home() -> Result<PathBuf> {
    // `data_dir()` already implements the XDG fallback, so an unset or relative
    // `$XDG_DATA_HOME` lands on `~/.local/share` the way the spec requires.
    let base = BaseDirs::new().ok_or(Error::NoHomeDirectory {
        reason: "neither $XDG_DATA_HOME nor $HOME is set",
    })?;
    Ok(base.data_dir().join(NAME))
}
