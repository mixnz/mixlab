//! Where MixEngine's root directory goes when the user has not chosen one.

use std::path::{Path, PathBuf};

use crate::Result;

/// The OS convention for "application data belonging to this user".
///
/// Only the *default* is platform business. What lives inside the root — `runtimes/`, `etc/`,
/// `mixengine.db` — is identical everywhere and therefore belongs to `mixengine-core`.
pub trait HomeDirs: std::fmt::Debug + Send + Sync {
    /// The default `MIXENGINE_HOME` for the current user.
    ///
    /// Per `docs/architecture/overview.md`:
    ///
    /// | OS | Directory | A build that is not a release |
    /// | --- | --- | --- |
    /// | Windows | `%LOCALAPPDATA%\MixEngine` | `%LOCALAPPDATA%\MixEngine-dev` |
    /// | macOS | `~/Library/Application Support/MixEngine` | `…/MixEngine-dev` |
    /// | Linux | `$XDG_DATA_HOME/mixengine`, falling back to `~/.local/share/mixengine` | `…/mixengine-dev` |
    ///
    /// The second column is [`crate::RELEASE`]'s doing and is not a mode anything selects at run
    /// time: it is decided when the binary is compiled.
    ///
    /// The directory is **not** created here and may not exist yet; bootstrapping it is the
    /// caller's job.
    ///
    /// # Errors
    ///
    /// [`Error::NoHomeDirectory`](crate::Error::NoHomeDirectory) when the environment does not say
    /// where the user's data lives.
    fn default_home(&self) -> Result<PathBuf>;

    /// Could a process elevated through this OS's prompt read files under `path`?
    ///
    /// **A question about where a home may go, asked before one is chosen.** On macOS the answer is
    /// no for any volume but the boot one: TCC gates removable and network volumes, and
    /// `mixengine-elevate` arrives through `osascript` and `authtrampoline` with no responsible
    /// process to inherit a grant from, so it cannot read the request in `<home>/run`.
    ///
    /// **`true` by default, which is Linux's and Windows' answer.** Root and an elevated token read
    /// an external disk there. The Linux mounts that fail the same way — FUSE without `allow_other`,
    /// NFS with `root_squash` — are not guessed at; the helper's own refusal names them instead.
    ///
    /// `path` need not exist. An answer this layer cannot work out is `true`: not knowing is not a
    /// reason to put somebody's home somewhere else.
    fn elevated_can_read(&self, path: &Path) -> bool {
        let _ = path;
        true
    }
}
