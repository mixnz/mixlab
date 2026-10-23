//! Linux: a `systemd-networkd` link of MixEngine's own.
//!
//! **This is the mechanism left after the measurement removed every one the feature spec and the
//! roadmap named** — the T45 design, D10:
//!
//! - a `/etc/systemd/resolved.conf.d/` drop-in with `DNS=127.0.0.1:53535` and `Domains=~test`
//!   redirects the **whole machine**: after it, `getent hosts github.com` answered `127.0.0.1`. A
//!   global routing domain does not scope the global DNS servers;
//! - `resolvectl dns lo …` is refused by name — *"Link lo is loopback device"* — and so is
//!   `resolvectl domain lo` and `resolvectl revert lo`;
//! - a real link would have its own servers **replaced** rather than added to, which is what
//!   `resolvectl dns <link>` does and what the roadmap already warned about;
//! - and a dummy link with **no address** is accepted, reports its servers back through
//!   `resolvectl status`, and never gets a DNS scope at all — so nothing is ever sent to it. That
//!   is the worst of the four, because everything about it reads as applied.
//!
//! So: a dummy link carrying a link-local `/32`, declared in two files so it survives a reboot with
//! no standing process of ours.
//!
//! **And unlike the other two systems, the mechanism here is a question about the machine rather
//! than about the platform** — D2. A Linux with no `systemd-resolved` and no `systemd-networkd` has
//! no scoped mechanism, which is [`ResolverMethod::None`] and a mode rather than a failure.

use crate::resolver::networkd;

#[cfg(feature = "host")]
use crate::{ResolverConfig, ResolverMethod, ResolverState, Result};

/// Where `systemd-resolved` puts its runtime state. Its presence is how this module asks whether
/// the service is running without starting a process to find out.
#[cfg(feature = "host")]
const RESOLVED_RUNTIME: &str = "/run/systemd/resolve";

/// The same question for `systemd-networkd`.
#[cfg(feature = "host")]
const NETWORKD_RUNTIME: &str = "/run/systemd/netif";

/// What glibc reads to choose a source for a name, and what it reads when that source is `dns`.
#[cfg(feature = "host")]
const NSSWITCH_CONF: &str = "/etc/nsswitch.conf";
#[cfg(feature = "host")]
const RESOLV_CONF: &str = "/etc/resolv.conf";

/// `systemd-resolved`'s stub listener.
#[cfg(feature = "host")]
const STUB: std::net::IpAddr = std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 53));

/// One directory per network link the kernel has, which is how a revoke asks whether the link is
/// still there.
#[cfg(feature = "elevated")]
const SYS_CLASS_NET: &str = "/sys/class/net";

/// This system's answer.
#[cfg(feature = "host")]
#[derive(Debug, Default)]
pub(crate) struct Resolver;

#[cfg(feature = "host")]
impl ResolverConfig for Resolver {
    fn method(&self) -> Result<ResolverMethod> {
        Ok(mechanism(
            std::path::Path::new(RESOLVED_RUNTIME).exists(),
            std::path::Path::new(NETWORKD_RUNTIME).exists(),
        ))
    }

    fn probe(&self, tlds: &[&str], port: u16) -> Result<ResolverState> {
        let method = self.method()?;

        if method == ResolverMethod::None {
            return Ok(ResolverState {
                method,
                wired: Vec::new(),
                missing: Some(
                    "this machine runs neither systemd-resolved nor systemd-networkd, so there is \
                     no way to send one TLD to a nameserver without changing every name it \
                     resolves"
                        .to_owned(),
                ),
                bypassed: None,
                declared: Vec::new(),
            });
        }

        let file = std::fs::read_to_string(networkd::NETWORK_PATH).unwrap_or_default();
        let declared = wired_in(&file, tlds, port);

        let read = |path| std::fs::read_to_string(path).ok();
        if let Some(reason) = bypassed(read(NSSWITCH_CONF).as_deref(), read(RESOLV_CONF).as_deref())
        {
            return Ok(ResolverState {
                method,
                wired: Vec::new(),
                missing: Some(reason.clone()),
                bypassed: Some(reason),
                declared,
            });
        }

        let missing = (declared.len() < tlds.len()).then(|| {
            format!(
                "{} does not send these names to 127.0.0.1:{port}",
                networkd::NETWORK_PATH
            )
        });

        Ok(ResolverState {
            method,
            wired: declared.clone(),
            missing,
            bypassed: None,
            declared,
        })
    }
}

