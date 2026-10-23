//! What an operation says to the person approving it: [`PrivilegedOp::describe`] and the sentence
//! each kind of operation is described with.

use super::{
    FirewallPlan, HostEntry, PortAccessPlan, PortAccessTarget, PrivilegedOp, ResolverPlan,
    ResolverTarget, TrustPlan, TrustTarget,
};

impl PrivilegedOp {
    /// What this operation will literally change, for a person about to allow it.
    ///
    /// **Derived from the operation rather than stored beside it** — the T40b design, D7. The
    /// alternative is a `summary` written by whoever enqueued the operation and kept in its row,
    /// which is a description that can disagree with what will be applied and would preserve that
    /// disagreement across a restart, on the one screen whose whole job is to tell the truth before
    /// somebody clicks Allow.
    ///
    /// [`String`] and not `&'static str`: the operations that matter carry data — `HostsApply`'s
    /// description is its domains (T41) — and a constant would be a shape the next operation has to
    /// break immediately.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Probe {} => "report the installed helper's version, whether it holds an \
                               administrative token, and where it writes its audit log"
                .to_owned(),
            Self::HostsApply { home, entries } => describe_hosts(home.as_deref(), entries),
            Self::PortAccessGrant { plan } => describe_grant(plan),
            Self::PortAccessRevoke { target } => describe_revoke(target),
            Self::ResolverApply { plan } => describe_resolver(plan),
            Self::ResolverRevoke { target } => describe_unwire(target),
            Self::TrustCaInstall { plan } => describe_trust(plan),
            Self::TrustCaRemove { target } => describe_untrust(target),
            Self::FirewallApply { plan } => describe_firewall(plan),
            // No path in the sentence, because this layer has none: which directory an OS keeps a
            // privileged helper in is `mixengine_platform::install`'s answer, and `mixengine-proto`
            // takes no platform dependency. What a person needs before clicking Allow is what
            // changes and why, and both are here.
            Self::HelperInstall {} => "install MixEngine's privileged helper in a directory only \
                                       administrators can write, so that every later prompt runs a \
                                       copy nothing running as you can replace"
                .to_owned(),
            // What makes this one safe is not the account it came from, so the sentence says what
            // it is instead — the screen this appears on is the one whose whole job is to tell the
            // truth before somebody clicks Allow.
            Self::HelperReplace {} => "replace MixEngine's privileged helper with the newer one \
                                       MixEngine has downloaded, after the helper already \
                                       installed on this machine has checked that its signature is \
                                       MixEngine's and that it is not an older release"
                .to_owned(),
            // No path in either sentence either, and for the reason written above them.
            Self::HelperRemove {} => {
                "remove MixEngine's privileged helper from the directory only \
                                      administrators can write, leaving nothing of it on this \
                                      machine"
                    .to_owned()
            }
            Self::PackageRemove {} => "remove the program the macOS package installed: the \
                                       command-line tools, the MixLab application and the \
                                       package receipt"
                .to_owned(),
            Self::AuditLogRemove {} => {
                "remove the root-owned log of everything MixEngine has ever \
                                        done as an administrator, and the directory holding it"
                    .to_owned()
            }
        }
    }
}

/// What a hosts change will literally do, for a person about to allow it.
///
/// The addresses are named when they differ, because the helper permits `::1` as well as
/// `127.0.0.1` (D5) and a description that hid the difference would be describing something else.
fn describe_hosts(home: Option<&str>, entries: &[HostEntry]) -> String {
    let Some(first) = entries.first() else {
        return match home {
            Some(_) => "remove MixEngine's block from the hosts file".to_owned(),
            None => "remove the block MixEngine wrote in the hosts file before each home kept its \
                     own"
            .to_owned(),
        };
    };

    let uniform = entries.iter().all(|entry| entry.address == first.address);
    let plural = if entries.len() == 1 { "" } else { "s" };

    let names: Vec<String> = entries
        .iter()
        .map(|entry| {
            if uniform {
                entry.domain.clone()
            } else {
                format!("{} ({})", entry.domain, entry.address)
            }
        })
        .collect();

    let at = if uniform {
        first.address.to_string()
    } else {
        "loopback".to_owned()
    };

    format!(
        "point {} name{plural} at {at} in the hosts file: {}",
        entries.len(),
        names.join(", ")
    )
}

