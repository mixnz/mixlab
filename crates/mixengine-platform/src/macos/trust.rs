//! macOS: the System keychain, through `/usr/bin/security`.
//!
//! **Not through Security.framework, and that is a decision about the other binary** — the T49a
//! design, D6. `SecCertificateCreateWithData`, `SecItemAdd` and `SecTrustSettingsSetTrustSettings`
//! would mean a new unsafe FFI surface inside `mixengine-elevate`, whose whole design constraint is
//! that a person can audit it by reading it, for an operation that runs once per install. The rule
//! T42 set with `pfctl` and T45 kept with `systemctl` holds here: one fixed command, a constant
//! argument vector, and no argument taken from the request.
//!
//! Reading is `security find-certificate -a -p`, which lists every certificate in a keychain as PEM
//! and **needs no administrative token** — measured by `tests/trust.rs` in CI's ordinary `test` job
//! rather than asserted here.

#[cfg(feature = "elevated")]
use crate::trust::Change;
#[cfg(feature = "host")]
use crate::{Result, TrustState, TrustStore, TrustStoreMethod};

/// The keychain a machine-wide root belongs in.
#[cfg(any(feature = "host", feature = "elevated"))]
pub(crate) const SYSTEM_KEYCHAIN: &str = "/Library/Keychains/System.keychain";

/// What the certificate is called while `security` reads it.
///
/// In the root-owned audit directory, so no unprivileged account can replace what is at this path
/// between the write and the read.
#[cfg(feature = "elevated")]
const HANDOFF_FILE: &str = "ca-handoff.pem";

/// Absolute, never resolved through `PATH`: this is invoked from a process holding an
/// administrative token, and a `PATH` entry is something another program can arrange.
#[cfg(any(feature = "host", feature = "elevated"))]
pub(crate) const SECURITY: &str = "/usr/bin/security";

/// Apple's, absolute for the same reason. What puts `security` into the caller's login session for
/// the one write that needs a window — see [`apply`].
#[cfg(feature = "elevated")]
const LAUNCHCTL: &str = "/bin/launchctl";

/// How long the trust-settings write may take, which is how long a person may take to type a
/// password: macOS raises a dialog for it and `security` waits on the answer. Nothing else in this
/// module waits on a person, so nothing else gets this.
#[cfg(feature = "elevated")]
const DIALOG_PATIENCE: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// This system's answer.
#[cfg(feature = "host")]
#[derive(Debug, Default)]
pub(crate) struct Trust;

#[cfg(feature = "host")]
impl TrustStore for Trust {
    fn method(&self) -> Result<TrustStoreMethod> {
        // A constant, unlike Linux: every macOS has this keychain — D7.
        Ok(TrustStoreMethod::SystemKeychain)
    }

    fn probe(&self, der: &[u8]) -> Result<TrustState> {
        let listed = certificates()?;

        // Exact DER bytes — D6. The listing carries `security`'s own SHA-1 beside each certificate
        // and this deliberately does not consult it: that is a different value from the SHA-256
        // `cert.ca_status` reports, and carrying two hashes for one identity is how they come apart.
        // The hash is there for the removal, which needs a name to hand back, and for nothing else.
        let present = listed.iter().any(|found| found.der == der);

        if !present {
            return Ok(TrustState {
                method: TrustStoreMethod::SystemKeychain,
                installed: false,
                missing: Some(format!(
                    "{SYSTEM_KEYCHAIN} does not hold MixEngine's certificate authority"
                )),
            });
        }

        // **In the keychain is not the same as trusted, and this asks the second question.**
        // `add-trusted-cert -d` is two writes — the certificate into the keychain, then a trust
        // setting into the admin domain — and the second can be refused after the first succeeded:
        // measured on a machine where `security` answered *the authorization was denied since no
        // user interaction was possible*, left the certificate in the keychain, and every later
        // probe reported it installed while `verify-cert` said `CSSMERR_TP_NOT_TRUSTED` and every
        // browser agreed. The daemon has no root-owned directory to hand `security` a path in, so
        // the file goes where this user's temporary files go, under a name only this process uses.
        let file =
            std::env::temp_dir().join(format!("mixengine-ca-probe-{}.pem", std::process::id()));
        std::fs::write(&file, crate::trust::pem::encode(der)).map_err(|source| {
            crate::Error::Io {
                action: "write the certificate for `security verify-cert`",
                path: file.clone(),
                source,
            }
        })?;
        let trusted = trusted(&file);
        let _ = std::fs::remove_file(&file);
        let trusted = trusted?;

        Ok(TrustState {
            method: TrustStoreMethod::SystemKeychain,
            installed: trusted,
            missing: (!trusted).then(|| {
                format!(
                    "{SYSTEM_KEYCHAIN} holds MixEngine's certificate authority but this machine \
                     does not trust it: the trust setting was never written{}",
                    BY_HAND
                )
            }),
        })
    }
}

