//! The file protocol between `mixengined` and `mixengine-elevate`.
//!
//! The daemon writes a [`PrivilegedRequest`] into a fresh single-use directory, raises the OS
//! elevation prompt on the helper with that file's path as its one argument, and reads the
//! [`PrivilegedResponse`] the helper leaves beside it. See
//! `docs/decisions/0005-on-demand-elevation.md` and the T40 design for why it is files and not a
//! socket: the helper has no listener, no idle state, and exists for seconds.
//!
//! **The response file is the protocol.** When it is there, it is the answer and the exit code says
//! nothing; the exit code matters only when there is no file. That is not a preference — the macOS
//! launcher raises an AppleScript error instead of handing back a status, so an outcome encoded as a
//! number is an outcome one of the three systems has to reconstruct from an error string.
//!
//! **What is denied and what is tolerated runs in opposite directions on purpose.** The request and
//! every operation in it use `deny_unknown_fields`: a helper that silently ignored a field inside an
//! operation it thought it understood would apply a weaker version of that operation and tell nobody.
//! The response does not: the helper is excluded from auto-update, so a helper newer than the daemon
//! reading it is routine, and a field it added must not make its answer unreadable.

use std::net::IpAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ProtocolVersion;

mod describe;

/// The name the response takes, beside the request it answers.
///
/// Not passed as an argument: one fewer argument is one fewer thing the elevated process has to
/// validate, and the daemon already knows where it will be. Its **existence** is also the whole of
/// the anti-replay check — a request with an answer beside it has been processed and is refused.
pub const RESPONSE_FILE_NAME: &str = "response.json";

/// Where a replacement helper waits for the prompt that installs it — roadmap task **T88a**.
///
/// **Under `run/` and not `cache/`**, because `Paths::new` builds `run` with no `[paths]` override:
/// the elevated process composes this path from a compiled-in constant, and a directory a config
/// file could move is a directory somebody else could choose. It is also where `run/elevate/`
/// already is, which is the right neighbourhood — a staged candidate is exactly as durable as the
/// pending row that will apply it.
const HELPER_CANDIDATE_DIR: &str = "helper";

/// The directory [`helper_candidate`] and [`helper_candidate_signature`] live in.
#[must_use]
pub fn helper_candidate_dir(home: &std::path::Path) -> PathBuf {
    home.join("run").join(HELPER_CANDIDATE_DIR)
}

/// The replacement binary [`PrivilegedOp::HelperReplace`] looks for.
///
/// One function rather than a constant each side joins for itself: the daemon writes this file and
/// the elevated process reads it, and two spellings that agree today are two spellings.
#[must_use]
pub fn helper_candidate(home: &std::path::Path) -> PathBuf {
    helper_candidate_dir(home).join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX))
}

/// Its detached minisign signature, named the way minisign names one.
#[must_use]
pub fn helper_candidate_signature(home: &std::path::Path) -> PathBuf {
    helper_candidate_dir(home).join(format!(
        "mixengine-elevate{}.minisig",
        std::env::consts::EXE_SUFFIX
    ))
}

/// The helper's own version — roadmap task **T182b**, D1.
///
/// **Not the product's.** Two releases' helpers would otherwise always differ, and keeping the
/// installed helper in step would cost a prompt at every update. `crates/mixengine-elevate/helper.lock`
/// decides when this changes, and `packaging/helper-lock.sh --bump` is what changes it.
pub const HELPER_VERSION: &str = "0.1.8";

/// What a candidate helper's *signed* trusted comment says it is — roadmap task **T88a**.
///
/// **The only fact about a candidate that a compromised daemon cannot write.** minisign's global
/// signature covers the trusted comment, and `minisign-verify` hands it over only after the
/// signature has verified, so this is where "which version, for which machine" can travel without
/// being taken on trust.
///
/// Parsed here rather than in either crate that checks a signature, so that the daemon's pre-check
/// and the elevated check read one grammar with one set of tests behind it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HelperStamp {
    /// The version the release published this helper as.
    pub version: String,

    /// `windows`, `macos` or `linux`, as the update feed spells them.
    pub os: String,

    /// `x86_64`, `aarch64`, or `universal` for the one macOS builds.
    pub arch: String,
}

impl HelperStamp {
    /// The first word of the comment, which is what says the signature is over a *helper*.
    ///
    /// Without it, any artifact this project signs — an installer, the feed — would parse as a
    /// stamp for whatever its second word happened to be.
    pub const LABEL: &'static str = "mixengine-elevate";

