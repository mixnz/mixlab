//! macOS: three files, compared against what the grant would write.
//!
//! **`/dev/pf` belongs to root**, so the daemon — which runs as the user — cannot ask the packet
//! filter whether it is enabled or what it has loaded. What it can read is the anchor, the block in
//! `/etc/pf.conf` and the boot job's plist, all of them world-readable. The probe is a byte
//! comparison against what a grant would put there.
//!
//! **That proves the configuration is in place; it does not prove pf is running right now.** The
//! plist is what makes that true at every boot, and the plist's presence is what is checked here.
//! The honest end-to-end check is a request to `127.0.0.1:80` reaching this home's front end, which
//! needs a front end that serves something — a site's, and `mix doctor`'s.

#[cfg(feature = "host")]
use std::path::Path;

#[cfg(feature = "host")]
use mixengine_proto::privileged::PortRedirect;

#[cfg(feature = "elevated")]
use mixengine_proto::privileged::{PortAccessPlan, PortAccessTarget};

#[cfg(any(feature = "host", feature = "elevated"))]
use crate::port_access::pf;
#[cfg(feature = "host")]
use crate::{PortAccess, PortAccessMethod, PortAccessState, PortBinding, Result};

/// This system's answer.
#[cfg(feature = "host")]
#[derive(Debug, Default)]
pub(crate) struct Ports;

#[cfg(feature = "host")]
impl PortAccess for Ports {
    /// The one system of the three that maps: 80 is answered by a program listening on 8080, and
    /// 443 by one listening on 8443. Everything else is itself.
    fn bindings(&self, answering: &[u16]) -> Vec<PortBinding> {
        answering
            .iter()
            .map(|&answer| PortBinding {
                answer,
                bind: target(answer),
            })
            .collect()
    }

    fn probe(&self, _binary: &Path, answering: &[u16]) -> Result<PortAccessState> {
        let bindings = self.bindings(answering);

        let redirects: Vec<PortRedirect> = bindings
            .iter()
            .filter(|binding| binding.answer != binding.bind)
            .map(|binding| PortRedirect {
                answer: binding.answer,
                bind: binding.bind,
            })
            .collect();

        if redirects.is_empty() {
            return Ok(PortAccessState {
                method: PortAccessMethod::Redirect,
                bindings,
                granted: true,
                missing: None,
            });
        }

        let mut absent = Vec::new();

        if text(pf::ANCHOR_FILE).as_deref() != Some(pf::anchor(&redirects).as_str()) {
            absent.push(pf::ANCHOR_FILE);
        }

        if !text(pf::CONF_FILE).map_or(Ok(false), |conf| pf::is_declared(&conf))? {
            absent.push(pf::CONF_FILE);
        }

        if text(pf::PLIST_FILE).as_deref() != Some(pf::plist().as_str()) {
            absent.push(pf::PLIST_FILE);
        }

        Ok(PortAccessState {
            method: PortAccessMethod::Redirect,
            bindings,
            granted: absent.is_empty(),
            missing: (!absent.is_empty()).then(|| {
                format!(
                    "{} {} not what MixEngine's packet-filter redirect needs",
                    absent.join(", "),
                    if absent.len() == 1 { "is" } else { "are" }
                )
            }),
        })
    }
}

/// The ordinary port a program binds to answer `answer` — D2's table, fixed.
///
/// A port that is not reserved answers itself: nothing has to move, so nothing does.
#[cfg(feature = "host")]
fn target(answer: u16) -> u16 {
    match answer {
        80 => 8080,
        443 => 8443,
        other => other,
    }
}

/// The contents of a file that may not be there. Absent and unreadable are the same answer here:
/// the grant is not in place.
#[cfg(feature = "host")]
fn text(path: &str) -> Option<String> {
    std::fs::read_to_string(Path::new(path)).ok()
}

