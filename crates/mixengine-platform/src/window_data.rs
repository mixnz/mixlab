//! Where the MixLab window keeps its own files.
//!
//! **Tauri's folders, not MixEngine's.** The window saves its connections, histories and sync
//! database in the per-user folders Tauri names after the application identifier, and its webview
//! keeps a cache and logs beside them. None of it is under `MIXENGINE_HOME`, so an uninstall that
//! only knew the home left all of it behind — found on the first real Windows uninstall.
//!
//! The mapping follows Tauri 2's own `app_*_dir` answers on each system, and is a pure function of
//! the base directories so each system's mapping is tested on every system.

use std::path::{Path, PathBuf};

/// The window's folders on this machine, split by what losing them costs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowData {
    /// What a person made: saved connections, histories, snippets, the sync database. Removed only
    /// when the uninstall removes the data too.
    pub data: Vec<PathBuf>,

    /// What the window can make again: the webview's cache and the window's logs. Removed with the
    /// program whatever is kept.
    pub cache: Vec<PathBuf>,
}

/// Which system's layout to answer for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum System {
    /// Roaming for what the window saves, Local for its cache and logs.
    Windows,
    /// Application Support, with the cache, the webview's data and the logs in their own folders.
    MacOs,
    /// The XDG data and config folders, and the cache folder.
    Linux,
}

/// The base folders the layout is built from, as `directories::BaseDirs` answers them.
#[derive(Debug, Clone, Copy)]
pub struct Bases<'a> {
    /// This user's home.
    pub home: &'a Path,
    /// `BaseDirs::data_dir`.
    pub data: &'a Path,
    /// `BaseDirs::data_local_dir`.
    pub data_local: &'a Path,
    /// `BaseDirs::config_dir`.
    pub config: &'a Path,
    /// `BaseDirs::cache_dir`.
    pub cache: &'a Path,
}

/// The window's folders for `identifier` on `system`, whether they exist or not.
#[must_use]
pub fn layout(system: System, bases: Bases<'_>, identifier: &str) -> WindowData {
    let (data, cache) = match system {
        // Tauri: app_data_dir and app_config_dir are Roaming; app_local_data_dir, app_cache_dir and
        // app_log_dir are Local, and WebView2 keeps `EBWebView` in the local one.
        System::Windows => (
            vec![bases.data.join(identifier), bases.config.join(identifier)],
            vec![
                bases.data_local.join(identifier),
                bases.cache.join(identifier),
            ],
        ),
        // Tauri: data, local data and config are all Application Support; the cache is
        // ~/Library/Caches, the logs ~/Library/Logs, and WKWebView keeps ~/Library/WebKit.
        System::MacOs => (
            vec![
                bases.data.join(identifier),
                bases.data_local.join(identifier),
                bases.config.join(identifier),
            ],
            vec![
                bases.cache.join(identifier),
                bases.home.join("Library").join("Logs").join(identifier),
                bases.home.join("Library").join("WebKit").join(identifier),
            ],
        ),
        // Tauri: data and local data are ~/.local/share, where WebKitGTK and the logs also go;
        // config is ~/.config and the cache ~/.cache.
        System::Linux => (
            vec![
                bases.data.join(identifier),
                bases.data_local.join(identifier),
                bases.config.join(identifier),
            ],
            vec![bases.cache.join(identifier)],
        ),
    };

    let data = distinct(data, &[]);
    let cache = distinct(cache, &data);

    WindowData { data, cache }
}

/// The window's folders on this machine, or `None` when it names no home for this user.
#[must_use]
pub fn locate(identifier: &str) -> Option<WindowData> {
    let base = directories::BaseDirs::new()?;

    let system = if cfg!(windows) {
        System::Windows
    } else if cfg!(target_os = "macos") {
        System::MacOs
    } else {
        System::Linux
    };

    Some(layout(
        system,
        Bases {
            home: base.home_dir(),
            data: base.data_dir(),
            data_local: base.data_local_dir(),
            config: base.config_dir(),
            cache: base.cache_dir(),
        },
        identifier,
    ))
}

/// `paths` in order, each once, leaving out any already in `taken`.
fn distinct(paths: Vec<PathBuf>, taken: &[PathBuf]) -> Vec<PathBuf> {
    let mut kept: Vec<PathBuf> = Vec::new();

    for path in paths {
        if !kept.contains(&path) && !taken.contains(&path) {
            kept.push(path);
        }
    }

    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "io.github.mixnz.mixlab";

    fn p(path: &str) -> PathBuf {
        PathBuf::from(path)
    }

    /// The two folders the first real Windows uninstall left behind, each on its own side.
    #[test]
    fn windows_keeps_what_a_person_made_in_roaming_and_the_cache_in_local() {
        let bases = Bases {
            home: Path::new("C:/Users/x"),
            data: Path::new("C:/Users/x/AppData/Roaming"),
            data_local: Path::new("C:/Users/x/AppData/Local"),
            config: Path::new("C:/Users/x/AppData/Roaming"),
            cache: Path::new("C:/Users/x/AppData/Local"),
        };

        assert_eq!(
            layout(System::Windows, bases, ID),
            WindowData {
                data: vec![p("C:/Users/x/AppData/Roaming").join(ID)],
                cache: vec![p("C:/Users/x/AppData/Local").join(ID)],
            }
        );
    }

    #[test]
    fn macos_keeps_the_cache_the_logs_and_the_webview_apart() {
        let bases = Bases {
            home: Path::new("/Users/x"),
            data: Path::new("/Users/x/Library/Application Support"),
            data_local: Path::new("/Users/x/Library/Application Support"),
            config: Path::new("/Users/x/Library/Application Support"),
            cache: Path::new("/Users/x/Library/Caches"),
        };

        assert_eq!(
            layout(System::MacOs, bases, ID),
            WindowData {
                data: vec![p("/Users/x/Library/Application Support").join(ID)],
                cache: vec![
                    p("/Users/x/Library/Caches").join(ID),
                    p("/Users/x/Library/Logs").join(ID),
                    p("/Users/x/Library/WebKit").join(ID),
                ],
            }
        );
    }

    #[test]
    fn linux_keeps_data_and_config_and_leaves_the_cache_to_go() {
        let bases = Bases {
            home: Path::new("/home/x"),
            data: Path::new("/home/x/.local/share"),
            data_local: Path::new("/home/x/.local/share"),
            config: Path::new("/home/x/.config"),
            cache: Path::new("/home/x/.cache"),
        };

        assert_eq!(
            layout(System::Linux, bases, ID),
            WindowData {
                data: vec![
                    p("/home/x/.local/share").join(ID),
                    p("/home/x/.config").join(ID)
                ],
                cache: vec![p("/home/x/.cache").join(ID)],
            }
        );
    }
}
