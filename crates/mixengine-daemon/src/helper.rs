//! What the privileged helper installed on this machine is, and what to say when it is older than
//! the daemon reading it — roadmap task **T88a**.
//!
//! # Asked without a prompt
//!
//! `Probe` needs no administrative token — the T40 design's D5 made it so, precisely so that the
//! operation reporting whether a process is elevated could ever report `false`. So the daemon finds
//! out what the installed helper is by *running it as an ordinary process*, with a one-operation
//! batch, and reading the header every answer already carries. No prompt is spent and nothing is
//! changed.
//!
//! **Only when a helper is installed.** On a fresh machine and in every development tree there is
//! nothing to ask, `Elevation::require_helper` enqueues `HelperInstall {}` as it always has, and
//! this costs nothing at all.
//!
//! # And what it is for
//!
//! Two things, and neither is speculative. The daemon marks each request with the lower of its own
//! protocol and the helper's, because a fixed old binary cannot be taught a newer one. And it reads
//! `supported_ops` before deciding what to say about an old helper: without it, the only way to
//! discover that the installed helper predates `helper-replace` is to enqueue one, spend a prompt,
//! and be answered `Unsupported` — which deletes the row (the T40b design, D5) and leaves a person
//! with a refusal and no sentence.

use std::path::Path;

use mixengine_proto::privileged::PrivilegedOp;
use mixengine_proto::{PendingOp, PendingOpId, ProtocolVersion, Timestamp};

/// How long the installed helper gets to answer a probe.
///
/// It reads two files and writes a small one. Generous enough for a cold start off a slow disk,
/// short enough that a daemon start is never held up by a binary that will not run.
const HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// What the installed helper answered a probe with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HelperFacts {
    /// The protocol it speaks, which is the ceiling every request to it is marked at.
    pub(crate) speaks: ProtocolVersion,

    /// Which release it is.
    pub(crate) version: String,

    /// Every operation it knows, by wire name.
    pub(crate) supported_ops: Vec<String>,
}

impl HelperFacts {
    /// Does this helper know how to replace itself?
    pub(crate) fn can_replace_itself(&self) -> bool {
        self.supported_ops
            .iter()
            .any(|name| name == PrivilegedOp::HelperReplace {}.name())
    }
}

/// Run `helper` as an ordinary process and read what it says about itself.
///
/// Every failure is [`None`]: an unreadable answer, a binary that will not start, a timeout, and
/// the `EXIT_UNAVAILABLE` a helper answers while a grant holds `elevate.lock` all mean the same
/// thing to every caller, which is that this daemon does not know and will not guess.
pub(crate) async fn handshake(helper: &Path, home: &Path, elevate: &Path) -> Option<HelperFacts> {
    let directory = elevate.join(format!("probe-{}", std::process::id()));
    let facts = probe(helper, home, &directory).await;

    // On every branch, as `Elevation::grant` removes its own: a single-use directory that outlived
    // its use is what makes `response.json`'s existence a weaker anti-replay check than it is.
    let _ = tokio::fs::remove_dir_all(&directory).await;

    facts
}