    /// Read `mixengine-elevate <version> <os> <arch>`, or nothing.
    ///
    /// Exactly four words: a comment with a fifth is not this grammar, and reading the first four
    /// out of something longer is how a value nobody wrote gets believed.
    #[must_use]
    pub fn parse(trusted_comment: &str) -> Option<Self> {
        let mut words = trusted_comment.split_whitespace();

        let (Some(Self::LABEL), Some(version), Some(os), Some(arch), None) = (
            words.next(),
            words.next(),
            words.next(),
            words.next(),
            words.next(),
        ) else {
            return None;
        };

        Some(Self {
            version: version.to_owned(),
            os: os.to_owned(),
            arch: arch.to_owned(),
        })
    }

    /// This operating system, as the update feed spells it.
    ///
    /// A wrapper over a constant rather than the constant itself, so the *name* of the question
    /// appears at every call site — `std::env::consts::OS` beside a signature check reads as an
    /// incidental detail, and it is not one.
    #[must_use]
    pub fn host_os() -> &'static str {
        std::env::consts::OS
    }

    /// This architecture, as the update feed spells it.
    #[must_use]
    pub fn host_arch() -> &'static str {
        std::env::consts::ARCH
    }

    /// Are these bytes for the machine asking?
    ///
    /// **A correctly signed binary for another machine is still a machine with no elevation left**:
    /// the helper cannot be loaded, every later prompt fails, and the only way back is a reinstall.
    /// So this is checked beside the signature rather than assumed from it.
    #[must_use]
    pub fn is_for_host(&self) -> bool {
        if self.os != Self::host_os() {
            return false;
        }

        // macOS publishes one universal helper listed under two architecture rows — the T88
        // design's D6, one artifact along — so there it is a third spelling of this machine.
        self.arch == Self::host_arch() || (self.os == "macos" && self.arch == "universal")
    }
}

/// One batch of privileged operations, covered by one prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PrivilegedRequest {
    /// The protocol this daemon speaks. A helper that does not know it refuses the whole request.
    pub version: ProtocolVersion,

    /// `MIXENGINE_HOME`. Every path in every operation is canonicalised and must resolve inside it,
    /// and the helper checks that this directory belongs to whoever owns the request file — without
    /// that, `--home C:\Windows\System32` is an escalation for every operation that takes a path.
    pub home: PathBuf,

    /// Echoed into the response, so a daemon cannot read the answer to an earlier request as the
    /// answer to this one. It is **not** the anti-replay check; [`RESPONSE_FILE_NAME`] is.
    pub nonce: String,

    /// The operations, left undecoded.
    ///
    /// A `Vec<PrivilegedOp>` would fail as a whole on one variant this build has never heard of,
    /// which — the helper being excluded from auto-update — is a routine event and not a corruption.
    /// Decoded one element at a time, an unknown operation becomes
    /// [`OpOutcome::Unsupported`] at its own index and its neighbours are applied. The daemon builds
    /// a `Vec<PrivilegedOp>` and serialises it into this field; the asymmetry is confined here.
    pub ops: Vec<serde_json::Value>,
}

/// One line of the managed block: a name, and the address it resolves to.
///
/// The address is an [`IpAddr`] and not a string because the helper refuses anything that is not
/// loopback, and a refusal that had to parse the field first would be a refusal with a second way
/// to be wrong. `serde` renders it the way a hosts file spells one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct HostEntry {
    /// Where the name points. Only `127.0.0.1` and `::1` are ever accepted — see the T41 design, D5.
    pub address: IpAddr,

    /// The name, lowercased and already checked by whoever built this.
    pub domain: String,
}

/// One port a site is reached on, and the ordinary port a program binds to answer it.
///
/// On macOS these differ — a packet-filter rule sends 80 to 8080 — and on the other two they do not.
/// The pair travels together because the layer that generates a front end's configuration needs both
/// numbers and may not ask which operating system it is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PortRedirect {
    /// What a browser asks for: 80 or 443.
    pub answer: u16,

    /// What a program actually binds, which an ordinary account may.
    pub bind: u16,
}

/// What granting port access means on the machine being asked — the T42 design, D2 and D4.
///
/// **Two variants rather than one struct holding both a binary and a redirect list**, because a
/// field the helper does not use is a field the helper cannot validate, and validating is that
/// binary's entire job. Every OS refuses the variant that is not its mechanism; no branch quietly
/// does nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "kebab-case", deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum PortAccessPlan {
    /// Linux: `cap_net_bind_service` on the front end's binary, which then binds 80 itself.
    Capability {
        /// The program the capability goes on. The helper checks that the caller owns it and that
        /// nobody else can write it — the T42 design, D5.
        binary: PathBuf,

        /// Which reserved ports it is being allowed. Only 80 and 443 are ever accepted.
        ports: Vec<u16>,
    },

    /// macOS: a packet-filter anchor, its declaration in `/etc/pf.conf`, and the boot job that
    /// enables pf — see ADR 0012. The program binds an ordinary port instead.
    Redirect {
        /// Every port that moves, and where to.
        redirects: Vec<PortRedirect>,
    },
}