/// What a person can type when the helper's own write of the trust setting is refused.
///
/// The one `security` call in this module that needs more than root: `add-trusted-cert -d` writes
/// the admin trust domain, whose authorization rule is *entitled or authenticate-admin*, and
/// `authenticate-admin` on a stock macOS does not exempt root. Under an elevation prompt there is no
/// window to authenticate in, so the same command from a terminal — where there is — is the honest
/// thing to offer.
#[cfg(any(feature = "host", feature = "elevated"))]
const BY_HAND: &str = "; from a terminal, `sudo security add-trusted-cert -d -r trustRoot -k \
                       /Library/Keychains/System.keychain <this home>/certs/ca/root.crt` writes it";

/// Does this machine trust the certificate in `file` — as an anchor, in any domain?
///
/// `security verify-cert` is the one question that has the same answer a browser gets: it consults
/// the admin and user trust domains and the system roots together, and exits zero only when the
/// chain ends at something this machine trusts. Measured: a root in the keychain with no trust
/// setting exits 1 with `CSSMERR_TP_NOT_TRUSTED`; a trusted one prints *certificate verification
/// successful*. `-L` keeps it off the network, and `basic` is the X.509 policy with no name or
/// key-usage demand a root would fail for reasons that are not about trust.
///
/// # Errors
///
/// When `security` itself cannot be run; a non-zero exit is an answer, not an error.
#[cfg(any(feature = "host", feature = "elevated"))]
fn trusted(file: &std::path::Path) -> crate::Result<bool> {
    let output = security(
        &[
            "verify-cert",
            "-L",
            "-c",
            &file.to_string_lossy(),
            "-p",
            "basic",
        ],
        "run security to ask whether this machine trusts a certificate",
    )?;

    Ok(output.status.success())
}

/// A certificate in the System keychain: the DER, and the SHA-1 `security` itself reports for it.
///
/// **The hash is carried and never computed.** Recomputing it would mean `sha2`-shaped weight in a
/// binary that runs as root, and the T49a design's D11 refused that. What this is for is naming one
/// exact certificate back to `security` in the removal — and a number the system just printed for a
/// certificate is a better name for it than anything this crate could derive.
#[cfg(any(feature = "host", feature = "elevated"))]
struct Certificate {
    /// As `security` printed it: forty hexadecimal characters.
    sha1: String,

    /// The certificate itself, which is what every check in this crate actually runs against.
    der: Vec<u8>,
}

/// What `-Z` prints before each certificate. There is a `SHA-256 hash:` line as well, deliberately
/// ignored: `delete-certificate` takes the SHA-1, and carrying two hashes for one identity is how
/// they come apart.
#[cfg(any(feature = "host", feature = "elevated"))]
const SHA1_LINE: &str = "SHA-1 hash:";

/// The envelope's two fences, which is how a block is known to have started and ended.
#[cfg(any(feature = "host", feature = "elevated"))]
const BEGIN: &str = "-----BEGIN CERTIFICATE-----";

/// See [`BEGIN`].
#[cfg(any(feature = "host", feature = "elevated"))]
const END: &str = "-----END CERTIFICATE-----";