/// Write the three artifacts a redirect is made of, then do now what the third does at boot — see
/// ADR 0012.
///
/// **The third is a boot job**, and it is what makes the other two mean anything: pf is disabled on
/// every boot and `pfctl -e` needs root, so a redirect that is only installed works until the first
/// reboot and then silently stops — leaving a front end answering on 8080 that nothing reaches on
/// 80.
///
/// **And the boot job runs at boot, which a machine that was granted the redirect this afternoon
/// has not done.** Three files on disk change nothing about the packet filter that is running:
/// until the next reboot pf stays off, the anchor stays unloaded, and `http://blog.test` finds
/// nothing on 80 while `mix doctor` — reading the same three files — reports the grant complete.
/// Measured on a machine booted at 01:05 and granted at 02:43. So the helper, already root, loads
/// the rules and enables pf itself, with the plist's own command and no other: `pfctl -f` then
/// `pfctl -e`, both fixed, neither taking a word from the request. It is the one step the plist
/// would take, taken once early.
///
/// # Errors
///
/// [`Error::UnsupportedPlatform`](crate::Error::UnsupportedPlatform) for a capability plan, which is
/// not this system's mechanism, [`Error::MalformedBlock`](crate::Error::MalformedBlock) for a
/// `/etc/pf.conf` somebody has half-edited, [`Error::Io`](crate::Error::Io) when a file cannot be
/// read or replaced, and [`Error::Os`](crate::Error::Os) when `pfctl` refuses the ruleset it was
/// just given or will not enable — with `pfctl`'s own words, because a rule it will not load is a
/// bug in [`pf::anchor`] and not a thing a user can fix.
#[cfg(feature = "elevated")]
pub(crate) fn apply(plan: &PortAccessPlan) -> crate::Result<crate::port_access::Change> {
    let PortAccessPlan::Redirect { redirects } = plan else {
        return Err(unsupported(
            "macOS reserves ports below 1024 and has no per-file capability; a redirect through \
             the packet filter is what this system grants",
        ));
    };

    let _held = crate::port_access::held()?;
    let mut changed = Vec::new();

    // Before the declaration that loads it: a `load anchor` naming a file that is not there is a
    // `/etc/pf.conf` `pfctl` refuses, and this order means no moment exists where that is true.
    if put(pf::ANCHOR_FILE, &pf::anchor(redirects))? {
        changed.push(pf::ANCHOR_FILE);
    }

    let conf = whole(pf::CONF_FILE);
    let declared = pf::declared(&conf)?;

    if declared != conf {
        crate::sys::replace::atomically(std::path::Path::new(pf::CONF_FILE), &declared)?;
        changed.push(pf::CONF_FILE);
    }

    if put(pf::PLIST_FILE, &pf::plist())? {
        changed.push(pf::PLIST_FILE);
    }

    // Whether pf was up is read *before* it is touched, so that a second call with the same plan on
    // a machine already redirecting is `Unchanged` — D4's whole-state promise — while the first call
    // on a machine that has never rebooted since the grant reports the switch it threw.
    let was_enabled = pfctl::enabled()?;
    pfctl::load()?;
    pfctl::enable()?;

    let mut detail = change(changed, "wrote");
    if !was_enabled {
        detail = also(detail, "enabled the packet filter");
    }

    Ok(detail)
}

/// Fold a second sentence into what [`apply`] reports.
#[cfg(feature = "elevated")]
fn also(change: crate::port_access::Change, more: &str) -> crate::port_access::Change {
    match change {
        crate::port_access::Change::Unchanged => crate::port_access::Change::Written {
            detail: more.to_owned(),
        },
        crate::port_access::Change::Written { detail } => crate::port_access::Change::Written {
            detail: format!("{detail}; {more}"),
        },
    }
}

/// Remove all three.
///
/// **`pfctl -d` is deliberately not run.** By then there is no way to know who else has come to
/// depend on pf being up, and pf enabled with none of our rules in it is not observably different
/// from pf disabled.
///
/// # Errors
///
/// As [`apply`].
#[cfg(feature = "elevated")]
pub(crate) fn revoke(target: &PortAccessTarget) -> crate::Result<crate::port_access::Change> {
    let PortAccessTarget::Redirect {} = target else {
        return Err(unsupported(
            "macOS has no per-file capability to take back; what it grants is a packet-filter \
             redirect",
        ));
    };

    let _held = crate::port_access::held()?;
    let mut changed = Vec::new();

    // The declaration first, this time: the reverse order, for the same reason.
    let conf = whole(pf::CONF_FILE);
    let undeclared = pf::undeclared(&conf)?;

    if undeclared != conf {
        crate::sys::replace::atomically(std::path::Path::new(pf::CONF_FILE), &undeclared)?;
        changed.push(pf::CONF_FILE);
    }

    for path in [pf::ANCHOR_FILE, pf::PLIST_FILE] {
        if remove(path)? {
            changed.push(path);
        }
    }

    // The running ruleset is reloaded from the file that no longer declares the anchor, so the
    // redirect stops now rather than at the next boot — otherwise 80 goes on being sent to a port
    // the front end this revoke was made for has left. pf itself is left as it was found; what is
    // taken out is the rules, not the switch.
    if !changed.is_empty() && pfctl::enabled()? {
        pfctl::load()?;
    }

    Ok(change(changed, "removed"))
}