/// What taking port access away means.
///
/// Mirrors [`PortAccessPlan`] with only the fields a removal reads: a capability is cleared whole,
/// so there is no port to name, and the three files a redirect leaves behind are constants in the
/// helper rather than anything a request may choose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "kebab-case", deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum PortAccessTarget {
    /// Clear `security.capability` from this binary.
    Capability {
        /// The program to clear it from, checked exactly as a grant's is.
        binary: PathBuf,
    },

    /// Remove the anchor, its block in `/etc/pf.conf` and the boot job.
    ///
    /// `Redirect {}` and not `Redirect`, for the reason [`PrivilegedOp::Probe`] is written that way:
    /// serde reads a unit variant of an internally tagged enum through `deserialize_any`, where
    /// `deny_unknown_fields` never gets a chance to fire.
    Redirect {},
}

/// How this machine is asked to route managed TLDs to MixEngine's own DNS server — the T45
/// design, D2 and D3.
///
/// What ports this machine should have open for MixEngine — roadmap task **T74**.
///
/// **Whole state, like [`ResolverPlan`] and [`PortAccessPlan`]**: the plan names every port that
/// should end up open, so a second request supersedes the first, "already done" is a comparison
/// rather than a judgement, and revoking is this same operation carrying an empty list. There is no
/// `FirewallRevoke` beside it for that reason.
///
/// **And unlike [`ResolverPlan`], it carries no OS mechanism.** That type has one variant per
/// mechanism because the daemon reads which one this machine has — through `ResolverConfig`, in
/// `mixengine-platform`, which this crate sits below and cannot link to — before it plans. Nothing
/// reads the firewall: the daemon has no trait for it and never asks what rules exist, so the helper
/// picks
/// the mechanism itself — `netsh` on Windows, `ufw` or `firewalld` on Linux where one is active,
/// and nothing at all on macOS, whose application firewall needs no rule for a listening socket.
/// A field the helper does not need is a field it cannot validate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FirewallPlan {
    /// Every port that should be reachable from the local network, sorted and deduplicated.
    ///
    /// Empty is the revoke, and is valid. Each one is checked by the helper itself against rules it
    /// can apply without a database — see `mixengine-elevate`'s own `firewall` module.
    pub ports: Vec<u16>,

    /// What the rules are named, so they can be found and removed again.
    ///
    /// The helper refuses anything that does not begin [`FIREWALL_LABEL`], which is what stops a
    /// compromised daemon from writing a rule MixEngine would never recognise as its own — and
    /// therefore never clean up.
    pub label: String,
}

/// What every firewall rule MixEngine writes is called.
///
/// A rule outside this prefix is not ours: uninstall enumerates by it, `mix doctor` reports by it,
/// and the helper refuses a plan whose label does not start with it.
pub const FIREWALL_LABEL: &str = "MixEngine — ";

/// The words an outcome's detail carries when the operating system accepted a removal but will only
/// perform it at the next restart — roadmap task **T87**.
///
/// **A constant because it is a promise to a person**, and it appears in three places a person can
/// read: the audit log's line, `mix job` output, and the sentence beside the row in `mix uninstall`.
/// One spelling for one fact.
///
/// **The daemon does not decide anything by reading it.** Whether a removal is reported as
/// [`Removal::OnRestart`](crate::Removal::OnRestart) is settled by the queue and the disk — the
/// operation is no longer waiting and the file is still there — because a decision that turned on
/// matching a sentence is a decision that breaks the day somebody rewords the sentence.
pub const AT_NEXT_RESTART: &str = "at the next restart";

/// **It carries no nameserver address, no link name and no registry key**, and that is the security
/// decision of the task rather than an economy. `mixengine-elevate` exists because a compromised
/// daemon *is* the attacker, so an operation that accepted an address from the request would be one
/// that let whoever owns the daemon redirect this machine's name resolution anywhere — with a valid
/// signature, through the audited binary, under the user's own Allow click. Every one of those
/// values is compiled into the helper. What travels is the two things the helper cannot know: which
/// of the managed TLDs to wire, and which port the server is listening on.
///
/// One variant per OS mechanism, as [`PortAccessPlan`] has and for its reason: a field the helper
/// does not use is a field the helper cannot validate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "kebab-case", deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ResolverPlan {
    /// macOS: one `/etc/resolver/<tld>` per TLD, each naming a port.
    ResolverDirectory {
        /// Which TLDs to route. Every one is checked against
        /// [`WIRED_TLDS`](crate::domains::WIRED_TLDS) by the helper itself.
        tlds: Vec<String>,

        /// Where the server is listening.
        ///
        /// A real field because `[dns] port` is a real setting and a test daemon binds an ephemeral
        /// one; bounded rather than trusted, being refused at zero.
        port: u16,
    },

    /// Linux: a dummy link of MixEngine's own, declared to `systemd-networkd`.
    ///
    /// **A link rather than a file of resolver settings** — the T45 design, D10, and every
    /// alternative was measured out: a `resolved.conf.d` drop-in redirects the whole machine, the
    /// loopback link is refused by systemd-resolved by name, a real link would have its own servers
    /// replaced rather than added to, and a link with no address is configured and inert.
    SystemdLink {
        /// Which TLDs become routing domains on that link.
        tlds: Vec<String>,

        /// Where the server is listening.
        port: u16,
    },

    /// Windows: **one** NRPT rule naming every TLD.
    ///
    /// Its `Name` value is a `REG_MULTI_SZ`, so all of them live under the one GUID the helper
    /// compiles in rather than one rule each — which is what makes "already done" a read of a single
    /// key. **No port**: NRPT has no field for one, which is why T44 puts the server on 53 here.
    Nrpt {
        /// Which namespaces the rule names.
        tlds: Vec<String>,
    },
}