/// Every certificate in the System keychain.
///
/// Read by both directions: the install compares against it to answer `Unchanged`, and the removal
/// walks it to find what it was asked to take out.
///
/// **An empty keychain is an empty list and not an error.** `security` exits non-zero when it finds
/// nothing, which is a true answer to the question this asks and must not become a failure that
/// stops a daemon start.
///
/// **`crate::Result` written out, and not because the import is untidy.** The `Result` alias is
/// imported under `feature = "host"` and this function is compiled under `host` *or* `elevated`; in
/// the helper's build — which is the only build that ever writes a keychain — a bare `Result` is
/// `std`'s, and the mistake is a compile error nothing on Windows or Linux can reach.
#[cfg(any(feature = "host", feature = "elevated"))]
fn certificates() -> crate::Result<Vec<Certificate>> {
    let output = security(
        &["find-certificate", "-a", "-Z", "-p", SYSTEM_KEYCHAIN],
        "run security to read the System keychain",
    )?;

    // Line by line rather than through `pem::decode_all`, because what is being read here is a
    // *pairing*: each block belongs to the hash line above it, and a parser that only collected the
    // envelopes would throw away the half the removal needs.
    let text = String::from_utf8_lossy(&output.stdout);
    let mut found = Vec::new();
    let mut sha1 = String::new();
    let mut block: Option<String> = None;

    for line in text.lines() {
        let line = line.trim();

        if let Some(rest) = line.strip_prefix(SHA1_LINE) {
            sha1 = rest.trim().to_owned();
            continue;
        }

        if line == BEGIN {
            block = Some(String::new());
        }

        let Some(buffer) = block.as_mut() else {
            continue;
        };

        buffer.push_str(line);
        buffer.push('\n');

        if line != END {
            continue;
        }

        let document = block.take().unwrap_or_default();

        // A block that will not decode is skipped rather than refused, for `decode_all`'s reason: a
        // real keychain listing carries things that are not certificates, and one of them must not
        // stop a daemon start.
        if let Some(der) = crate::trust::pem::decode(document.as_bytes()) {
            found.push(Certificate {
                sha1: std::mem::take(&mut sha1),
                der,
            });
        }
    }

    Ok(found)
}

/// Is this the shape of a hash `security` printed, and therefore something it may be handed back?
///
/// **Checked even though it came from `security` a moment ago**, which is the rule this whole binary
/// is built on: `mixengine-elevate` validates what it is about to act on rather than trusting where
/// it came from. Forty hexadecimal characters cannot be a flag, a path, or a second argument.
#[cfg(feature = "elevated")]
fn is_hash(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// How long a `security` call gets before it is treated as one that will never answer.
///
/// A read of this keychain is milliseconds and a write is not much more; thirty seconds is not a
/// budget, it is the point past which waiting has stopped being waiting.
#[cfg(any(feature = "host", feature = "elevated"))]
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(30);

/// How long the last of the output is waited for once the command itself has exited.
#[cfg(any(feature = "host", feature = "elevated"))]
const GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Run `security`, with no console to ask at and a limit on how long it may take.
///
/// **Two things a helper behind an elevation prompt has to get right, and T49a had neither.**
///
/// `stdin` is `/dev/null`. `security` is a program that asks for a password when it wants one, and
/// this is called from a process that has no terminal to ask at — under `sudo` in CI, and from an
/// OS elevation prompt in front of a user who has already clicked Allow and is looking at nothing.
/// A question nobody can answer must fail, not wait.
///
/// And the wait is bounded. A privileged helper that blocks forever is worse than one that fails:
/// it holds this crate's trust lock while it does it, and the operation it was spawned for is the
/// one standing between a first run and a working machine. Whatever went wrong comes back as
/// `Failed` with the command in the message — which is a thing a person can read — rather than as
/// a job that gets cancelled twenty minutes later having printed nothing at all.
#[cfg(any(feature = "host", feature = "elevated"))]
fn security(arguments: &[&str], action: &'static str) -> crate::Result<std::process::Output> {
    command(SECURITY, arguments, action, PATIENCE)
}

/// [`security`]'s body, for the one call that is not `security` at the front of its command line.
#[cfg(any(feature = "host", feature = "elevated"))]
fn command(
    program: &str,
    arguments: &[&str],
    action: &'static str,
    patience: std::time::Duration,
) -> crate::Result<std::process::Output> {
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let failed = |source| crate::Error::Os { action, source };

    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(failed)?;

    // **Drained on threads of their own, and that is not tidiness.** `find-certificate -a -p`
    // prints every certificate this machine trusts — a couple of hundred kilobytes against a pipe
    // buffer of 64, so a loop that polled for exit without reading would block the child on its own
    // output and then report the deadlock as a timeout.
    let reading_out = read_on_a_thread(child.stdout.take().expect("stdout was piped just above"));
    let reading_err = read_on_a_thread(child.stderr.take().expect("stderr was piped just above"));

    let deadline = Instant::now() + patience;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(failed)? {
            break status;
        }

        if Instant::now() >= deadline {
            // Killed rather than left: this process is about to exit, and a `security` still
            // waiting on something would outlive it as somebody else's child.
            let _ = child.kill();
            let _ = child.wait();

            return Err(failed(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "`{} {}` did not answer within {} seconds",
                    program,
                    arguments.join(" "),
                    patience.as_secs()
                ),
            )));
        }

        std::thread::sleep(std::time::Duration::from_millis(20));
    };

    // **A grace period and not a `join`.** End of file on these pipes means every holder of the
    // write end has gone, and a grandchild the command left behind would be one — so a join here
    // would be one more unbounded wait in the same function that exists to remove them. The exit
    // status is already in hand; what arrived by now is the whole of what this can honestly report.
    Ok(std::process::Output {
        status,
        stdout: reading_out.recv_timeout(GRACE).unwrap_or_default(),
        stderr: reading_err.recv_timeout(GRACE).unwrap_or_default(),
    })
}