/// Which mechanism a machine with these two services has.
///
/// **Both, or neither.** `systemd-resolved` is what routes a domain to a nameserver and
/// `systemd-networkd` is what brings up the link the routing is attached to; a machine with one of
/// them has no way to do this that does not change every name it resolves.
#[cfg(feature = "host")]
fn mechanism(resolved: bool, networkd_running: bool) -> ResolverMethod {
    if resolved && networkd_running {
        ResolverMethod::SystemdLink
    } else {
        ResolverMethod::None
    }
}

/// Why programs on this machine never reach `systemd-resolved`, or [`None`] when they do.
///
/// **A link routes only what is asked of resolved**, and glibc asks it one of two ways: through
/// `nss-resolve`, when `resolve` comes before `dns` on the hosts line of `/etc/nsswitch.conf`, or
/// through the stub, when `/etc/resolv.conf` names `127.0.0.53` first. On the server M34 ran on,
/// AdGuard Home had turned the stub off and resolved had written its upstreams into `resolv.conf`,
/// so `resolvectl query` answered through the link while `getent` never reached it.
///
/// `127.0.0.54`, the proxy stub, is not counted: whether it applies per-link routing has not been
/// measured. A file that cannot be read says nothing for resolved.
#[cfg(feature = "host")]
fn bypassed(nsswitch: Option<&str>, resolv: Option<&str>) -> Option<String> {
    if nsswitch.is_some_and(nss_asks_resolved)
        || resolv
            .and_then(first_nameserver)
            .is_some_and(|server| server == STUB)
    {
        return None;
    }

    let resolv = match resolv.map(first_nameserver) {
        Some(Some(server)) => format!("{RESOLV_CONF} names {server} rather than {STUB}"),
        Some(None) => format!("{RESOLV_CONF} names no nameserver"),
        None => format!("{RESOLV_CONF} could not be read"),
    };
    let nsswitch = match nsswitch {
        Some(_) => format!("the hosts line of {NSSWITCH_CONF} has no `resolve` before `dns`"),
        None => format!("{NSSWITCH_CONF} could not be read"),
    };

    Some(format!(
        "programs on this machine do not ask systemd-resolved: {resolv}, and {nsswitch}"
    ))
}

/// Whether the hosts line asks `resolve` before `dns`; a `[...]` action is not a source.
#[cfg(feature = "host")]
fn nss_asks_resolved(nsswitch: &str) -> bool {
    let Some(sources) = lines(nsswitch, '#').find_map(|line| line.strip_prefix("hosts:")) else {
        return false;
    };

    sources
        .split_whitespace()
        .filter(|source| !source.starts_with('['))
        .find(|source| *source == "resolve" || *source == "dns")
        == Some("resolve")
}

/// The first `nameserver` in a `resolv.conf`, which is the one glibc asks first.
#[cfg(feature = "host")]
fn first_nameserver(resolv: &str) -> Option<std::net::IpAddr> {
    lines(resolv, '#')
        .filter_map(|line| line.split(';').next())
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next() == Some("nameserver"))
                .then(|| fields.next()?.parse().ok())
                .flatten()
        })
}

/// A file's lines with comments cut off and blanks dropped.
#[cfg(feature = "host")]
fn lines(contents: &str, comment: char) -> impl Iterator<Item = &str> {
    contents
        .lines()
        .filter_map(move |line| line.split(comment).next())
        .map(str::trim)
        .filter(|line| !line.is_empty())
}

/// Which of `tlds` a `.network` file routes to this home's server on `port`.
///
/// **The port has to match.** A file another home wrote names another port, and counting it would
/// report this home as wired while none of its names resolve.
#[cfg(feature = "host")]
fn wired_in(contents: &str, tlds: &[&str], port: u16) -> Vec<String> {
    let (domains, declared) = networkd::declared(contents);

    if declared != Some(port) {
        return Vec::new();
    }

    tlds.iter()
        .filter(|tld| domains.iter().any(|domain| domain == *tld))
        .map(|tld| (*tld).to_owned())
        .collect()
}

