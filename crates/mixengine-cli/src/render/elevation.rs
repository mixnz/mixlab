//! `mix elevation`: what waits for an administrator, and what a grant did.

use mixengine_proto::{ElevationStatus, GrantOutcome, privileged::ElevationOutcome};

/// `mix elevation status`, for a person.
///
/// **The list is the point, and it comes before any offer to raise a prompt.** T64 is what turns
/// this into the screen that explains every operation and what it will literally change *before*
/// somebody is asked to allow it; what is here already prints the operations' own descriptions,
/// because the daemon renders them and a client that composed its own would be composing the
/// sentence a person judges the change by.
pub(crate) fn elevation_status(status: &ElevationStatus) -> String {
    let mut rendered = waiting(status);

    if let Some(last) = &status.last {
        rendered.push_str(&format!("  last      {}\n", grant(last)));
    }

    if status.elevated {
        rendered.push_str(
            "  note      this daemon holds an administrative token; every service it supervises \
             inherits it\n",
        );
    }

    // T88a. The daemon composed this sentence, because what to do about an old helper differs by
    // *which* old helper it is — one that can replace itself is pointed at a command and one that
    // cannot is pointed at the installer — and choosing between those here would be a client
    // deciding what runs as root.
    if let Some(said) = status
        .installed_helper
        .as_ref()
        .and_then(|helper| helper.upgrade.as_ref())
    {
        rendered.push_str(&format!("  helper    {said}\n"));
    }

    // ADR 0065. Before the reason: a machine that cannot raise a dialog is exactly where a grant
    // waits, and what it waits for is the line a person can act on.
    if let Some(waiting) = &status.waiting {
        rendered.push_str(&format!(
            "  waiting   job {} waits for this to be run in a terminal:\n    {}\n",
            waiting.job,
            crate::by_hand::command(waiting)
        ));
    }

    match (&status.reason, status.pending.is_empty()) {
        // The reason is the answer, and on Linux it is a command to type. Printed whether or not
        // anything is waiting: a machine that cannot elevate is worth knowing about before the first
        // site is created rather than after.
        (Some(reason), _) => rendered.push_str(&format!("  cannot    {reason}\n")),

        (None, false) => rendered.push_str(
            "\n`mix elevation grant` asks once for all of them; `mix elevation drop` forgets one\n",
        ),

        (None, true) => {}
    }

    rendered
}

/// Everything that is waiting, and what each one will change: the part of the screen that is the
/// same whether it is being reported or being asked about.
///
/// The description is the operation's own — `PrivilegedOp::describe`, rendered by the daemon into
/// `PendingOp::description`. A client that composed its own sentence here would be composing the
/// one a person judges the change by, and it would be the sentence most able to disagree with what
/// is actually applied.
fn waiting(status: &ElevationStatus) -> String {
    let mut rendered = match status.pending.len() {
        0 => "nothing is waiting for permission\n".to_owned(),
        waiting => format!("{} for permission\n", operations(waiting)),
    };

    for pending in &status.pending {
        rendered.push_str(&format!(
            "  {:<4} {}: {}\n",
            pending.id,
            pending.op.name(),
            pending.description
        ));

        // The machine's sentence, under the row it is about.
        if let Some(reason) = &pending.needs_the_screen {
            rendered.push_str(&format!("       not over SSH: {reason}\n"));
        }
    }

    rendered
}

/// The screen `mix elevation grant` shows **before** it raises anything — roadmap task **T64**.
///
/// The same list [`elevation_status`] prints, and then the one thing that screen cannot say: a
/// prompt is about to appear, it will appear once, and it will name this program. What is
/// deliberately absent is the advice [`elevation_status`] ends with — a person reading this is
/// already running `mix elevation grant`.
pub(crate) fn elevation_prompt(status: &ElevationStatus) -> String {
    let helper = status.helper.as_deref().unwrap_or("mixengine-elevate");

    format!(
        "{}\nyour operating system will ask once, for all of them, to allow\n  {helper}\n",
        waiting(status)
    )
}

/// [`elevation_prompt`] for a grant raised from this terminal (ADR 0065): `sudo` asks, here.
///
/// `over_ssh` is whether this terminal is away from the machine's screen: only then do the rows
/// marked `needs_the_screen` stay out of the batch, and only then is that said.
pub(crate) fn elevation_prompt_by_hand(status: &ElevationStatus, over_ssh: bool) -> String {
    let helper = status.helper.as_deref().unwrap_or("mixengine-elevate");

    let mut rendered = format!(
        "{}\nsudo will ask once, in this terminal, for your password to run\n  {helper}\n",
        waiting(status)
    );

    // What the batch leaves out, said before anybody agrees to it.
    let left: Vec<&str> = status
        .pending
        .iter()
        .filter(|_| over_ssh)
        .filter_map(|pending| pending.needs_the_screen.as_deref())
        .collect();
    if let Some(reason) = left.first() {
        let stays = match left.len() {
            1 => "1 operation stays waiting".to_owned(),
            many => format!("{many} operations stay waiting"),
        };
        rendered.push_str(&format!("{stays}: {reason}\n"));
    }

    rendered
}

