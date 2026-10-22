use std::fmt;
use std::path::Path;
use std::process::Command;

/// What one reading found, per thing. [`None`] is "nothing of ours there".
pub(super) struct Reading {
    hosts: Option<String>,
    resolver: Option<String>,
    port_access: Option<String>,
    trust: Option<String>,
    helper: Option<String>,
    audit_log: Option<String>,
    autostart: Option<String>,
    path_entry: Option<String>,
}

impl Reading {
    /// Did this machine end up holding anything at all?
    ///
    /// A runner that produced none of it proves nothing by having none of it removed, which is
    /// why the round trip asserts this before it removes anything.
    pub(super) fn anything(&self) -> bool {
        self.rows().iter().any(|(_, found)| found.is_some())
    }

    /// Assert that everything this reading found is gone from `after`.
    ///
    /// **Per row, and only the rows this machine actually produced.** A leg with no resolver
    /// mechanism never wrote one, and demanding its removal would be demanding the removal of
    /// something that was never there.
    ///
    /// **The privileged helper is the one exception, and it is Windows'.** A file whose image is
    /// mapped cannot be unlinked, and the helper is the running program when it removes itself,
    /// so there the assertion is that the operating system has *accepted* the removal — the path
    /// appearing in its own pending-rename queue (the T87 design, D8).
    pub(super) fn gone(&self, after: &Reading) {
        for ((name, before), (_, left)) in self.rows().iter().zip(after.rows()) {
            let Some(before) = before else {
                println!("{name}: this runner never had one, so nothing is asserted");
                continue;
            };

            if *name == "the privileged helper" && cfg!(windows) {
                assert!(
                    scheduled_for_removal(),
                    "the helper is still at {before} and this system has not been asked to \
                         remove it at the next restart"
                );
                continue;
            }

            assert!(
                left.is_none(),
                "{name} is still on this machine: was {before}, now {left:?}"
            );
        }
    }

    /// Each row, named for the assertion that fails on it.
    fn rows(&self) -> [(&'static str, &Option<String>); 8] {
        [
            ("the managed hosts block", &self.hosts),
            ("the resolver wiring", &self.resolver),
            ("the port grant", &self.port_access),
            ("the certificate authority", &self.trust),
            ("the privileged helper", &self.helper),
            ("the audit log", &self.audit_log),
            ("the autostart entry", &self.autostart),
            ("the PATH entry", &self.path_entry),
        ]
    }
}

impl fmt::Display for Reading {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (name, found) in self.rows() {
            match found {
                Some(what) => writeln!(f, "{name}: {what}")?,
                None => writeln!(f, "{name}: nothing")?,
            }
        }

        Ok(())
    }
}

/// Read this machine, now.
///
/// `authority` is the eight-character key-id of the home being asked about, which is what scopes
/// the trust row to one home's certificate rather than to the product's name. `site` does the same
/// for the hosts row: every home writes a block of its own (ADR 0066), and the one holding this
/// home's name is this home's.
pub(super) fn reading(authority: &str, site: &str) -> Reading {
    Reading {
        hosts: containing(&hosts_file(), site),
        resolver: resolver(),
        port_access: port_access(),
        trust: trust(authority),
        helper: there(&helper()),
        audit_log: there(&audit_log()),
        autostart: autostart(),
        path_entry: path_entry(),
    }
}

/// `%SystemRoot%\System32\drivers\etc\hosts`, or `/etc/hosts`.
fn hosts_file() -> std::path::PathBuf {
    #[cfg(windows)]
    {
        std::path::PathBuf::from(
            std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_owned()),
        )
        .join("System32\\drivers\\etc\\hosts")
    }
    #[cfg(unix)]
    {
        std::path::PathBuf::from("/etc/hosts")
    }
}

/// `%ProgramFiles%\MixEngine\mixengine-elevate.exe`, or the two Unix constants.
fn helper() -> std::path::PathBuf {
    #[cfg(windows)]
    {
        std::path::PathBuf::from(
            std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_owned()),
        )
        .join("MixEngine\\mixengine-elevate.exe")
    }
    #[cfg(target_os = "macos")]
    {
        std::path::PathBuf::from("/Library/PrivilegedHelperTools/dev.mixengine.elevate")
    }
    #[cfg(target_os = "linux")]
    {
        std::path::PathBuf::from("/usr/local/libexec/mixengine/mixengine-elevate")
    }
}

/// `%ProgramData%\MixEngine\elevate.log`, or the two Unix constants.
fn audit_log() -> std::path::PathBuf {
    #[cfg(windows)]
    {
        std::path::PathBuf::from(
            std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".to_owned()),
        )
        .join("MixEngine\\elevate.log")
    }
    #[cfg(target_os = "macos")]
    {
        std::path::PathBuf::from("/Library/Logs/MixEngine/elevate.log")
    }
    #[cfg(target_os = "linux")]
    {
        std::path::PathBuf::from("/var/log/mixengine/elevate.log")
    }
}

/// Whatever routes a TLD at a DNS server of ours.
fn resolver() -> Option<String> {
    #[cfg(windows)]
    {
        said(
            "reg",
            &[
                "query",
                "HKLM\\SYSTEM\\CurrentControlSet\\services\\Dnscache\\Parameters\\DnsPolicyConfig",
                "/s",
            ],
        )
        .filter(|out| out.contains(".test"))
    }
    #[cfg(target_os = "macos")]
    {
        // By name, and not everything in the directory: `/etc/resolver` is shared, and another
        // product's file there is not ours to report as left behind. These are the TLDs
        // MixEngine can ever have written — `mixengine_proto::domains::WIRED_TLDS`, spelled out
        // again for this module's reason.
        ["test", "localhost", "internal"]
            .into_iter()
            .find_map(|tld| listing(Path::new("/etc/resolver"), tld))
    }
    #[cfg(target_os = "linux")]
    {
        listing(Path::new("/etc/systemd/network"), "mixengine")
    }
}