/// Read one pipe to the end on a thread, and hand back whatever arrived.
///
/// A read error ends the thread with what it has rather than being raised: what the caller needs is
/// an exit status, and the bytes that did arrive are still the program's account of itself.
#[cfg(any(feature = "host", feature = "elevated"))]
fn read_on_a_thread<R: std::io::Read + Send + 'static>(
    mut pipe: R,
) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (finished, done) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        let _ = finished.send(bytes);
    });

    done
}

/// Hand the certificate to `security` as a trusted root.
///
/// **One fixed command, and the file path is the helper's own** — the T49a design, D6. The DER
/// arrives in the request; the *path* it is written to is chosen here, so the rule T42 set with
/// `pfctl` holds: no argument comes from the request.
///
/// **Run inside the caller's login session, through `launchctl asuser`, and that is what makes it
/// work at all.** `add-trusted-cert -d` writes the admin trust domain, whose authorization rule is
/// *entitled or authenticate-admin*, and `authenticate-admin` does not exempt root: macOS raises a
/// password dialog for it even under `sudo`. A process behind the OS elevation prompt is root but is
/// not in the session that owns the screen, so the dialog cannot be raised and `security` fails
/// with *the authorization was denied since no user interaction was possible* — after it has already
/// put the certificate into the keychain. Measured three ways on one machine: `sudo security
/// add-trusted-cert -d` from a terminal raised the dialog and succeeded; the same command under
/// `do shell script … with administrator privileges` failed as above; and the same command under
/// the same prompt through `/bin/launchctl asuser <uid>` raised the dialog and succeeded. The uid
/// is the caller's, read from the token the helper verified before it opened the request — it is
/// digits or the command is not run — and everything else on the line is a constant.
///
/// So on macOS a first run asks twice: once at the OS elevation prompt, once at this dialog. That
/// is the operating system's price for an admin-domain trust setting and there is no cheaper one
/// without an Apple entitlement; ADR 0005's budget of about two prompts is spent exactly here.
#[cfg(feature = "elevated")]
pub(crate) fn apply(
    plan: &mixengine_proto::privileged::TrustPlan,
    caller: &crate::elevated::Owner,
) -> crate::Result<Change> {
    use mixengine_proto::privileged::TrustPlan;

    let der = match plan {
        TrustPlan::SystemKeychain { der } => der,
        TrustPlan::SystemRoot { .. }
        | TrustPlan::CaCertificates { .. }
        | TrustPlan::CaTrustAnchors { .. } => {
            return Err(crate::trust::unsupported(
                "this is macOS, whose trust store is the System keychain rather than a Windows \
                 certificate store or a Linux anchors directory",
            ));
        }
    };

    let _lock = crate::trust::held()?;

    let file = written(der)?;

    // Read before writing, under the lock: a keychain that already holds exactly this **and trusts
    // it** is `Unchanged`, and adding it again would raise a second trust-settings write for
    // nothing. Both halves, because `add-trusted-cert -d` is two writes and the second can fail
    // after the first: a certificate that is in the keychain with no trust setting is exactly what
    // this call exists to finish, and answering `Unchanged` for it was how a refused trust setting
    // became *already done* on the next prompt and *trusted* in `mix doctor`.
    let present = certificates()?.iter().any(|found| &found.der == der);
    let already = present && trusted(&file).unwrap_or(false);
    if already {
        let _ = std::fs::remove_file(&file);
        return Ok(Change::Unchanged);
    }

    let ran = run_in_session(
        caller,
        &[
            "add-trusted-cert",
            "-d",
            "-r",
            "trustRoot",
            "-k",
            SYSTEM_KEYCHAIN,
            &file.to_string_lossy(),
        ],
    );

    // The handoff file has served its purpose whether or not `security` accepted it, and leaving a
    // certificate lying in a root-owned directory is litter the next run would read.
    let _ = std::fs::remove_file(&file);
    ran?;

    Ok(Change::Written {
        detail: format!("added MixEngine's certificate authority to {SYSTEM_KEYCHAIN}"),
    })
}

