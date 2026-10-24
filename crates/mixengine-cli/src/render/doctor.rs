//! `mix doctor`: the report, `--repair` and `--bundle`.

use super::size::size;
use mixengine_proto::{Action, BundleReport, DoctorReport, Outcome, RepairReport};

/// `daemon.doctor`, as a person reads it.
///
/// **Every check gets a line, including the ones that found nothing.** A doctor that printed only
/// faults would leave a person unsure it looked, which is the whole reason the report carries what
/// was examined rather than only what was wrong.
///
/// The word in the margin is the outcome and the indented line under it is the daemon's own
/// sentence — the daemon's `because` never carries advice, so the one line telling a person what to
/// do about a `PROBLEM` is this client's own, appended once for the whole report rather than
/// repeated under every line that earned it.
pub(crate) fn doctor(report: &DoctorReport) -> String {
    let mut out = String::new();

    for check in &report.checks {
        let (mark, because) = match &check.outcome {
            Outcome::Ok {} => ("ok     ", None),
            Outcome::Note { because } => ("note   ", Some(because)),
            Outcome::Problem { because, .. } => ("PROBLEM", Some(because)),
            Outcome::Skipped { because } => ("skipped", Some(because)),
        };

        out.push_str(&format!("{mark}  {}\n", check.name));

        if let Some(because) = because {
            out.push_str(&format!("         {because}\n"));
        }
    }

    if report.has_a_problem() {
        out.push_str("\nrun `mix doctor --repair` to fix what it can\n");
    }

    out
}

/// `daemon.doctor_repair`, as a person reads it.
///
/// **The same three margins as [`doctor`]**, so the two read as one tool: what was done, what is
/// waiting, and what nothing could be done about. A `PROBLEM` here means the same thing it means
/// there — something is wrong and this build cannot fix it.
///
/// A repair that found nothing prints a sentence rather than nothing at all, for `doctor`'s reason
/// one document along: silence cannot be told apart from a command that did not run.
pub(crate) fn repair(report: &RepairReport) -> String {
    if report.actions.is_empty() {
        return "nothing to repair\n".to_owned();
    }

    let mut out = String::new();

    for action in &report.actions {
        let (mark, sentence) = match &action.outcome {
            Action::Repaired { what } => ("repaired", what),
            Action::Enqueued { what } => ("waiting ", what),
            Action::Untouched { because } => ("PROBLEM ", because),
        };

        out.push_str(&format!("{mark}  {}\n", action.name));
        out.push_str(&format!("          {sentence}\n"));
    }

    out
}

/// `daemon.bundle`, as a person reads it.
///
/// **The omissions are printed and not summarised.** They are the half a person will not otherwise
/// know to ask about, and a bundle whose gaps live only in a JSON field is a bundle whose gaps get
/// discovered by whoever opens it three days later, looking for the file that is not there.
pub(crate) fn bundle(report: &BundleReport, copied_to: Option<&std::path::Path>) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "wrote  {}
",
        report.path
    ));
    out.push_str(&format!(
        "       {}, {} file(s)
",
        size(report.bytes),
        report.members.len()
    ));

    if let Some(destination) = copied_to {
        out.push_str(&format!(
            "copied {}
",
            destination.display()
        ));
    }

    if !report.omitted.is_empty() {
        out.push_str(
            "
not included
",
        );
        for left in &report.omitted {
            out.push_str(&format!(
                "       {}: {}
",
                left.name, left.because
            ));
        }
    }

    out
}
