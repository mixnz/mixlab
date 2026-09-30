//! What the desktop application is called, in the four currencies something here needs — roadmap
//! task **T107**.
//!
//! Two of these were [`crate::updates::apply`]'s until this task, pinned to `packaging/common.sh`
//! since T106; the other two arrive with the daemon's own lookup for the window, and neither a
//! display name nor a URL scheme is an updater's business. One module, four constants, and
//! `crates/mixengine-core/tests/packaging.rs` holds every one of them to the file that also
//! declares it.

/// The executable, on every operating system: `packaging/common.sh`'s `MIX_WINDOW`.
///
/// **Lower case everywhere, including Windows.** [`crate::updates::apply`]'s `swap` looks a
/// payload's name up as `directory.join(binary_name(name))`, and `binary_name` appends this
/// platform's executable suffix and nothing else — an install file spelled any other way is one
/// every future update would skip without a word. Argued in full by T105.
pub const EXECUTABLE: &str = "mixlab";

/// What macOS wraps [`EXECUTABLE`] in: `packaging/common.sh`'s `MIX_WINDOW_APP`.
///
/// A windowed application there is a directory rather than a file, which is what
/// `mixengine_platform::install::application_file_name` exists to say and what the macOS payload's
/// `provides` names. Named in prose and not linked: that crate is a dependency of this one, so a
/// link would point the wrong way down the graph and rustdoc would refuse it.
pub const BUNDLE: &str = "MixLab.app";

/// What a person calls it: `apps/desktop/src-tauri/tauri.conf.json`'s `productName`.
///
/// Printed by `mix database client` and `mix database open` where an extension's display name is
/// printed, because on a merged install the window is the client and there is no extension to name.
pub const NAME: &str = "MixLab";

/// The URL scheme a handoff is written to, and the one the window is registered for.
///
/// **The product's name, and nothing else answers** — ADR 0047. The NSIS installer writes
/// `Software\Classes\mixlab`, `packaging/linux/mixlab.desktop` declares
/// `x-scheme-handler/mixlab`, and the window refuses any other scheme, the old one included.
pub const SCHEME: &str = "mixlab";

/// The application identifier: `apps/desktop/src-tauri/tauri.conf.json`'s `identifier`.
///
/// **What names the window's own directories** — Tauri puts its saved connections, histories and
/// its webview's cache in per-user folders named after it, and `mix uninstall` finds them by this
/// name (roadmap task **T182b**).
pub const IDENTIFIER: &str = "io.github.mixnz.mixlab";

/// **The service the window keeps its credentials under** in the OS credential store: saved
/// connections' passwords and sync's sign-in. `mix uninstall` removes them with the home (roadmap
/// task **T182d**). The same spelling as `SERVICE` in `apps/desktop/src-tauri/src/secrets.rs`,
/// which cannot import it: the window depends on nothing here but proto and platform.
pub const KEYRING_SERVICE: &str = "MixLab";