/// What a port-access grant will literally do, for a person about to allow it.
///
/// The binary's whole path, never its file name: T42's D11 leaves exactly one control against a
/// compromised daemon pointing the grant at a program of its own choosing, and this is it.
fn describe_grant(plan: &PortAccessPlan) -> String {
    match plan {
        PortAccessPlan::Capability { binary, ports } => format!(
            "let {} bind port{} {} without an administrator, by giving that file the \
             cap_net_bind_service capability",
            binary.display(),
            if ports.len() == 1 { "" } else { "s" },
            list(ports)
        ),
        PortAccessPlan::Redirect { redirects } => format!(
            "send {} on 127.0.0.1 to a port an ordinary program may bind, through a packet-filter \
             anchor, a block in /etc/pf.conf and a boot-time job that enables the packet filter: {}",
            if redirects.len() == 1 {
                "one port"
            } else {
                "two ports"
            },
            redirects
                .iter()
                .map(|redirect| format!("{} to {}", redirect.answer, redirect.bind))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// What taking it away will literally do.
fn describe_revoke(target: &PortAccessTarget) -> String {
    match target {
        PortAccessTarget::Capability { binary } => format!(
            "take the cap_net_bind_service capability back off {}",
            binary.display()
        ),
        PortAccessTarget::Redirect {} => "remove MixEngine's packet-filter anchor, its block in \
                                          /etc/pf.conf and its boot-time job"
            .to_owned(),
    }
}

/// What wiring a resolver will literally do, for a person about to allow it.
///
/// Names the files or the rule, because "wire the resolver" is not a sentence anybody can consent
/// to. The port is named where the mechanism can carry one and omitted where it cannot.
fn describe_resolver(plan: &ResolverPlan) -> String {
    match plan {
        ResolverPlan::ResolverDirectory { tlds, port } => format!(
            "send {} to MixEngine's DNS server on 127.0.0.1:{port}, by writing {}",
            patterns(tlds),
            tlds.iter()
                .map(|tld| format!("/etc/resolver/{tld}"))
                .collect::<Vec<_>>()
                .join(" and ")
        ),
        ResolverPlan::SystemdLink { tlds, port } => format!(
            "send {} to MixEngine's DNS server on 127.0.0.1:{port}, by adding a network link \
             called mixengine0 in /etc/systemd/network and reloading systemd-networkd",
            patterns(tlds)
        ),
        ResolverPlan::Nrpt { tlds } => format!(
            "send {} to MixEngine's DNS server on 127.0.0.1, by adding one Name Resolution Policy \
             rule to this machine",
            patterns(tlds)
        ),
    }
}

/// What unwiring will literally do.
fn describe_unwire(target: &ResolverTarget) -> String {
    match target {
        ResolverTarget::ResolverDirectory { port } => format!(
            "remove the resolver files MixEngine wrote in /etc/resolver that point at 127.0.0.1:{port}"
        ),
        ResolverTarget::SystemdLink { port } => format!(
            "remove MixEngine's network link and its two files in /etc/systemd/network, while they \
             point at 127.0.0.1:{port}"
        ),
        ResolverTarget::Nrpt {} => {
            "remove MixEngine's Name Resolution Policy rule from this machine".to_owned()
        }
    }
}

/// What trusting MixEngine's authority will literally do, for a person about to allow it.
///
/// **The store is named**, because "trust a certificate" and "add a root to this machine's own
/// store, for every account on it" are different sentences and only the second one is true. Nothing
/// is said about the certificate's contents: this screen is read before the helper has checked
/// them, so any claim about them here would be the daemon's word rather than a fact.
fn describe_trust(plan: &TrustPlan) -> String {
    let store = match plan {
        TrustPlan::SystemRoot { .. } => "this machine's Trusted Root Certification Authorities",
        TrustPlan::SystemKeychain { .. } => "this machine's System keychain",
        TrustPlan::CaCertificates { .. } => "/usr/local/share/ca-certificates",
        TrustPlan::CaTrustAnchors { .. } => "/etc/pki/ca-trust/source/anchors",
    };

    format!(
        "add MixEngine's own certificate authority to {store}, so this machine trusts the \
         certificates MixEngine issues for local sites"
    )
}

/// And what removing it will do. The authority is named, because a machine may hold more than one.
fn describe_untrust(target: &TrustTarget) -> String {
    let (store, key_id) = match target {
        TrustTarget::SystemRoot { key_id } => (
            "this machine's Trusted Root Certification Authorities",
            key_id,
        ),
        TrustTarget::SystemKeychain { key_id } => ("this machine's System keychain", key_id),
        TrustTarget::CaCertificates { key_id } => ("/usr/local/share/ca-certificates", key_id),
        TrustTarget::CaTrustAnchors { key_id } => ("/etc/pki/ca-trust/source/anchors", key_id),
    };

    format!("remove MixEngine's certificate authority {key_id} from {store}")
}

/// What a firewall change will literally do, for a person about to allow it.
///
/// The ports, in the order they will be written, and the name the rules will carry — which is what
/// a person needs to go and find them afterwards. An empty plan says the rules are going away,
/// because that is what an empty whole state means.
fn describe_firewall(plan: &FirewallPlan) -> String {
    if plan.ports.is_empty() {
        return format!("remove the firewall rules named \"{}\"", plan.label);
    }

    let ports = plan
        .ports
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "let this machine's local network reach TCP {ports}, as a firewall rule named \"{}\"",
        plan.label
    )
}

/// `*.test, *.internal`, which is what a wildcard route reads as in a sentence.
fn patterns(tlds: &[String]) -> String {
    tlds.iter()
        .map(|tld| format!("*.{tld}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `80 and 443`, the way a sentence names a short list.
fn list(ports: &[u16]) -> String {
    let rendered: Vec<String> = ports.iter().map(u16::to_string).collect();

    match rendered.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}