/// Declare the link and its routing domains, then ask `systemd-networkd` to bring it up.
///
/// # Errors
///
/// [`Error::UnsupportedPlatform`](crate::Error::UnsupportedPlatform) for another system's plan,
/// [`Error::Io`](crate::Error::Io) when a file cannot be written, and
/// [`Error::Os`](crate::Error::Os) when the reload could not be run or refused.
#[cfg(feature = "elevated")]
pub(crate) fn apply(
    plan: &mixengine_proto::privileged::ResolverPlan,
) -> crate::Result<crate::resolver::Change> {
    use mixengine_proto::privileged::ResolverPlan;

    let ResolverPlan::SystemdLink { tlds, port } = plan else {
        return Err(unsupported(
            "Linux routes a TLD with a systemd-networkd link of MixEngine's own; this plan is \
             another system's mechanism",
        ));
    };

    // After the mechanism check and never before it — see `crate::resolver::held`.
    let _held = crate::resolver::held()?;

    let netdev = networkd::netdev();
    let network = networkd::network(tlds, *port);

    let mut changed = false;
    changed |= put(networkd::NETDEV_PATH, &netdev)?;
    changed |= put(networkd::NETWORK_PATH, &network)?;

    if !changed {
        return Ok(crate::resolver::Change::Unchanged);
    }

    reload()?;

    Ok(crate::resolver::Change::Written {
        detail: format!(
            "declared the {} link in {} and {}, sending {} to 127.0.0.1:{port}",
            networkd::LINK,
            networkd::NETDEV_PATH,
            networkd::NETWORK_PATH,
            tlds.join(", ")
        ),
    })
}

/// Take both files away, then the link they brought up.
///
/// **The link is deleted by name, after the reload.** Removing the `.netdev` and restarting
/// `systemd-networkd` leaves the dummy link up until the next boot: networkd creates a netdev from
/// its file and never deletes one whose file went. Deleting it before the reload would let a file
/// still on disk bring it back.
///
/// # Errors
///
/// As [`apply`].
#[cfg(feature = "elevated")]
pub(crate) fn revoke(
    target: &mixengine_proto::privileged::ResolverTarget,
) -> crate::Result<crate::resolver::Change> {
    use mixengine_proto::privileged::ResolverTarget;

    let ResolverTarget::SystemdLink { port } = target else {
        return Err(unsupported(
            "Linux routes a TLD with a systemd-networkd link of MixEngine's own; this target is \
             another system's mechanism",
        ));
    };

    let _held = crate::resolver::held()?;

    // a link pointing at another home's server is that home's. A file that is not there is
    // read as empty, and an empty file points nowhere — which leaves the `.netdev` alone with it,
    // as a revoke of a half-written link from before this rule would not.
    let declared = std::fs::read_to_string(networkd::NETWORK_PATH).unwrap_or_default();
    if !declared.is_empty() && !networkd::points_at(&declared, *port) {
        return Ok(crate::resolver::Change::Unchanged);
    }

    let mut files = false;
    files |= remove(networkd::NETDEV_PATH)?;
    files |= remove(networkd::NETWORK_PATH)?;

    if files {
        reload()?;
    }

    // Asked of the kernel rather than read from `networkctl`'s exit status, which is 1 both for a
    // link that is not there and for a delete that failed. A link with no files is one an earlier
    // helper left behind, and nothing else would ever take it away.
    let link = std::path::Path::new(SYS_CLASS_NET)
        .join(networkd::LINK)
        .exists();
    if link {
        delete_link()?;
    }

    Ok(revoked(files, link))
}

/// What a revoke that removed the files, the link, both or neither says it did.
#[cfg(feature = "elevated")]
fn revoked(files: bool, link: bool) -> crate::resolver::Change {
    let detail = match (files, link) {
        (false, false) => return crate::resolver::Change::Unchanged,
        (true, true) => format!(
            "removed the {} link and its two files under /etc/systemd/network",
            networkd::LINK
        ),
        (true, false) => format!(
            "removed the two files under /etc/systemd/network; the {} link was already gone",
            networkd::LINK
        ),
        (false, true) => format!(
            "removed the {} link, whose files under /etc/systemd/network were already gone",
            networkd::LINK
        ),
    };

    crate::resolver::Change::Written { detail }
}

/// Write `contents` to `path` unless it already says exactly that. Answers whether it wrote.
#[cfg(feature = "elevated")]
fn put(path: &str, contents: &str) -> crate::Result<bool> {
    let path = std::path::Path::new(path);

    if std::fs::read_to_string(path).ok().as_deref() == Some(contents) {
        return Ok(false);
    }

    crate::sys::replace::atomically(path, contents)?;

    Ok(true)
}