/// Take it back out, having first checked that what is there is ours.
///
/// **`delete-certificate -Z`, and `remove-trusted-cert` is not a thing this can use.** Measured on
/// a macOS runner, one command at a time under a twenty-second alarm:
///
/// - `security remove-trusted-cert -d` never returns. Not under plain `sudo`, not under `sudo -H`,
///   not with `HOME` unset, not against a root-owned path, and — the case that settles what kind of
///   fault it is — **not even when there is nothing left to remove**. Without `-d` it fails in a
///   millisecond, so it is the admin domain specifically.
/// - `security trust-settings-import -d` never returns either, with the domain unchanged or with
///   one entry dropped. `trust-settings-export -d` reads it fine and `add-trusted-cert -d` writes
///   it fine. So on a machine with no window server the admin domain can be read and added to, and
///   neither removed from nor replaced.
/// - `security delete-certificate` answers immediately, takes the certificate out of the keychain,
///   **and takes the trust setting with it** — because the admin domain *is* this keychain rather
///   than a store beside it. Proved targeted rather than wholesale by installing two certificates
///   and deleting one: the other was still there and still trusted.
///
/// **What identifies the certificate is the hash `security` printed for it**, not its name.
/// `delete-certificate -c` would match on common name and give up the check this crate performs
/// everywhere else — `windows::store::remove` runs it against every certificate it walks, and so
/// does the loop below. The DER is what is checked; the hash is only how the answer is spoken back.
#[cfg(feature = "elevated")]
pub(crate) fn revoke(target: &mixengine_proto::privileged::TrustTarget) -> crate::Result<Change> {
    use mixengine_proto::privileged::TrustTarget;

    let key_id = match target {
        TrustTarget::SystemKeychain { key_id } => key_id,
        TrustTarget::SystemRoot { .. }
        | TrustTarget::CaCertificates { .. }
        | TrustTarget::CaTrustAnchors { .. } => {
            return Err(crate::trust::unsupported(
                "this is macOS, whose trust store is the System keychain",
            ));
        }
    };

    let _lock = crate::trust::held()?;

    // **D5's second check.** Every certificate in the keychain that both passes the shape check and
    // carries the authority that was named — nothing else is touched, and a keychain holding a
    // corporate root is a keychain this cannot be aimed at.
    let mut removed = 0;
    for certificate in certificates()? {
        let ours =
            crate::trust::ours(&certificate.der).is_ok_and(|authority| &authority.key_id == key_id);
        if !ours {
            continue;
        }

        if !is_hash(&certificate.sha1) {
            return Err(crate::Error::Os {
                action: "read the System keychain",
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "security named a certificate `{}`, which is not a hash",
                        certificate.sha1
                    ),
                ),
            });
        }

        run(&[
            "delete-certificate",
            "-Z",
            &certificate.sha1,
            SYSTEM_KEYCHAIN,
        ])?;
        removed += 1;
    }

    if removed == 0 {
        return Ok(Change::Unchanged);
    }

    Ok(Change::Written {
        detail: format!(
            "removed MixEngine's certificate authority {key_id} from {SYSTEM_KEYCHAIN}"
        ),
    })
}