/// The packet filter on macOS; nothing MixEngine writes to a file on the other two.
fn port_access() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        containing(Path::new("/etc/pf.conf"), "mixengine")
            .or_else(|| there(Path::new("/etc/pf.anchors/dev.mixengine")))
            .or_else(|| there(Path::new("/Library/LaunchDaemons/dev.mixengine.pf.plist")))
    }
    // Linux puts a capability on a binary inside the home, which goes with the home; Windows
    // reserves no port below 1024 and grants nothing at all.
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// This home's authority, in whichever store this system keeps trusted roots in.
///
/// **By key-id on the two systems that keep certificates by subject.** T48 names every authority
/// `MixEngine Local CA <key-id>`, so the id is what tells one home's from another's — and telling
/// them apart is the whole point here, because the suites that ran before this one left their
/// own in the same store.
///
/// Linux needs no such scoping: the anchor is one file with one fixed name, so the file being
/// there *is* the answer.
fn trust(authority: &str) -> Option<String> {
    #[cfg(windows)]
    {
        said("certutil", &["-store", "Root"])
            .filter(|out| out.to_lowercase().contains(&authority.to_lowercase()))
            .map(|_| format!("LocalMachine\\Root holds {authority}"))
    }
    #[cfg(target_os = "macos")]
    {
        said(
            "security",
            &[
                "find-certificate",
                "-a",
                "-c",
                "MixEngine",
                "/Library/Keychains/System.keychain",
            ],
        )
        .filter(|out| out.contains(authority))
        .map(|_| format!("the System keychain holds {authority}"))
    }
    #[cfg(target_os = "linux")]
    {
        // One file with one fixed name, so `authority` has nothing to add here — named rather
        // than ignored silently, on this module's own rule about being explicit.
        let _ = authority;

        listing(Path::new("/usr/local/share/ca-certificates"), "mixengine")
            .or_else(|| listing(Path::new("/etc/pki/ca-trust/source/anchors"), "mixengine"))
    }
}

/// The entry that starts a daemon at login.
fn autostart() -> Option<String> {
    #[cfg(windows)]
    {
        said("schtasks", &["/Query", "/TN", "MixEngine"])
    }
    #[cfg(target_os = "macos")]
    {
        under_home("Library/LaunchAgents/dev.mixengine.daemon.plist")
    }
    #[cfg(target_os = "linux")]
    {
        under_home(".config/systemd/user/mixengine.service")
    }
}

/// `<root>/bin` on this user's PATH, wherever this system persists one.
fn path_entry() -> Option<String> {
    #[cfg(windows)]
    {
        said("reg", &["query", "HKCU\\Environment", "/v", "Path"])
            .filter(|out| out.to_lowercase().contains("mixengine"))
    }
    #[cfg(unix)]
    {
        let home = std::env::var_os("HOME")?;

        [
            ".profile",
            ".bash_profile",
            ".zprofile",
            ".bashrc",
            ".zshrc",
        ]
        .into_iter()
        .find_map(|name| containing(&Path::new(&home).join(name), "MixEngine"))
    }
}

/// Has this system been asked to remove the helper at the next restart?
///
/// Windows' own removal queue, read as the registry value it is: `MoveFileExW` with
/// `MOVEFILE_DELAY_UNTIL_REBOOT` is what writes it.
#[cfg(windows)]
fn scheduled_for_removal() -> bool {
    said(
        "reg",
        &[
            "query",
            "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager",
            "/v",
            "PendingFileRenameOperations",
        ],
    )
    .is_some_and(|out| out.to_lowercase().contains("mixengine"))
}

/// Never reached off Windows; declared so the one shared assertion compiles everywhere.
#[cfg(not(windows))]
fn scheduled_for_removal() -> bool {
    false
}

/// The names in `directory` that hold `needle`, when there are any.
#[cfg(unix)]
fn listing(directory: &Path, needle: &str) -> Option<String> {
    let names: Vec<String> = directory
        .read_dir()
        .ok()?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.to_lowercase().contains(needle))
        .collect();

    (!names.is_empty()).then(|| format!("{}: {}", directory.display(), names.join(", ")))
}

/// A path under this user's home, when something is there.
#[cfg(unix)]
fn under_home(relative: &str) -> Option<String> {
    let home = std::env::var_os("HOME")?;

    there(&Path::new(&home).join(relative))
}

/// The file, named, when it holds `needle`.
fn containing(path: &Path, needle: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;

    text.to_lowercase()
        .contains(&needle.to_lowercase())
        .then(|| format!("{} holds {needle}", path.display()))
}

/// The path, when something is there. `symlink_metadata`, so a dangling link counts as there.
fn there(path: &Path) -> Option<String> {
    std::fs::symlink_metadata(path)
        .is_ok()
        .then(|| path.display().to_string())
}

/// What a tool said, when it ran and answered something.
///
/// A tool that is not on this machine, or that exited non-zero because it found nothing, is
/// [`None`] — the same answer as "nothing of ours", and the right one: this asks whether
/// something is there, not whether the tool works.
#[cfg_attr(
    target_os = "linux",
    expect(
        dead_code,
        reason = "every Linux reading is a file; Windows and macOS are the callers"
    )
)]
fn said(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program).args(arguments).output().ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();

    (!text.is_empty()).then_some(text)
}