/// Remove `path` if it is there. Answers whether it removed anything.
#[cfg(feature = "elevated")]
fn remove(path: &str) -> crate::Result<bool> {
    let path = std::path::Path::new(path);

    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(crate::Error::Io {
            action: "remove",
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Ask `systemd-networkd` to read the files that were just written.
///
/// **One fixed command with no argument from the request** — the shape T42 established with
/// `pfctl`. The rule this binary keeps is that it never runs an *arbitrary* command; a constant
/// argument vector is not one, and there is no API for this that does not go through systemd's own
/// tool.
///
/// `networkctl reload` would be lighter and **was not measured**, so it is not used: what the
/// probe demonstrated bringing the link up was a restart, and a mechanism nobody has watched work
/// is not one to ship on the strength of its documentation.
#[cfg(feature = "elevated")]
fn reload() -> crate::Result<()> {
    run(
        ["systemctl", "restart", "systemd-networkd"],
        "run systemctl to reload systemd-networkd",
        "reload systemd-networkd",
    )
}

/// Delete the dummy link, which removing its file does not — measured on systemd 255: `networkctl
/// delete mixengine0` exits 0 and the link is gone, and against a link that is not there exits 1
/// with *"No such device"*. The same fixed-command shape as [`reload`].
#[cfg(feature = "elevated")]
fn delete_link() -> crate::Result<()> {
    run(
        ["networkctl", "delete", networkd::LINK],
        "run networkctl to delete MixEngine's link",
        "delete MixEngine's link",
    )
}

/// Run one constant argument vector, reading a non-zero exit as `failed` with its stderr.
#[cfg(feature = "elevated")]
fn run(command: [&str; 3], start: &'static str, failed: &'static str) -> crate::Result<()> {
    let output = std::process::Command::new(command[0])
        .args(&command[1..])
        .output()
        .map_err(|source| crate::Error::Os {
            action: start,
            source,
        })?;

    if !output.status.success() {
        return Err(crate::Error::Os {
            action: failed,
            source: std::io::Error::other(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ),
        });
    }

    Ok(())
}

/// A plan or target that is not this system's mechanism.
#[cfg(feature = "elevated")]
fn unsupported(reason: &str) -> crate::Error {
    crate::Error::UnsupportedPlatform {
        capability: "ResolverConfig",
        reason: reason.to_owned(),
    }
}

#[cfg(all(test, feature = "host"))]
mod tests {
    use super::*;

    /// D2, and the whole reason [`ResolverConfig::method`] returns a `Result` here where the other
    /// two systems return a constant: on this system the mechanism depends on what is running, and
    /// a machine with neither service is a mode rather than a failure.
    #[test]
    fn a_machine_without_both_services_has_no_mechanism() {
        assert_eq!(mechanism(false, false), ResolverMethod::None);
        assert_eq!(mechanism(true, false), ResolverMethod::None);
        assert_eq!(mechanism(false, true), ResolverMethod::None);
        assert_eq!(mechanism(true, true), ResolverMethod::SystemdLink);
    }

    /// The probe reads the file back, so a wiring that drifted is not reported as present.
    #[test]
    fn a_file_that_matches_reports_its_tlds_wired() {
        let contents = networkd::network(&["test".to_owned()], 53_535);

        assert_eq!(
            wired_in(&contents, &["test", "internal"], 53_535),
            vec!["test".to_owned()]
        );
    }

    /// A file naming another port belongs to another home's server.
    #[test]
    fn a_file_naming_another_port_is_not_wired() {
        let contents = networkd::network(&["test".to_owned()], 60_000);

        assert!(wired_in(&contents, &["test"], 53_535).is_empty());
    }

    /// No file at all is every machine before the first grant, and is not an error.
    #[test]
    fn nothing_declared_is_nothing_wired() {
        assert!(wired_in("", &["test"], 53_535).is_empty());
    }

    /// The server M34 ran on: AdGuard Home took port 53 with `DNSStubListener=no`, so resolved
    /// wrote its upstreams into the file glibc reads, and the hosts line has no `resolve`.
    const ADGUARD_RESOLV: &str = "# This is /run/systemd/resolve/resolv.conf managed by \
                                  man:systemd-resolved(8).\n# Do not edit.\n#\n\
                                  nameserver 127.0.0.1\nnameserver 1.1.1.1\nnameserver 8.8.8.8\n\
                                  search .\n";
    const PLAIN_NSSWITCH: &str = "passwd: files systemd\nhosts:          files dns\n\
                                  networks:       files\n";

    /// Ubuntu's own: the stub is on, and the hosts line asks it through `dns`.
    const STUB_RESOLV: &str = "nameserver 127.0.0.53\noptions edns0 trust-ad\nsearch .\n";
    const DESKTOP_NSSWITCH: &str = "hosts: files mdns4_minimal [NOTFOUND=return] dns\n";

    /// What `libnss-resolve` writes: glibc asks resolved over its socket before `dns` is reached.
    const NSS_RESOLVE: &str = "hosts: files resolve [!UNAVAIL=return] dns myhostname\n";

    #[test]
    fn a_machine_whose_programs_skip_resolved_is_bypassed() {
        let why = bypassed(Some(PLAIN_NSSWITCH), Some(ADGUARD_RESOLV)).expect("bypassed");

        assert!(why.contains("/etc/resolv.conf names 127.0.0.1"), "{why}");
        assert!(why.contains("/etc/nsswitch.conf"), "{why}");
    }

    #[test]
    fn the_stub_first_in_resolv_conf_is_asked() {
        assert_eq!(bypassed(Some(DESKTOP_NSSWITCH), Some(STUB_RESOLV)), None);
        assert_eq!(bypassed(Some(PLAIN_NSSWITCH), Some(STUB_RESOLV)), None);
    }

    #[test]
    fn nss_resolve_before_dns_is_asked_whatever_resolv_conf_says() {
        assert_eq!(bypassed(Some(NSS_RESOLVE), Some(ADGUARD_RESOLV)), None);
        assert_eq!(bypassed(Some("hosts: files resolve\n"), None), None);
    }

    /// glibc asks `dns` first and stops at its answer, so a `resolve` after it is never reached
    /// for a name the upstream answers.
    #[test]
    fn resolve_after_dns_is_not_asked() {
        assert!(bypassed(Some("hosts: files dns resolve\n"), Some(ADGUARD_RESOLV)).is_some());
    }

    /// The stub second is asked only when the first server fails, which a working upstream never
    /// does.
    #[test]
    fn the_stub_after_another_server_is_not_asked() {
        let resolv = "nameserver 1.1.1.1\nnameserver 127.0.0.53\n";

        assert!(bypassed(Some(PLAIN_NSSWITCH), Some(resolv)).is_some());
    }

    #[test]
    fn comments_are_not_lines() {
        let nsswitch = "# hosts: files resolve\nhosts: files dns # resolve is not here\n";
        let resolv = "# nameserver 127.0.0.53\nnameserver 192.168.1.1\n";

        assert!(bypassed(Some(nsswitch), Some(resolv)).is_some());
    }

    /// The proxy stub is not counted: whether it applies per-link routing was not measured.
    #[test]
    fn the_proxy_stub_is_not_the_stub() {
        assert!(bypassed(Some(PLAIN_NSSWITCH), Some("nameserver 127.0.0.54\n")).is_some());
    }

    #[test]
    fn two_unreadable_files_say_so() {
        let why = bypassed(None, None).expect("nothing says resolved is asked");

        assert!(why.contains("could not be read"), "{why}");
    }
}

#[cfg(all(test, feature = "elevated"))]
mod revoke_tests {
    use super::*;
    use crate::resolver::Change;

    /// The detail says what went, so a revoke that left the link up cannot read as one that
    /// took it.
    #[test]
    fn a_revoke_says_whether_the_link_went_with_its_files() {
        let Change::Written { detail } = revoked(true, true) else {
            panic!("files and link removed is a change");
        };
        assert!(
            detail.contains("mixengine0 link and its two files"),
            "{detail}"
        );

        let Change::Written { detail } = revoked(true, false) else {
            panic!("files removed is a change");
        };
        assert!(detail.contains("already gone"), "{detail}");
    }

    /// A link an earlier helper left with no files is still this revoke's to take.
    #[test]
    fn a_link_with_no_files_is_still_removed() {
        let Change::Written { detail } = revoked(false, true) else {
            panic!("a leftover link removed is a change");
        };
        assert!(detail.contains("files"), "{detail}");
        assert!(detail.contains("were already gone"), "{detail}");
    }

    #[test]
    fn nothing_there_is_unchanged() {
        assert!(matches!(revoked(false, false), Change::Unchanged));
    }
}