/// What unwiring means, per mechanism.
///
/// Mirrors [`ResolverPlan`] with only what a removal reads. Every artifact each variant deletes is
/// a constant in the helper, so no variant carries a path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "kebab-case", deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ResolverTarget {
    /// Remove every `/etc/resolver/<tld>` file MixEngine marked.
    ResolverDirectory {},

    /// Remove both `systemd-networkd` files and the link they declare.
    SystemdLink {},

    /// Remove the one registry key.
    Nrpt {},
}

/// How this machine is asked to trust MixEngine's own certificate authority — roadmap task **T49a**.
///
/// **The certificate's DER travels; a path to it does not.** [`ResolverPlan`] above carries the
/// argument in full: what the helper can know is compiled into the helper, and a path is somebody
/// else choosing which file root reads after root has decided to trust the request. The destination
/// store, the file name on Linux and the update command are all constants in `mixengine-elevate`.
///
/// One variant per OS mechanism, as [`PortAccessPlan`] and [`ResolverPlan`] have and for their
/// reason: a plan naming a mechanism is a plan the helper re-validates against the machine it is
/// actually running on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "kebab-case", deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum TrustPlan {
    /// Windows: the `Root` store under `LocalMachine`.
    SystemRoot {
        /// The certificate, DER. Checked against the T49a design's D4 table by the helper itself,
        /// before a store is opened.
        der: Vec<u8>,
    },

    /// macOS: `/Library/Keychains/System.keychain`, as a trusted root.
    SystemKeychain {
        /// As [`SystemRoot`](Self::SystemRoot).
        der: Vec<u8>,
    },

    /// Linux, Debian family: `/usr/local/share/ca-certificates`, then `update-ca-certificates`.
    CaCertificates {
        /// As [`SystemRoot`](Self::SystemRoot).
        der: Vec<u8>,
    },

    /// Linux, Red Hat family: `/etc/pki/ca-trust/source/anchors`, then `update-ca-trust`.
    CaTrustAnchors {
        /// As [`SystemRoot`](Self::SystemRoot).
        der: Vec<u8>,
    },
}

/// Which authority to take back out, and **not which certificate** — the T49a design, D5.
///
/// **There is no fingerprint field, and that is the whole of this type's security decision.** The
/// install direction is close to harmless: a daemon compromised badly enough to forge one already
/// holds the private key of the authority this machine trusts, and can sign any certificate for any
/// name without installing a second root. A *removal* that named a certificate by its hash is not —
/// it could take out the root that validates Windows Update, or an organisation's own root, through
/// the audited binary and under the user's own Allow click.
///
/// So what travels is T48's key-id: eight lowercase hex characters, refused by the helper before a
/// store is opened, and unable to describe a corporate root at all. The helper then finds
/// certificates whose subject is exactly `MixEngine Local CA <key_id>`, checks each against D4's
/// table, and removes only those that pass. This is [`ResolverPlan`]'s own argument one capability
/// along: the value an attacker would abuse is not validated, it is absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "kebab-case", deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum TrustTarget {
    /// Windows.
    SystemRoot {
        /// Eight lowercase hex characters, and nothing else is accepted.
        key_id: String,
    },

    /// macOS.
    SystemKeychain {
        /// As [`SystemRoot`](Self::SystemRoot).
        key_id: String,
    },

    /// Linux, Debian family.
    CaCertificates {
        /// As [`SystemRoot`](Self::SystemRoot).
        key_id: String,
    },

    /// Linux, Red Hat family.
    CaTrustAnchors {
        /// As [`SystemRoot`](Self::SystemRoot).
        key_id: String,
    },
}