/// The body of [`handshake`], so the cleanup above happens whatever this answers.
async fn probe(helper: &Path, home: &Path, directory: &Path) -> Option<HelperFacts> {
    let op = PrivilegedOp::Probe {};
    let waiting = PendingOp {
        id: PendingOpId(0),
        description: op.describe(),
        op,
        requested_at: Timestamp::from_system_time(std::time::SystemTime::now()),
    };

    // **At the floor, and not at this build's own protocol.** This is the one request sent before
    // anything is known about the peer, so it is marked with the version every build that will ever
    // exist still serves — which is what makes a handshake work against a helper older than the
    // daemon, and is the whole reason there is a floor.
    let request = mixengine_core::elevation::write_request(
        directory,
        home,
        std::slice::from_ref(&waiting),
        mixengine_proto::PROTOCOL_MINIMUM,
    )
    .ok()?;

    let mut probing = tokio::process::Command::new(helper);
    probing
        .arg(request.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    // The helper is a console program and this daemon has no console, so without this Windows
    // makes one for it — a terminal window on the desktop, on every daemon start.
    mixengine_platform::process::without_a_window(probing.as_std_mut());

    let ran = tokio::time::timeout(HANDSHAKE_TIMEOUT, probing.status()).await;

    if !matches!(ran, Ok(Ok(status)) if status.success()) {
        tracing::debug!(
            helper = %helper.display(),
            "the installed privileged helper did not answer a probe"
        );
        return None;
    }

    let report = mixengine_core::elevation::read_report(&request).ok()?;

    Some(HelperFacts {
        speaks: report.version,
        version: report.elevate_version,
        supported_ops: report.supported_ops,
    })
}

/// What to tell somebody about a helper older than this release's, or [`None`].
///
/// **One sentence, and no command in it** — the T182b design, D2. The daemon replaces an older
/// helper by itself at the next elevation prompt, whichever kind of old it is, so there is nothing
/// for a person to run and nothing to choose between.
///
/// A helper *newer* than this release's is [`None`] too: that is routine, and is the reason the
/// response was written without `deny_unknown_fields` in the first place. A version either side
/// cannot parse is [`None`]: a sentence about a string nobody can compare helps nobody.
pub(crate) fn upgrade_sentence(facts: &HelperFacts, ours: &str) -> Option<String> {
    use mixengine_proto::PackageVersion;

    let installed = PackageVersion::parse(facts.version.clone()).ok()?;
    let current = PackageVersion::parse(ours.to_owned()).ok()?;

    if installed.cmp_precedence(&current) != std::cmp::Ordering::Less {
        return None;
    }

    Some(format!(
        "the privileged helper on this machine is {}, and this release's is {ours}. It keeps \
         serving everything it knows, and MixEngine replaces it at the next permission prompt",
        facts.version
    ))
}

/// What the start of a daemon does about the installed helper — the T182b design, D2's table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Row {
    /// The installed helper is `HELPER_VERSION`.
    Current,

    /// It is newer: a person went back a release. Nothing is downgraded.
    Newer,

    /// None is installed. `Elevation::require_helper` has already queued the install.
    Install,

    /// Older, and it knows `helper-replace`: this release's signed helper replaces it.
    Replace,

    /// Older, and it does not: the shipped copy installs itself over it (D3).
    InstallOverOld,

    /// A helper is installed and would not say what it is, so nothing is decided about it.
    Unknown,
}

/// D2's table, over facts rather than a machine, so every row is a unit test.
pub(crate) fn row(installed: bool, facts: Option<&HelperFacts>) -> Row {
    use mixengine_proto::PackageVersion;

    let Some(facts) = facts else {
        return match installed {
            true => Row::Unknown,
            false => Row::Install,
        };
    };

    let ordering = PackageVersion::parse(facts.version.clone())
        .ok()
        .zip(PackageVersion::parse(mixengine_proto::privileged::HELPER_VERSION.to_owned()).ok())
        .map(|(here, ours)| here.cmp_precedence(&ours));

    match ordering {
        Some(std::cmp::Ordering::Equal) => Row::Current,
        Some(std::cmp::Ordering::Greater) => Row::Newer,
        // A version that does not parse is older than any this release could have written.
        Some(std::cmp::Ordering::Less) | None => match facts.can_replace_itself() {
            true => Row::Replace,
            false => Row::InstallOverOld,
        },
    }
}

/// Keep the installed helper in step with this release — the T182b design, D2.
///
/// **Queued, not prompted**: the operation joins the next grant, first in its batch. The one
/// exception is the first start after an update that changed the helper, which raises the prompt
/// itself, once per `HELPER_VERSION`, because the person has just asked for something.
///
/// Spawned at every start and never fails it: a machine that cannot fetch this release's helper
/// today (offline, a feed that will not answer) is asked again at the next start.
pub(crate) async fn keep_in_step(
    elevation: &std::sync::Arc<crate::elevation::Elevation>,
    updates: &std::sync::Arc<crate::updates::Updates>,
    paths: &mixengine_core::paths::Paths,
) -> Row {
    let facts = elevation.facts();
    let decided = row(elevation.helper_is_installed(), facts.as_ref());

    let queued = match decided {
        Row::Replace => match stage_this_release(updates, paths).await {
            Ok(()) => elevation.enqueue(&PrivilegedOp::HelperReplace {}).await,
            Err(reason) => {
                tracing::info!(%reason, "the privileged helper stays as it is until the next start");
                return decided;
            }
        },
        Row::InstallOverOld => elevation.enqueue(&PrivilegedOp::HelperInstall {}).await,
        Row::Current | Row::Newer | Row::Install | Row::Unknown => return decided,
    };

    if let Err(error) = queued {
        tracing::warn!(%error, "could not queue the privileged helper's replacement");
        return decided;
    }

    if elevation
        .first_prompt_for(mixengine_proto::privileged::HELPER_VERSION)
        .await
        && let Err(error) = elevation.grant(crate::elevation::Route::Dialog).await
    {
        tracing::info!(%error, "the helper's replacement waits for the next prompt");
    }

    decided
}

