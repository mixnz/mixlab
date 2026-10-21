//! The queue of privileged operations, and what a client is told about it.
//!
//! The daemon batches everything that needs an administrative token and spends **one** prompt on the
//! whole of it — `docs/decisions/0005-on-demand-elevation.md` calls elevating inside a loop a
//! defect. What is here is the vocabulary that makes the waiting visible: what is in the queue, what
//! each operation will change, whether this machine can raise a prompt at all, and what the last
//! grant did.
//!
//! **Nothing here decides anything.** A declined prompt is a normal outcome, so a client renders a
//! pending list rather than an error, and the daemon is what knows the list is not empty.

use crate::privileged::{ElevationOutcome, PrivilegedOp};
use crate::{JobId, Timestamp};

/// One row of the queue, by its rowid.
///
/// A newtype rather than a bare `i64` on `docs/standards/rust.md`'s rule: the one method that
/// takes one is `elevation.drop`, and an integer there could be a job, an operation or a mistake.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PendingOpId(pub i64);

impl std::fmt::Display for PendingOpId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// One operation waiting for somebody to allow it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PendingOp {
    /// Which row, so `elevation.drop` can name one.
    pub id: PendingOpId,

    /// The operation itself, decoded. A row this build cannot decode is deleted rather than carried
    /// — see the T40b design, D2 — so this is never a value nobody can act on.
    pub op: PrivilegedOp,

    /// [`PrivilegedOp::describe`], rendered here rather than by the client.
    ///
    /// `CLAUDE.md`: a client renders what the daemon returns. It matters more here than anywhere
    /// else — this is the sentence somebody reads before allowing a change to a file outside their
    /// home — and a second client composing its own wording would be a second chance to get it
    /// wrong.
    pub description: String,

    /// When it was first asked for.
    ///
    /// **First** and not last: the same operation enqueued again keeps this reading (D2), so
    /// "pending since" says how long the machine has been missing it rather than how recently
    /// somebody noticed.
    pub requested_at: Timestamp,

    /// Why a grant raised from a terminal leaves this row waiting, when it does.
    ///
    /// The operating system's sentence: on macOS, adding MixEngine's authority to the System
    /// keychain asks on the Mac's own screen even under `sudo`. A grant raised at the screen still
    /// carries it. Absent rather than `null`, and absent from a daemon that predates it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs_the_screen: Option<String>,
}

/// The three facts `daemon.status` carries about elevation — the T40b design, D6.
///
/// *Degraded* is not a field here and is not a column anywhere: it means
/// [`pending`](ElevationSummary::pending) is not zero, computed where it is asked for. A second
/// representation of one fact is a second thing that can be wrong, and this one would be wrong in
/// the worst direction — reporting a healthy machine that is missing its hosts entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ElevationSummary {
    /// Is the **daemon itself** running with an administrative token?
    ///
    /// Reported rather than refused (D10): CI's Windows third runs the daemon suites under a full
    /// token, and a hard refusal would turn one platform red for a reason that has nothing to do
    /// with the code under test. What is worth saying about it is that every supervised service
    /// inherits that token.
    pub elevated: bool,

    /// Could a prompt be raised on this machine? `Elevation::probe` and a helper that is there.
    pub can_prompt: bool,

    /// How many operations are waiting. Not zero means degraded.
    pub pending: usize,
}

/// `elevation.status`, and what `elevation.drop` leaves behind.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ElevationStatus {
    /// As [`ElevationSummary::elevated`].
    pub elevated: bool,

    /// As [`ElevationSummary::can_prompt`].
    pub can_prompt: bool,

    /// Why not, when it cannot.
    ///
    /// On Linux it ends by naming what works instead — `mix elevation grant` from a terminal (ADR
    /// 0065) — so dropping it would leave a person with "unavailable" and nothing to type. Absent
    /// rather than `null` when there is nothing to say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// Where `mixengine-elevate` was found, when it was.
    ///
    /// A string and not a `PathBuf` for [`DaemonStatus`](crate::DaemonStatus)' reason: serde refuses
    /// a `PathBuf` that is not valid UTF-8, and an oddly named directory is a reason to see it
    /// spelled oddly rather than a reason for the call to fail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub helper: Option<String>,

    /// What the installed helper turned out to be, when there is one and it answered — roadmap
    /// task **T88a**.
    ///
    /// `default`, so a client built before this field existed still reads a newer daemon's answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_helper: Option<InstalledHelper>,

    /// Everything waiting, oldest first.
    pub pending: Vec<PendingOp>,

    /// What the most recent grant did, **for as long as this daemon has been up**.
    ///
    /// Deliberately not persisted: the durable fact is the queue, and a stored "you declined once"
    /// would outlive the reason it was true. A daemon that has just started answers [`None`], and
    /// the pending list already says everything a client needs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<GrantOutcome>,

    /// A grant waiting for somebody to run the helper from a terminal, while one is (ADR 0065).
    ///
    /// The command is here rather than only in the reply to the caller that started it, so that
    /// `mix elevation status` and MixLab can show it, and a second session can run it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<ByHand>,
}