/// The closed list of things that cross into the elevated process.
///
/// See `docs/architecture/platform-abstraction.md`: the list is closed against operations **with
/// effects**, and adding one of those requires an ADR. [`PrivilegedOp::Probe`] has none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum PrivilegedOp {
    /// Report this build: nothing is read, nothing is written, nothing is changed.
    ///
    /// What it reports arrives in the [`PrivilegedResponse`] header rather than in its own outcome,
    /// because every answer carries it — see that type.
    ///
    /// It is `Probe {}` and not `Probe` because serde deserialises a *unit* variant of an
    /// internally tagged enum through `deserialize_any`, which reads the map and drops every key
    /// but the tag — `deny_unknown_fields` never gets a chance to fire. An empty struct variant is
    /// deserialised as a struct, where it does. The rule above is only worth having if it holds for
    /// the operation that carries no fields as well as the ones that do.
    Probe {},

    /// Set MixEngine's block in the hosts file to exactly `entries`.
    ///
    /// **The whole state, not a delta** — the T41 design, D1. A block that has drifted cannot be
    /// pulled back by "add this line", so a whole-state operation is idempotent, is its own repair,
    /// and makes "already done" a byte comparison rather than a judgement. An empty list removes
    /// the block.
    HostsApply {
        /// Sorted and deduplicated by [`PrivilegedOp::hosts_apply`], which is the only way one
        /// should be built.
        entries: Vec<HostEntry>,
    },

    /// Let this machine's front end answer on the ports the OS reserves — roadmap task **T42**.
    ///
    /// **Whole state, like [`HostsApply`](Self::HostsApply)**: the plan says what the machine should
    /// end up allowing, so a second request supersedes the first rather than queueing behind it, and
    /// "already done" is a comparison rather than a judgement.
    PortAccessGrant {
        /// What to grant, and how — one variant per OS mechanism.
        plan: PortAccessPlan,
    },

    /// Take it away again.
    ///
    /// **Nothing in T42 enqueues one** — the T42 design, D12. The producer asks in one direction
    /// only, deliberately: on Linux the question needs the front end's binary, which is exactly what
    /// a home with no front end cannot supply. Uninstall (T87) is the producer that can. It ships
    /// built, validated and tested, which is the shape T20, T21 and T22 landed in.
    PortAccessRevoke {
        /// What to take away.
        target: PortAccessTarget,
    },
    /// Route this home's managed TLDs to MixEngine's own DNS server — roadmap task **T45**.
    ///
    /// **Whole state**, like [`HostsApply`](Self::HostsApply) and
    /// [`PortAccessGrant`](Self::PortAccessGrant): the plan says what this machine should end up
    /// routing, so a second request supersedes the first, "already done" is a comparison rather
    /// than a judgement, and a wiring that drifted is repaired by the operation that created it.
    ResolverApply {
        /// What to route, and how — one variant per OS mechanism.
        plan: ResolverPlan,
    },

    /// Take it away again.
    ///
    /// **Nothing in T45 enqueues one** — the T45 design, D13, on T42's precedent. Uninstall (T87)
    /// is the producer. It ships built, validated and tested because reversing a wiring written
    /// five phases earlier is a worse task than writing both halves while the mechanism is in view.
    ResolverRevoke {
        /// Which mechanism's artifacts to remove.
        target: ResolverTarget,
    },

    /// Make this machine trust MixEngine's own certificate authority — roadmap task **T49a**.
    ///
    /// **The helper refuses a certificate that is not shaped like one T48 generates, and the reason
    /// is not that this stops a compromised daemon.** One holding the CA key can already sign
    /// anything. It is that `mix cert ca-uninstall` (T54) and uninstall (T87) must be able to
    /// enumerate everything an install could ever have put into a store, and an unconstrained one
    /// could leave behind a root called anything at all, which nothing would ever find again.
    TrustCaInstall {
        /// Which store, and the certificate.
        plan: TrustPlan,
    },

    /// Take it back out again.
    ///
    /// **Nothing in T49a enqueues one** — the T49a design, D5, on T42's D12 and T45's D13. T54 and
    /// T87 are the producers. It ships built, validated and tested because reversing a mechanism
    /// phases after it was written is a worse task than writing both halves while it is in view.
    TrustCaRemove {
        /// Which authority, by key-id. Never a fingerprint — see [`TrustTarget`].
        target: TrustTarget,
    },

    /// Let the local network reach the ports a shared site answers on — roadmap task **T74**.
    ///
    /// **Whole state**, like every operation above it, so unsharing the last shared site is this
    /// same operation carrying no ports rather than a revoke of its own.
    FirewallApply {
        /// What should end up open, and under what name.
        plan: FirewallPlan,
    },

    /// Put this helper where only an administrator can rewrite it — roadmap task **T85**.
    ///
    /// **It carries nothing, and that is the design** (the T85 design, D2). The alternative — a
    /// `source` field — would hand a compromised daemon a primitive it does not have today: *copy
    /// this file, as root, into a directory only root can write*. That is `Exec { cmd }` with two
    /// more steps, and the closed-enum rule in `docs/architecture/security-model.md` exists to
    /// refuse exactly this shape. What is copied is the elevated process's own image; where it goes
    /// is a constant compiled into that binary. Neither end of the copy is anything the caller said.
    ///
    /// **Enqueued at every daemon start and applied inside the prompt first-run setup already
    /// costs**, beside the resolver wiring, the CA install and the port grant. A `.deb`, an `.rpm`
    /// or a `.pkg` has usually done the work already, and then this answers
    /// [`OpOutcome::AlreadyDone`]; the four ways of installing that run entirely as the user — the
    /// per-user Windows installer, the portable zip, the AppImage, and a `cargo build` — are why the
    /// mechanism cannot be the packager's.
    ///
    /// `HelperInstall {}` and not `HelperInstall`, for [`Probe`](Self::Probe)'s reason.
    HelperInstall {},

    /// Replace this helper with a newer one MixEngine downloaded — roadmap task **T88a**.
    ///
    /// **It carries nothing, on [`HelperInstall`](Self::HelperInstall)'s rule**, and the rule
    /// survives here for a reason worth writing out. The candidate is at
    /// [`helper_candidate`]`(home)` — a compiled-in name under the directory the elevated process
    /// has *already* established belongs to whoever wrote the request. So a compromised daemon can
    /// put any bytes there and gains nothing by it: the elevated process checks a detached minisign
    /// signature over those bytes against a public key compiled into the copy running now, and
    /// refuses anything the signed trusted comment ([`HelperStamp`]) says is older than itself or
    /// built for another machine.
    ///
    /// **That check is the whole of T88a**, and it is why this is not the
    /// `HelperInstall { source }` ADR 0015 refused: the primitive is not *copy this file as root*
    /// but *install a `mixengine-elevate` that MixEngine signed, and never an older one*.
    ///
    /// **Only the installed copy may apply it.** A helper running out of a directory the user can
    /// write, checking a signature, proves nothing — whoever could replace the helper could replace
    /// the check. On a machine with nothing installed the operation to ask for is
    /// [`HelperInstall`](Self::HelperInstall).
    ///
    /// `HelperReplace {}` and not `HelperReplace`, for [`Probe`](Self::Probe)'s reason.
    HelperReplace {},

    /// Take that helper back off this machine — roadmap task **T87**.
    ///
    /// **It carries nothing, on [`HelperInstall`](Self::HelperInstall)'s rule** (the T85 design,
    /// D2): where the file is is a constant compiled into `mixengine-elevate`, so this is not a
    /// *delete this file as root* primitive a compromised daemon could aim anywhere.
    ///
    /// **On Windows this cannot complete at once, and the helper says so rather than pretending.** A
    /// file whose image is mapped cannot be unlinked, and the helper is the running program when it
    /// applies this; there the removal is handed to the operating system's own queue and happens at
    /// the next restart, and the outcome's detail says so in [`AT_NEXT_RESTART`]'s words. What the
    /// daemon reports it as is settled by the queue and the disk rather than by that sentence — see
    /// the constant. The T87 design, D8.
    HelperRemove {},

    /// Remove what the macOS `.pkg` placed outside the home — roadmap task **T182a**.
    ///
    /// **It carries nothing, on [`HelperRemove`](Self::HelperRemove)'s rule**: the four command-line
    /// binaries, the application bundle and the package receipt are constants compiled into
    /// `mixengine-platform`, so this is not a *delete this as root* primitive a daemon could aim. The
    /// bundle goes only when its `Info.plist` names MixLab. Other systems answer `Unsupported`.
    PackageRemove {},

    /// Remove the root-owned record of what ran as root — roadmap task **T87**.
    ///
    /// **The one thing outside `MIXENGINE_HOME` that no other operation can reach.** The log lives
    /// where the user cannot unlink it, which is the whole reason it is outside the home — so taking
    /// it away needs a privileged operation of its own.
    ///
    /// **Applied after every other operation in the batch, and recorded nowhere.** The line
    /// describing this removal would recreate the file the removal exists to remove, so this is the
    /// one operation that log cannot record (the T87 design, D5). Its outcome still arrives in
    /// [`PrivilegedResponse::results`] at its own index, which is the record the daemon reads.
    AuditLogRemove {},
}