/// Fetch, verify and smoke-test this release's signed helper into the candidate directory — the
/// half of T88a's `upgrade` that stops before anything is queued.
///
/// # Errors
///
/// A sentence for a copy whose helper a package manages, a feed that will not answer, a published
/// helper that is not this release's, one that did not verify, and one that will not start here.
async fn stage_this_release(
    updates: &std::sync::Arc<crate::updates::Updates>,
    paths: &mixengine_core::paths::Paths,
) -> Result<(), String> {
    if !matches!(
        updates.placement(),
        mixengine_core::updates::Placement::SelfUpdatable { .. }
    ) {
        return Err("a package manages this copy's helper".to_owned());
    }

    let (offered, artifact) = updates
        .published_helper()
        .await
        .map_err(|error| mixengine_proto::flatten(&error))?;

    if offered != mixengine_proto::privileged::HELPER_VERSION {
        return Err(format!(
            "the feed offers helper {offered}, and this release is {}",
            mixengine_proto::privileged::HELPER_VERSION
        ));
    }

    let into = mixengine_proto::privileged::helper_candidate_dir(paths.root());
    mixengine_core::updates::helper::stage(
        updates.installer(),
        &artifact,
        mixengine_core::updates::PUBLIC_KEY,
        &into,
    )
    .await
    .map_err(|error| error.to_string())?;

    let candidate = mixengine_proto::privileged::helper_candidate(paths.root());
    match handshake(&candidate, paths.root(), &paths.run().join("elevate")).await {
        Some(_) => Ok(()),
        None => Err("this release's helper will not run on this machine".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(version: &str, knows_replace: bool) -> HelperFacts {
        HelperFacts {
            speaks: mixengine_proto::PROTOCOL_VERSION,
            version: version.to_owned(),
            supported_ops: if knows_replace {
                vec!["probe".to_owned(), "helper-replace".to_owned()]
            } else {
                vec!["probe".to_owned()]
            },
        }
    }

    /// T182b, D2: every row of the table, from the facts a probe answers.
    #[test]
    fn every_row_of_the_table_is_decided_from_what_the_probe_said() {
        use mixengine_proto::privileged::HELPER_VERSION;

        assert_eq!(row(false, None), Row::Install, "nothing installed");
        assert_eq!(row(true, None), Row::Unknown, "installed, and silent");
        assert_eq!(row(true, Some(&facts(HELPER_VERSION, true))), Row::Current);
        assert_eq!(
            row(true, Some(&facts("9.9.9", true))),
            Row::Newer,
            "never downgraded"
        );
        assert_eq!(row(true, Some(&facts("0.0.7", true))), Row::Replace);
        assert_eq!(
            row(true, Some(&facts("0.1.0", false))),
            Row::InstallOverOld,
            "the stray 0.1.0 knows no helper-replace"
        );
        assert_eq!(
            row(true, Some(&facts("not a version", false))),
            Row::InstallOverOld,
            "a version that does not parse is older than any this release wrote"
        );
    }

    #[test]
    fn a_helper_of_this_build_needs_nothing_said_about_it() {
        assert!(upgrade_sentence(&facts("0.2.0", true), "0.2.0").is_none());
    }

    /// A helper *newer* than the daemon is routine — the response was written without
    /// `deny_unknown_fields` for exactly that — so it is not something to report either.
    #[test]
    fn a_helper_newer_than_this_daemon_needs_nothing_said_about_it() {
        assert!(upgrade_sentence(&facts("0.3.0", true), "0.2.0").is_none());
    }

    /// T182b, D2. Whichever kind of old it is, the sentence says the daemon replaces it, and names
    /// no command: there is none left to run.
    #[test]
    fn an_older_helper_is_said_to_be_replaced_at_the_next_prompt() {
        for knows_replace in [true, false] {
            let said =
                upgrade_sentence(&facts("0.1.0", knows_replace), "0.2.0").expect("a sentence");

            assert!(said.contains("0.1.0"), "{said}");
            assert!(said.contains("next permission prompt"), "{said}");
            assert!(!said.contains("mix elevation"), "{said}");
        }
    }

    /// A version neither side can parse is not a reason to tell somebody to reinstall.
    #[test]
    fn a_version_that_is_not_one_says_nothing() {
        assert!(upgrade_sentence(&facts("not a version", true), "0.2.0").is_none());
    }

    /// The operation list is read by name, and the name is the wire tag rather than a second
    /// spelling this module invented.
    #[test]
    fn a_helper_that_knows_the_operation_is_the_one_that_names_it() {
        assert!(facts("0.2.0", true).can_replace_itself());
        assert!(!facts("0.2.0", false).can_replace_itself());
    }
}