/// The certificate in a file whose name this process chose and whose directory only root can write.
///
/// **`tempfile` is a dev-dependency and stays one.** `security` needs a path, and the two ways to
/// give it one are a crate in a binary that runs as root or a directory this project already owns.
/// The audit directory is root-owned, already exists — the locks live in it — and gives the file a
/// fixed name, so nothing about this path comes from a request and no unprivileged account can
/// swap what is at it between the write and the read.
#[cfg(feature = "elevated")]
fn written(der: &[u8]) -> crate::Result<std::path::PathBuf> {
    let path = crate::elevated::audit_directory()?.join(HANDOFF_FILE);

    std::fs::write(&path, crate::trust::pem::encode(der)).map_err(|source| crate::Error::Io {
        action: "write the certificate for this machine's keychain",
        path: path.clone(),
        source,
    })?;

    Ok(path)
}

/// Run `security` with a fixed verb and this process's own file path, inside `caller`'s login
/// session — see [`apply`] for why that session and not this one.
///
/// **The uid is checked before it is placed on a command line**, exactly as [`is_hash`] checks
/// what `security` printed a moment ago: this binary validates what it is about to act on rather
/// than trusting where it came from. Digits are a uid; anything else is refused by name.
#[cfg(feature = "elevated")]
fn run_in_session(caller: &crate::elevated::Owner, arguments: &[&str]) -> crate::Result<()> {
    let uid = caller.id();

    if uid.is_empty() || !uid.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(crate::Error::Os {
            action: "change this machine's System keychain",
            source: std::io::Error::other(format!(
                "the caller's account {uid:?} is not a uid, so `security` cannot be run in its \
                 session"
            )),
        });
    }

    // **Before anything is written**: `add-trusted-cert` puts the certificate in the keychain and
    // then asks on screen, so a dialog with nowhere to appear leaves it there untrusted. A console
    // that cannot be read is asked anyway, as before this check existed.
    if let Ok(console) = std::fs::metadata("/dev/console") {
        use std::os::unix::fs::MetadataExt as _;

        if let Err(why) = crate::trust::screen::callers_screen(console.uid(), uid) {
            return Err(crate::Error::Os {
                action: "change this machine's System keychain",
                source: std::io::Error::other(why),
            });
        }
    }

    let mut line: Vec<&str> = vec!["asuser", uid, SECURITY];
    line.extend_from_slice(arguments);

    let output = command(
        LAUNCHCTL,
        &line,
        "run security to change the System keychain",
        DIALOG_PATIENCE,
    )?;

    if output.status.success() {
        return Ok(());
    }

    Err(refused(arguments, &output))
}

/// Run `security` with a fixed verb and this process's own file path.
#[cfg(feature = "elevated")]
fn run(arguments: &[&str]) -> crate::Result<()> {
    let output = security(arguments, "run security to change the System keychain")?;

    if output.status.success() {
        return Ok(());
    }

    Err(refused(arguments, &output))
}

/// What a `security` that exited non-zero said, as the error a person reads.
#[cfg(feature = "elevated")]
fn refused(arguments: &[&str], output: &std::process::Output) -> crate::Error {
    // The verb as well as the complaint. `security` says "The specified item could not be found in
    // the keychain" for several different requests, and which one was made is the half of that
    // sentence a person needs.
    let complaint = String::from_utf8_lossy(&output.stderr);
    let complaint = complaint.trim();

    // **The refusal this helper cannot get past on its own, named as such.** The admin trust
    // domain's authorization rule asks root to authenticate too, and a process behind the OS
    // elevation prompt has no window to do it in — so `security` reports that no user interaction
    // was possible, having already put the certificate into the keychain. What a person needs from
    // this message is the command that finishes the job from a terminal, where there is a window.
    let way_out = if complaint.contains("no user interaction was possible") {
        BY_HAND
    } else {
        ""
    };

    crate::Error::Os {
        action: "change this machine's System keychain",
        source: std::io::Error::other(format!(
            "`security {}` failed: {complaint}{way_out}",
            arguments.join(" "),
        )),
    }
}