impl PrivilegedOp {
    /// Every operation this build knows, by wire name.
    ///
    /// Reported in [`PrivilegedResponse::supported_ops`] so a daemon can find out what the installed
    /// helper can do without spending a prompt to discover it by failure.
    pub const ALL: &'static [&'static str] = &[
        "probe",
        "hosts-apply",
        "port-access-grant",
        "port-access-revoke",
        "resolver-apply",
        "resolver-revoke",
        "trust-ca-install",
        "trust-ca-remove",
        "firewall-apply",
        "helper-install",
        "helper-replace",
        "helper-remove",
        "package-remove",
        "audit-log-remove",
    ];

    /// A hosts change from whatever order its caller happened to have.
    ///
    /// Sorted and deduplicated, so two orderings of one change are one operation: the queue
    /// deduplicates on identity (see [`dedupe_key`](Self::dedupe_key)) and the *equality* below it
    /// is what decides whether anything is announced.
    #[must_use]
    pub fn hosts_apply(entries: impl IntoIterator<Item = HostEntry>) -> Self {
        let mut entries: Vec<HostEntry> = entries.into_iter().collect();

        // By name first: this order is what the block is rendered in and what `describe` reads out,
        // and a person scanning a dialog is scanning names.
        entries.sort_by(|left, right| {
            (left.domain.as_str(), left.address).cmp(&(right.domain.as_str(), right.address))
        });
        entries.dedup();

        Self::HostsApply { entries }
    }

    /// The identity a queue deduplicates on — the T41 design, D2.
    ///
    /// For an operation that carries no state this is its serialisation, so two identical requests
    /// are one row. For a **whole-state** operation it is the bare kind: two `hosts-apply` rows
    /// disagreeing about what the file should hold would both be valid and both be rendered on the
    /// one screen whose job is to say what is about to happen, so the newer state supersedes the
    /// older one instead.
    #[must_use]
    pub fn dedupe_key(&self) -> String {
        match self {
            // Falling back to the tag cannot happen — this type holds nothing serde refuses — and
            // is written rather than unwrapped because nothing in this crate panics.
            Self::Probe {} => {
                serde_json::to_string(self).unwrap_or_else(|_| self.name().to_owned())
            }
            Self::HostsApply { .. } => self.name().to_owned(),
            // D12: two values of one question — what port access should this machine have? — so a
            // revoke enqueued behind a pending grant replaces it rather than queueing after it.
            Self::PortAccessGrant { .. } | Self::PortAccessRevoke { .. } => {
                "port-access".to_owned()
            }
            // D4, on the line above's shape: two values of one question — what should this
            // machine route? — so a revoke enqueued behind a pending apply replaces it rather
            // than queueing after it.
            Self::ResolverApply { .. } | Self::ResolverRevoke { .. } => "resolver".to_owned(),
            // The line above's shape once more: two values of one question — what should this
            // machine trust? — so a removal enqueued behind a pending install replaces it rather
            // than queueing after it.
            Self::TrustCaInstall { .. } | Self::TrustCaRemove { .. } => "trust-store".to_owned(),
            // One value of one question — what should this machine have open? — so a second plan
            // enqueued behind a pending one replaces it. Unsharing while a share is still waiting
            // for the prompt therefore leaves nothing to allow, which is the correct outcome.
            Self::FirewallApply { .. } => "firewall".to_owned(),
            // Two values of one question — is the helper where it belongs? — so a removal
            // enqueued behind a pending install replaces it rather than queueing after it, which is
            // the arrangement `TrustCaInstall`/`TrustCaRemove` has three arms above. Written out
            // rather than taken from `name()`, which is how the install alone used to spell it: the
            // two names differ and the key must not.
            Self::HelperInstall {} | Self::HelperReplace {} | Self::HelperRemove {} => {
                "helper-install".to_owned()
            }
            // No opposite: nothing installs the log, the helper creates it on its first elevated
            // run. One value of one question, so the name is the whole key.
            // One value of one question — is the program the package placed still here? — with no
            // opposite: the package installs it, nothing here does.
            Self::PackageRemove {} => "package".to_owned(),
            Self::AuditLogRemove {} => "audit-log".to_owned(),
        }
    }

    /// Does this operation need an administrative token to mean anything?
    ///
    /// **A property of the operation, not a gate on the process.** The obvious frame refuses to do
    /// anything at all when it is not elevated, and `Probe` is what shows that to be wrong: the
    /// operation whose job includes reporting whether the token is elevated could then never report
    /// `false`. The helper applies this at one place, which is what keeps it auditable.
    #[must_use]
    pub fn requires_elevation(&self) -> bool {
        match self {
            Self::Probe {} => false,
            Self::HostsApply { .. } => true,
            Self::PortAccessGrant { .. } | Self::PortAccessRevoke { .. } => true,
            Self::ResolverApply { .. } | Self::ResolverRevoke { .. } => true,
            Self::TrustCaInstall { .. } | Self::TrustCaRemove { .. } => true,
            // True even on macOS, where the helper will answer `Unmanaged` without touching
            // anything: what the machine turns out not to need is decided by the helper, and a
            // planner that decided it here would be deciding it from the wrong side of the trust
            // boundary.
            Self::FirewallApply { .. } => true,
            // The copy lands in a directory an ordinary account cannot write, which is the whole
            // point of it: without a token there is nothing this could do but fail.
            Self::HelperInstall {} => true,
            // The destination is that same directory — and the copy that decides whether the
            // candidate deserves to go there is the one already living in it.
            Self::HelperReplace {} => true,
            // Both removals reach inside a directory only an administrator can write, for that same
            // reason and with the same consequence.
            // The package's files are root's, and so is the receipt database.
            Self::HelperRemove {} | Self::PackageRemove {} | Self::AuditLogRemove {} => true,
        }
    }

    /// The wire tag, which is also what the audit log records.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Probe {} => "probe",
            Self::HostsApply { .. } => "hosts-apply",
            Self::PortAccessGrant { .. } => "port-access-grant",
            Self::PortAccessRevoke { .. } => "port-access-revoke",
            Self::ResolverApply { .. } => "resolver-apply",
            Self::ResolverRevoke { .. } => "resolver-revoke",
            Self::TrustCaInstall { .. } => "trust-ca-install",
            Self::TrustCaRemove { .. } => "trust-ca-remove",
            Self::FirewallApply { .. } => "firewall-apply",
            Self::HelperInstall {} => "helper-install",
            Self::HelperReplace {} => "helper-replace",
            Self::HelperRemove {} => "helper-remove",
            Self::PackageRemove {} => "package-remove",
            Self::AuditLogRemove {} => "audit-log-remove",
        }
    }
}