/// A grant that wrote its request and is waiting for a terminal to run the helper on it.
///
/// Strings and not `PathBuf`s, for [`ElevationStatus::helper`]'s reason.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ByHand {
    /// The job that is waiting, and the one to cancel to stop waiting.
    pub job: JobId,

    /// The installed helper, absolute. `mix` runs no other, whatever this says.
    pub helper: String,

    /// The request the helper is to be given, absolute.
    pub request: String,
}

/// What the privileged helper installed on this machine turned out to be — roadmap task **T88a**.
///
/// Read by running that helper as an *ordinary* process with a `probe`, so nothing here costs a
/// prompt: `Probe` needs no administrative token, which is what the T40 design's D5 arranged.
/// Absent when nothing is installed, or when the machine would not answer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct InstalledHelper {
    /// Which release it is.
    pub version: String,

    /// The protocol it speaks, which is the ceiling every request to it is marked at.
    pub protocol: u32,

    /// Every operation it knows, by wire name — the evidence behind [`upgrade`](Self::upgrade).
    pub supported_ops: Vec<String>,

    /// What to do about it being older than this daemon, when it is.
    ///
    /// Rendered here rather than by a client, on [`PendingOp::description`]'s rule and for its
    /// reason: a client deciding what to say about the helper would be a client deciding what runs
    /// as root. Since T182b there is nothing for a person to run: the daemon replaces an older
    /// helper at the next prompt, and this says so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade: Option<String>,
}

/// What one grant did.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct GrantOutcome {
    /// The job that raised the prompt, so `job.status` can be asked about it afterwards.
    pub job: JobId,

    /// When it ended.
    pub at: Timestamp,

    /// What became of the prompt.
    ///
    /// `flatten`, on [`JobFinish`](crate::JobFinish)'s precedent: one object with one discriminator,
    /// rather than an `outcome` wrapper a client unwraps for this type and for nothing else. It also
    /// puts [`ElevationOutcome::Unavailable`]'s `reason` on the top level, where it reads as the
    /// sentence it is.
    #[serde(flatten)]
    pub outcome: ElevationOutcome,

    /// How many operations came back done — applied, or already so.
    pub applied: usize,

    /// How many are still in the queue afterwards.
    ///
    /// Not `pending.len()` minus `applied`: an operation the OS refused for a reason of its own is
    /// kept and one the helper refused is dropped, so the two numbers do not add up and pretending
    /// they do would make a client compute a third that is wrong.
    pub still_pending: usize,

    /// What every operation that did not come back done had to say, one sentence each.
    ///
    /// **Counts are not a diagnosis.** *0 applied, 1 still waiting* is true and there is nothing a
    /// person can do with it: the reason the helper gave lived in the audit log and in the daemon's
    /// own log, neither of which is in front of whoever just answered a password prompt. Measured on
    /// 2026-09-16, when one operation could never succeed on the machine it was queued on and was
    /// granted eight times against a sentence nobody had been shown.
    ///
    /// Both kinds are here — the refused, whose rows are gone, and the failed, whose rows stay for a
    /// retry — because the difference matters to the queue and not to the reader: either way this
    /// grant did not do it, and this is why.
    ///
    /// Optional on the wire, so an older client reading a newer daemon is unaffected
    /// ([ADR 0019](../../../docs/decisions/0019-an-added-response-member-is-optional.md)).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<String>,
}

/// `elevation.grant` — how the one prompt is to be raised.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ElevationGrant {
    /// Write the request and wait for a terminal to run the helper on it, instead of raising a
    /// dialog (ADR 0065).
    ///
    /// **Defaults to `false`**, the dialog `elevation.grant` always raised. Only a caller holding a
    /// terminal sends `true`; MixLab never does.
    #[serde(default)]
    pub by_hand: bool,

    /// That terminal is behind SSH, away from the machine's screen. With `by_hand`, the rows
    /// [`PendingOp::needs_the_screen`] marks stay queued: nobody there can answer the dialog they
    /// ask in. **Defaults to `false`**, a terminal at the screen.
    #[serde(default)]
    pub over_ssh: bool,
}