/// What one grant did, in a line — and under it, what anything that did not happen had to say.
///
/// **The counts alone are not a diagnosis.** *0 applied, 1 still waiting* is true and there is
/// nothing to do with it: until 2026-09-16 the reason an operation gave lived in the audit log and
/// the daemon's log, neither of which is in front of the person who has just typed a password. One
/// of them was granted eight times against a sentence nobody had been shown.
pub(super) fn grant(outcome: &GrantOutcome) -> String {
    let what = match &outcome.outcome {
        // A choice and not a failure — ADR 0005. The word carries that, and nothing here adds to it.
        ElevationOutcome::Declined => "declined".to_owned(),
        ElevationOutcome::Unavailable { reason } => format!("could not be raised: {reason}"),
        ElevationOutcome::Completed => format!(
            "{} applied, {} still waiting",
            outcome.applied, outcome.still_pending
        ),
    };

    let mut rendered = format!("job {}: {what}", outcome.job);

    // The daemon's own sentences, one per line, unchanged. Which operation each is about is already
    // in the sentence, because the daemon put it there.
    for problem in &outcome.problems {
        rendered.push_str(&format!("\n  {problem}"));
    }

    rendered
}

/// The same, for the queue of privileged operations.
pub(super) fn operations(count: usize) -> String {
    match count {
        1 => "1 operation is waiting".to_owned(),
        many => format!("{many} operations are waiting"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_pending_probe(id: i64) -> mixengine_proto::PendingOp {
        let op = mixengine_proto::privileged::PrivilegedOp::Probe {};

        mixengine_proto::PendingOp {
            id: mixengine_proto::PendingOpId(id),
            description: op.describe(),
            op,
            requested_at: mixengine_proto::Timestamp(1_760_000_000_000),
            needs_the_screen: None,
        }
    }

    /// The screen T64 will build on: every operation, and what each will literally change, before
    /// anybody raises a prompt.
    #[test]
    fn a_pending_list_says_what_each_operation_will_change() {
        let rendered = elevation_status(&ElevationStatus {
            elevated: false,
            can_prompt: true,
            reason: None,
            helper: Some("/opt/mixengine/mixengine-elevate".to_owned()),
            installed_helper: None,
            pending: vec![a_pending_probe(1), a_pending_probe(2)],
            last: None,
            waiting: None,
        });

        assert!(rendered.contains("2 operations are waiting"), "{rendered}");
        assert!(rendered.contains("mix elevation grant"), "{rendered}");
        assert!(
            rendered.contains(&mixengine_proto::privileged::PrivilegedOp::Probe {}.describe()),
            "{rendered}"
        );
    }

    /// T64's screen: the same list, and one sentence about the prompt that is about to be raised.
    ///
    /// The assertion that matters is the negative one. `mix elevation status` ends by telling a
    /// person to run `mix elevation grant`; this is printed *by* that command, so repeating the
    /// advice would be telling somebody to run what they are already running.
    #[test]
    fn the_screen_before_a_prompt_is_the_list_and_what_the_prompt_will_be() {
        let rendered = elevation_prompt(&ElevationStatus {
            elevated: false,
            can_prompt: true,
            reason: None,
            helper: Some("/opt/mixengine/mixengine-elevate".to_owned()),
            installed_helper: None,
            pending: vec![a_pending_probe(1), a_pending_probe(2)],
            last: None,
            waiting: None,
        });

        assert!(rendered.contains("2 operations are waiting"), "{rendered}");
        assert!(
            rendered.contains(&mixengine_proto::privileged::PrivilegedOp::Probe {}.describe()),
            "{rendered}"
        );
        assert!(rendered.contains("once"), "{rendered}");
        assert!(!rendered.contains("mix elevation grant"), "{rendered}");
    }

    /// T88a. The sentence about an old helper comes from the daemon, so what this asserts is that
    /// it reaches the screen — not what it says.
    #[test]
    fn an_old_privileged_helper_is_reported_where_the_queue_is() {
        let rendered = elevation_status(&ElevationStatus {
            elevated: false,
            can_prompt: true,
            reason: None,
            helper: Some("/opt/mixengine/mixengine-elevate".to_owned()),
            installed_helper: Some(mixengine_proto::InstalledHelper {
                version: "0.1.0".to_owned(),
                protocol: 1,
                supported_ops: vec!["probe".to_owned()],
                upgrade: Some("run this release's installer".to_owned()),
            }),
            pending: Vec::new(),
            last: None,
            waiting: None,
        });

        assert!(rendered.contains("helper"), "{rendered}");
        assert!(
            rendered.contains("run this release's installer"),
            "{rendered}"
        );
    }

    /// ADR 0065: while a grant waits for a terminal, the command is on the screen for anybody who
    /// looks — a second SSH session can run it.
    #[test]
    fn a_waiting_grant_shows_the_command() {
        let rendered = elevation_status(&ElevationStatus {
            elevated: false,
            can_prompt: false,
            reason: Some("no polkit agent".to_owned()),
            helper: Some("/usr/local/libexec/mixengine/mixengine-elevate".to_owned()),
            installed_helper: None,
            pending: vec![a_pending_probe(1)],
            last: None,
            waiting: Some(mixengine_proto::ByHand {
                job: mixengine_proto::JobId(4),
                helper: "/usr/local/libexec/mixengine/mixengine-elevate".to_owned(),
                request: "/home/me/.mixengine/run/elevate/abc/request.json".to_owned(),
            }),
        });

        assert!(rendered.contains("job #4"), "{rendered}");
        assert!(
            rendered.contains(
                "sudo -- /usr/local/libexec/mixengine/mixengine-elevate \
                 /home/me/.mixengine/run/elevate/abc/request.json"
            ),
            "{rendered}"
        );
    }

    fn a_probe_for_the_screen(id: i64) -> mixengine_proto::PendingOp {
        mixengine_proto::PendingOp {
            needs_the_screen: Some("it needs this Mac's own screen".to_owned()),
            ..a_pending_probe(id)
        }
    }

    fn waiting_on(pending: Vec<mixengine_proto::PendingOp>) -> ElevationStatus {
        ElevationStatus {
            elevated: false,
            can_prompt: true,
            reason: None,
            helper: Some("/Library/PrivilegedHelperTools/dev.mixengine.elevate".to_owned()),
            installed_helper: None,
            pending,
            last: None,
            waiting: None,
        }
    }

    /// The row says why a terminal leaves it, under the row it is about.
    #[test]
    fn status_says_which_row_a_terminal_cannot_grant() {
        let rendered = elevation_status(&waiting_on(vec![a_probe_for_the_screen(1)]));

        assert!(
            rendered.contains("it needs this Mac's own screen"),
            "{rendered}"
        );
    }

    /// Said before `continue? [y/N]`, so nobody agrees to a batch that is smaller than the list.
    #[test]
    fn the_by_hand_prompt_says_what_stays_waiting() {
        let rendered = elevation_prompt_by_hand(
            &waiting_on(vec![a_probe_for_the_screen(1), a_pending_probe(2)]),
            true,
        );

        assert!(
            rendered.contains("1 operation stays waiting: it needs this Mac's own screen"),
            "{rendered}"
        );
    }

    /// `sudo` in a terminal at the screen reaches the dialog it asks in, so nothing stays.
    #[test]
    fn a_terminal_at_the_screen_says_nothing_stays_waiting() {
        let rendered =
            elevation_prompt_by_hand(&waiting_on(vec![a_probe_for_the_screen(1)]), false);

        assert!(!rendered.contains("stays waiting"), "{rendered}");
    }

    /// The label says where it cannot be done, not that terminals cannot do it: one at the screen
    /// can.
    #[test]
    fn the_row_says_it_is_not_over_ssh() {
        let rendered = elevation_status(&waiting_on(vec![a_probe_for_the_screen(1)]));

        assert!(rendered.contains("not over SSH: it needs"), "{rendered}");
    }

    /// A dialog at the screen carries it, so there is nothing to say there.
    #[test]
    fn the_dialog_prompt_says_nothing_stays_waiting() {
        let rendered = elevation_prompt(&waiting_on(vec![a_probe_for_the_screen(1)]));

        assert!(!rendered.contains("stays waiting"), "{rendered}");
    }

    /// From a terminal it is `sudo` that asks, here, and not a dialog somewhere else.
    #[test]
    fn the_by_hand_prompt_says_sudo_will_ask_here() {
        let rendered = elevation_prompt_by_hand(
            &ElevationStatus {
                elevated: false,
                can_prompt: false,
                reason: None,
                helper: Some("/usr/local/libexec/mixengine/mixengine-elevate".to_owned()),
                installed_helper: None,
                pending: vec![a_pending_probe(1)],
                last: None,
                waiting: None,
            },
            true,
        );

        assert!(rendered.contains("sudo"), "{rendered}");
        assert!(rendered.contains("this terminal"), "{rendered}");
        assert!(
            !rendered.contains("operating system will ask"),
            "{rendered}"
        );
    }

    /// And a helper this build is happy with says nothing at all, which is the ordinary machine.
    #[test]
    fn a_current_privileged_helper_is_not_mentioned() {
        let rendered = elevation_status(&ElevationStatus {
            elevated: false,
            can_prompt: true,
            reason: None,
            helper: Some("/opt/mixengine/mixengine-elevate".to_owned()),
            installed_helper: Some(mixengine_proto::InstalledHelper {
                version: "0.2.0".to_owned(),
                protocol: 1,
                supported_ops: vec!["probe".to_owned(), "helper-replace".to_owned()],
                upgrade: None,
            }),
            pending: Vec::new(),
            last: None,
            waiting: None,
        });

        assert!(!rendered.contains("helper"), "{rendered}");
    }

    /// A machine that cannot prompt has to print the reason, because on Linux the reason says what
    /// is missing and what works instead.
    #[test]
    fn a_machine_that_cannot_prompt_prints_what_to_do_instead() {
        let rendered = elevation_status(&ElevationStatus {
            elevated: false,
            can_prompt: false,
            reason: Some("no polkit agent; pkexec is not installed".to_owned()),
            helper: Some("/opt/mixengine/mixengine-elevate".to_owned()),
            installed_helper: None,
            pending: vec![a_pending_probe(1)],
            last: None,
            waiting: None,
        });

        assert!(rendered.contains("no polkit agent"), "{rendered}");
        assert!(
            !rendered.contains("mix elevation grant"),
            "offering a command that cannot work: {rendered}"
        );
    }

    /// A decline is a normal outcome, so it reads as one — and the list stays, which is the whole of
    /// the degraded mode a person sees.
    #[test]
    fn a_declined_grant_reads_as_a_choice_rather_than_a_failure() {
        let rendered = elevation_status(&ElevationStatus {
            elevated: false,
            can_prompt: true,
            reason: None,
            helper: Some("/opt/mixengine/mixengine-elevate".to_owned()),
            installed_helper: None,
            pending: vec![a_pending_probe(1)],
            last: Some(mixengine_proto::GrantOutcome {
                job: mixengine_proto::JobId(4),
                at: mixengine_proto::Timestamp(1_760_000_000_000),
                outcome: mixengine_proto::privileged::ElevationOutcome::Declined,
                applied: 0,
                still_pending: 1,
                problems: Vec::new(),
            }),
            waiting: None,
        });

        assert!(rendered.contains("declined"), "{rendered}");
        assert!(!rendered.to_lowercase().contains("error"), "{rendered}");
    }

    /// **A grant that did nothing says why** — roadmap task **T147**.
    ///
    /// The regression this pins is not a crash. It is a person granting the same operation eight
    /// times because the only thing they were shown was *0 applied, 1 still waiting*, while the
    /// sentence that would have stopped them sat in a log file.
    #[test]
    fn what_did_not_happen_is_printed_under_the_line() {
        let outcome = GrantOutcome {
            job: mixengine_proto::JobId(8),
            at: mixengine_proto::Timestamp(1_760_000_000_000),
            outcome: mixengine_proto::privileged::ElevationOutcome::Completed,
            applied: 0,
            still_pending: 1,
            problems: vec![
                "helper-install: cannot read /Volumes/SSD/mixengine-elevate: Operation not \
                 permitted (os error 1)"
                    .to_owned(),
            ],
        };

        let rendered = grant(&outcome);

        assert!(
            rendered.contains("0 applied, 1 still waiting"),
            "{rendered}"
        );
        assert!(rendered.contains("helper-install"), "{rendered}");
        assert!(rendered.contains("Operation not permitted"), "{rendered}");
    }

    /// A grant with nothing to report is the line it always was, with nothing appended.
    #[test]
    fn a_grant_that_did_everything_gains_no_lines() {
        let outcome = GrantOutcome {
            job: mixengine_proto::JobId(1),
            at: mixengine_proto::Timestamp(1_760_000_000_000),
            outcome: mixengine_proto::privileged::ElevationOutcome::Completed,
            applied: 2,
            still_pending: 0,
            problems: Vec::new(),
        };

        assert_eq!(grant(&outcome), "job #1: 2 applied, 0 still waiting");
    }
}