/// `/sbin/pfctl`, three fixed invocations, none of them taking a word from anywhere.
///
/// Here rather than in `port_access/pf.rs`, which is compiled and tested on all three systems and
/// is text only: this is the part that is macOS and root.
#[cfg(feature = "elevated")]
mod pfctl {
    use std::process::{Command, Output, Stdio};

    use crate::port_access::pf;

    /// Apple's, on every macOS machine.
    const PFCTL: &str = "/sbin/pfctl";

    /// Whether pf is up. `pfctl -s info` opens with `Status: Enabled` or `Status: Disabled`, and
    /// nothing but root can ask — which is why the daemon's probe reads files instead.
    pub(super) fn enabled() -> crate::Result<bool> {
        let output = run(
            &["-s", "info"],
            "ask the packet filter whether it is enabled",
        )?;

        Ok(String::from_utf8_lossy(&output.stdout).contains("Status: Enabled"))
    }

    /// Load the main ruleset, anchor and all — the `-f` half of the boot job's command.
    ///
    /// A refusal here is the file the grant just wrote, in `pfctl`'s own words, and it is a bug in
    /// this crate's rendering rather than anything a user did.
    pub(super) fn load() -> crate::Result<()> {
        let output = run(&["-f", pf::CONF_FILE], "load the packet-filter rules")?;

        if output.status.success() {
            return Ok(());
        }

        Err(refused("load the packet-filter rules", &output))
    }

    /// Enable pf — the `-e` half.
    ///
    /// **Already enabled is success.** `pfctl -e` on a running pf exits non-zero saying `pf
    /// already enabled`, and a grant on a machine where a VPN or an earlier grant turned it on is
    /// exactly the case D4 calls unchanged rather than failed.
    pub(super) fn enable() -> crate::Result<()> {
        let output = run(&["-e"], "enable the packet filter")?;

        if output.status.success()
            || String::from_utf8_lossy(&output.stderr).contains("already enabled")
        {
            return Ok(());
        }

        Err(refused("enable the packet filter", &output))
    }

    fn run(arguments: &[&str], action: &'static str) -> crate::Result<Output> {
        Command::new(PFCTL)
            .args(arguments)
            .stdin(Stdio::null())
            .output()
            .map_err(|source| crate::Error::Os { action, source })
    }

    fn refused(action: &'static str, output: &Output) -> crate::Error {
        let said = String::from_utf8_lossy(&output.stderr);
        let said = said.trim();

        crate::Error::Os {
            action,
            source: std::io::Error::other(format!(
                "`pfctl` exited with {}{}{}",
                output.status,
                if said.is_empty() { "" } else { ": " },
                said
            )),
        }
    }
}

/// Replace `path` with `contents` when it does not already say that. Answers whether it wrote.
#[cfg(feature = "elevated")]
fn put(path: &str, contents: &str) -> crate::Result<bool> {
    if whole(path) == contents {
        return Ok(false);
    }

    crate::sys::replace::atomically(std::path::Path::new(path), contents)?;

    Ok(true)
}

/// Delete `path` if it is there. Answers whether it removed anything.
#[cfg(feature = "elevated")]
fn remove(path: &str) -> crate::Result<bool> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(crate::Error::Io {
            action: "remove",
            path: std::path::PathBuf::from(path),
            source,
        }),
    }
}

/// The contents of a file that may not be there, as the empty string when it is not.
///
/// A machine with no `/etc/pf.conf` is not one macOS ships, and is reachable — somebody who has
/// cleaned it up. It is created rather than refused, exactly as the hosts file is.
#[cfg(feature = "elevated")]
fn whole(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// [`Change`](crate::port_access::Change) from the list of paths that moved.
#[cfg(feature = "elevated")]
fn change(changed: Vec<&str>, verb: &str) -> crate::port_access::Change {
    if changed.is_empty() {
        return crate::port_access::Change::Unchanged;
    }

    crate::port_access::Change::Written {
        detail: format!("{verb} {}", changed.join(", ")),
    }
}

/// The refusal this system gives a plan that is not its mechanism.
#[cfg(feature = "elevated")]
fn unsupported(reason: &str) -> crate::Error {
    crate::Error::UnsupportedPlatform {
        capability: "PortAccess",
        reason: format!("{reason}; nothing was changed"),
    }
}