/// `elevation.drop` — forget one operation, or all of them.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ElevationDrop {
    /// Which one. [`None`] is every one of them.
    ///
    /// `deny_unknown_fields` above is what keeps that safe: a client that misspells this field would
    /// otherwise have asked to empty the queue and been obeyed.
    #[serde(default)]
    pub op: Option<PendingOpId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privileged::{ElevationOutcome, PrivilegedOp};
    use crate::{JobId, Timestamp};

    fn waiting() -> PendingOp {
        let op = PrivilegedOp::Probe {};

        PendingOp {
            id: PendingOpId(3),
            description: op.describe(),
            op,
            requested_at: Timestamp(1_760_000_000_000),
            needs_the_screen: None,
        }
    }

    /// A client built before `waiting` existed sent and read statuses without it.
    #[test]
    fn an_older_status_without_waiting_still_reads() {
        let encoded = serde_json::json!({
            "elevated": false,
            "can_prompt": true,
            "pending": [],
        });

        let status: ElevationStatus = serde_json::from_value(encoded).unwrap();

        assert_eq!(status.waiting, None);
    }

    /// Absent rather than `null`, as `reason` and `last` are.
    #[test]
    fn waiting_is_absent_not_null_when_none() {
        let encoded = serde_json::to_value(ElevationStatus {
            elevated: false,
            can_prompt: true,
            reason: None,
            helper: None,
            installed_helper: None,
            pending: Vec::new(),
            last: None,
            waiting: None,
        })
        .unwrap();

        assert!(encoded.get("waiting").is_none(), "{encoded}");
    }

    /// `elevation.grant` took no parameters before ADR 0065, and a client that still sends none
    /// asks for the dialog it always did.
    #[test]
    fn a_grant_with_no_params_is_not_by_hand() {
        assert!(!ElevationGrant::default().by_hand);
        assert!(
            !serde_json::from_value::<ElevationGrant>(serde_json::json!({}))
                .unwrap()
                .by_hand
        );
    }

    /// A misspelt `by_hand` must not quietly become a dialog nobody at this terminal can see.
    #[test]
    fn a_grant_refuses_a_field_it_does_not_know() {
        assert!(
            serde_json::from_value::<ElevationGrant>(serde_json::json!({ "byhand": true }))
                .is_err()
        );
    }

    /// A client renders the list without deciding what an operation means, which is what the
    /// pre-rendered `description` is for — `CLAUDE.md`'s "no business logic in clients", applied to
    /// the one screen where getting it wrong means somebody allows the wrong thing.
    #[test]
    fn a_pending_operation_carries_its_own_description() {
        let encoded = serde_json::to_value(waiting()).unwrap();

        assert_eq!(encoded["id"], 3);
        assert_eq!(encoded["op"]["op"], "probe");
        assert!(
            encoded["description"]
                .as_str()
                .is_some_and(|d| !d.is_empty())
        );
        assert_eq!(encoded["requested_at"], 1_760_000_000_000_i64);

        assert_eq!(
            serde_json::from_value::<PendingOp>(encoded).unwrap(),
            waiting()
        );
    }

    /// Flattened, on `JobFinish`'s precedent: one object with one discriminator, not an `outcome`
    /// wrapper a client has to unwrap for this type and for no other.
    #[test]
    fn a_grant_outcome_is_one_flat_object() {
        let grant = GrantOutcome {
            job: JobId(9),
            at: Timestamp(1_760_000_000_000),
            outcome: ElevationOutcome::Declined,
            applied: 0,
            still_pending: 3,
            problems: Vec::new(),
        };

        let encoded = serde_json::to_value(&grant).unwrap();
        assert_eq!(encoded["outcome"], "declined");
        assert_eq!(encoded["still_pending"], 3);
        assert!(encoded.get("reason").is_none());

        assert_eq!(
            serde_json::from_value::<GrantOutcome>(encoded).unwrap(),
            grant
        );
    }

    /// The Linux branch is the one this matters for: `reason` says what is missing and what works
    /// instead, and a flattening that lost it would leave a person with "unavailable".
    #[test]
    fn an_unavailable_grant_keeps_the_command_to_run_by_hand() {
        let grant = GrantOutcome {
            job: JobId(9),
            at: Timestamp(1_760_000_000_000),
            outcome: ElevationOutcome::Unavailable {
                reason: "no polkit agent; pkexec is not installed".to_owned(),
            },
            applied: 0,
            still_pending: 1,
            problems: Vec::new(),
        };

        let encoded = serde_json::to_value(&grant).unwrap();
        assert_eq!(encoded["outcome"], "unavailable");
        assert!(
            encoded["reason"]
                .as_str()
                .unwrap()
                .contains("no polkit agent")
        );

        assert_eq!(
            serde_json::from_value::<GrantOutcome>(encoded).unwrap(),
            grant
        );
    }

    /// A status with nothing waiting is the ordinary machine, and it puts no nulls on the wire.
    #[test]
    fn a_machine_with_nothing_waiting_says_so_without_nulls() {
        let status = ElevationStatus {
            elevated: false,
            can_prompt: true,
            reason: None,
            helper: Some("/opt/mixengine/mixengine-elevate".to_owned()),
            installed_helper: None,
            pending: Vec::new(),
            last: None,
            waiting: None,
        };

        let encoded = serde_json::to_value(&status).unwrap();
        assert_eq!(encoded["pending"], serde_json::json!([]));
        assert!(encoded.get("reason").is_none(), "{encoded}");
        assert!(encoded.get("last").is_none(), "{encoded}");

        assert_eq!(
            serde_json::from_value::<ElevationStatus>(encoded).unwrap(),
            status
        );
    }

    /// `elevation.drop` with no parameters means "all of them", and that has to be reachable from a
    /// client that sends `{}` as well as from one that sends nothing at all.
    #[test]
    fn a_drop_with_nothing_named_means_all_of_them() {
        assert_eq!(
            serde_json::from_str::<ElevationDrop>("{}").unwrap(),
            ElevationDrop { op: None }
        );
        assert_eq!(
            serde_json::from_str::<ElevationDrop>(r#"{"op":7}"#).unwrap(),
            ElevationDrop {
                op: Some(PendingOpId(7))
            }
        );
        // A typo in the one field this type has must not be read as "drop everything".
        assert!(serde_json::from_str::<ElevationDrop>(r#"{"id":7}"#).is_err());
    }

    /// T88a. The three facts a handshake found, and the sentence the daemon composed from them —
    /// `CLAUDE.md`'s "no business logic in clients", applied to the one screen that says what to do
    /// about the file this machine runs as root.
    #[test]
    fn an_installed_helper_carries_its_own_sentence() {
        let status = ElevationStatus {
            elevated: false,
            can_prompt: true,
            reason: None,
            helper: Some("/opt/mixengine/mixengine-elevate".to_owned()),
            installed_helper: Some(InstalledHelper {
                version: "0.1.0".to_owned(),
                protocol: 1,
                supported_ops: vec!["probe".to_owned()],
                upgrade: Some("run this release's installer".to_owned()),
            }),
            pending: Vec::new(),
            last: None,
            waiting: None,
        };

        let encoded = serde_json::to_value(&status).unwrap();
        assert_eq!(encoded["installed_helper"]["version"], "0.1.0");
        assert_eq!(encoded["installed_helper"]["protocol"], 1);
        assert!(
            encoded["installed_helper"]["upgrade"]
                .as_str()
                .is_some_and(|said| said.contains("installer"))
        );

        assert_eq!(
            serde_json::from_value::<ElevationStatus>(encoded).unwrap(),
            status
        );
    }

    /// A helper of this build has nothing to say about itself, and puts no null on the wire.
    #[test]
    fn an_installed_helper_with_nothing_to_report_says_nothing() {
        let helper = InstalledHelper {
            version: "0.2.0".to_owned(),
            protocol: 1,
            supported_ops: vec!["probe".to_owned(), "helper-replace".to_owned()],
            upgrade: None,
        };

        let encoded = serde_json::to_value(&helper).unwrap();
        assert!(encoded.get("upgrade").is_none(), "{encoded}");
    }

    #[test]
    fn a_summary_is_three_facts_and_no_more() {
        let summary = ElevationSummary {
            elevated: false,
            can_prompt: true,
            pending: 3,
        };

        assert_eq!(
            serde_json::to_string(&summary).unwrap(),
            r#"{"elevated":false,"can_prompt":true,"pending":3}"#
        );
    }
}