/// What the helper did, one entry per operation, plus what it is.
///
/// **The report is a property of the response and not the outcome of `Probe`.** Nesting it in
/// [`OpOutcome::Applied`]'s `detail` would put a JSON document inside a JSON string, and would mean
/// the daemon learns what the installed helper can do only on the round trips where it thought to
/// ask. Here it costs a few strings, arrives on every answer, and is read the same way whatever the
/// request contained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PrivilegedResponse {
    /// The protocol the helper speaks.
    pub version: ProtocolVersion,

    /// The helper binary's own version, which is not the protocol's: it is installed once and
    /// excluded from auto-update, so it drifts behind the daemon by design.
    pub elevate_version: String,

    /// Echoed from the request.
    pub nonce: String,

    /// Was this process actually running with an administrative token?
    pub elevated: bool,

    /// [`PrivilegedOp::ALL`] for the build that answered.
    pub supported_ops: Vec<String>,

    /// Where this helper records what it applied — reported whether or not anything was written to
    /// it, so `mix doctor` can find it on a machine where nothing has been applied yet.
    pub audit_log: PathBuf,

    /// One outcome per element of [`PrivilegedRequest::ops`], at the same index.
    pub results: Vec<OpOutcome>,
}

/// What became of one operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum OpOutcome {
    /// Done, and the machine changed.
    Applied {
        /// What changed, for a log line and for `mix doctor`.
        detail: String,
    },

    /// The machine was already in the state this asked for. Not a failure and not a change.
    AlreadyDone,

    /// Validation said no — the caller's fault, and the same request will be refused again.
    Refused {
        /// Which rule it broke.
        reason: String,
    },

    /// This build does not know this operation, or does not understand it as it was written.
    Unsupported {
        /// What could not be decoded.
        reason: String,
    },

    /// The operating system refused. Trying again may work; nothing about the request is wrong.
    Failed {
        /// The OS's own complaint.
        message: String,
    },

    /// This machine has no mechanism for what was asked, and saying so is the honest answer —
    /// roadmap task **T74**.
    ///
    /// **Neither [`Applied`](Self::Applied) nor [`Failed`](Self::Failed), and the distinction is the
    /// point.** macOS's application firewall needs no rule for a listening socket, and a Linux with
    /// neither `ufw` nor `firewalld` running has nothing to add a rule to. Reporting that as success
    /// would tell a user their phone can reach the site when the machine has done nothing to make
    /// it so; reporting it as failure would stop a share that is, on those machines, already
    /// working. So it is its own outcome, carrying the command a person would run if their machine
    /// does turn out to block the port.
    Unmanaged {
        /// Why nothing was done, phrased for a user.
        reason: String,

        /// What to run by hand where the port does turn out to be blocked. Empty where there is
        /// nothing sensible to suggest.
        manual: String,
    },
}

/// What became of one *attempt to elevate* — the outcome of raising the prompt, not of the batch.
///
/// Defined here and used by T40a, where the three launchers live. A declined prompt cannot be an exit
/// code of the helper's, because when the user clicks Cancel the helper never ran: `ERROR_CANCELLED`
/// (1223), osascript's `-128` and `pkexec`'s 126 all map onto [`ElevationOutcome::Declined`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ElevationOutcome {
    /// The helper ran. Whether the batch succeeded is in the response file.
    Completed,

    /// The user said no. A normal outcome, and the daemon goes into degraded mode (T40b).
    Declined,

    /// There is no way to raise a prompt on this machine — no polkit agent, no session.
    Unavailable {
        /// What is missing, phrased for a user, with the manual command where one exists.
        reason: String,
    },
}

#[cfg(test)]
mod tests;
