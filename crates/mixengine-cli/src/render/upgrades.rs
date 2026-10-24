//! What an update will do or did, for a runtime and a package alike.

use super::prerequisites::requirements;
use super::size::size;
use mixengine_proto::{OldVersion, UpgradeItem, UpgradeOutcome, UpgradePlan};

/// What an update will do or did, for a person.
///
/// **One rendering for the plan and for the result**, as the type is one: each line gains how it
/// went once the job has run.
pub(crate) fn upgrade_plan(plan: &UpgradePlan) -> String {
    let mut said = format!("{} {} → {}", plan.subject, plan.from, plan.to);
    said.push_str(&match plan.to_installed {
        true => ", already installed\n".to_owned(),
        false => format!(", {} to download\n", size(plan.bytes)),
    });

    if plan.stale {
        said.push_str(
            "this is judged against a cached index; mixengined could not reach the package index\n",
        );
    }
    if !plan.needs.is_empty() {
        said.push_str(&requirements(&plan.needs));
    }

    for entry in &plan.entries {
        said.push_str(&format!(
            "  - {}{}\n",
            upgrade_item(plan, &entry.item),
            upgrade_outcome(&entry.outcome)
        ));
    }

    let (subject, from) = (&plan.subject, &plan.from);
    said.push_str(&match &plan.old {
        OldVersion::WillBeRemoved {} => {
            format!("{subject} {from} will be removed afterwards; --keep keeps it\n")
        }
        OldVersion::WillBeKept { because } => {
            format!("{subject} {from} will be kept: {}\n", because.join("; "))
        }
        OldVersion::Removed {} => format!("{subject} {from} was removed\n"),
        OldVersion::Kept { because } => {
            format!("{subject} {from} was kept: {}\n", because.join("; "))
        }
    });

    said
}

/// One entry of a plan, as a sentence.
fn upgrade_item(plan: &UpgradePlan, item: &UpgradeItem) -> String {
    let (subject, old, new) = (&plan.subject, &plan.from, &plan.to);
    match item {
        UpgradeItem::Default {} => format!("{new} becomes the default {subject}"),
        UpgradeItem::Site { site, from, to } => format!("{site} moves from {from} to {to}"),
        UpgradeItem::ExtensionPool {
            pool, moves: true, ..
        } => {
            format!("{pool} moves to {subject} {new}")
        }
        UpgradeItem::ExtensionPool {
            pool,
            moves: false,
            requires: Some(requires),
        } => format!("{pool} stays on {subject} {old}: it requires {requires}"),
        UpgradeItem::ExtensionPool {
            pool,
            moves: false,
            requires: None,
        } => format!("{pool} stays on {subject} {old}"),
        UpgradeItem::DroppedExtension { name } => {
            format!("{name} is not in {new}'s build, so it will be off")
        }
        UpgradeItem::Pin { project, from, to } => format!("{project}'s pin {from} becomes {to}"),
        UpgradeItem::Manifest {
            project,
            path,
            constraint,
        } => format!("{project} pins {subject} {constraint} in {path}, which only {old} answers"),
        UpgradeItem::Tool { name } => {
            format!("{name} is installed only in {old}; install it again under {new}")
        }
        UpgradeItem::Instance {
            service,
            restarts: true,
        } => format!("{service} is stopped, moved to {new} and started again"),
        UpgradeItem::Instance {
            service,
            restarts: false,
        } => format!("{service} moves to {new} and stays stopped"),
        UpgradeItem::FrontEndRestart { service } => format!(
            "{service} restarts, so no site answers for a moment; this machine may ask whether \
             {new} may answer on 80 and 443"
        ),
        UpgradeItem::NoDowngrade { service } => {
            format!("{service}: once MySQL {new} has opened its data, {old} cannot open it again")
        }
    }
}

/// How one entry went, as a suffix; nothing for a plan.
fn upgrade_outcome(outcome: &UpgradeOutcome) -> String {
    match outcome {
        UpgradeOutcome::Planned {} => String::new(),
        UpgradeOutcome::Done {} => " (done)".to_owned(),
        UpgradeOutcome::Skipped {} => " (not done)".to_owned(),
        UpgradeOutcome::Failed { because } => format!(" (failed: {because})"),
    }
}

#[cfg(test)]
mod tests {
    use mixengine_proto::{PackageVersion, ServiceId, UpgradeEntry};

    use super::*;

    fn a_plan(old: OldVersion, outcome: UpgradeOutcome) -> UpgradePlan {
        UpgradePlan {
            subject: "php".to_owned(),
            from: PackageVersion::parse("8.4.24").expect("a fixture"),
            to: PackageVersion::parse("8.4.25").expect("a fixture"),
            to_installed: false,
            bytes: 31_457_280,
            stale: false,
            needs: Vec::new(),
            entries: vec![
                UpgradeEntry {
                    item: UpgradeItem::Default {},
                    outcome: outcome.clone(),
                },
                UpgradeEntry {
                    item: UpgradeItem::Site {
                        site: "blog.test".to_owned(),
                        from: ServiceId::parse("php-fpm@8.4.24").expect("a fixture"),
                        to: ServiceId::parse("php-fpm@8.4.25").expect("a fixture"),
                    },
                    outcome,
                },
            ],
            old,
        }
    }

    #[test]
    fn a_plan_says_what_moves_and_what_becomes_of_the_old_version() {
        let rendered = upgrade_plan(&a_plan(
            OldVersion::WillBeRemoved {},
            UpgradeOutcome::Planned {},
        ));

        assert!(rendered.starts_with("php 8.4.24 → 8.4.25"), "{rendered}");
        assert!(rendered.contains("to download"), "{rendered}");
        assert!(
            rendered.contains("blog.test moves from php-fpm@8.4.24 to php-fpm@8.4.25"),
            "{rendered}"
        );
        assert!(
            rendered.contains("8.4.25 becomes the default php"),
            "{rendered}"
        );
        assert!(
            rendered.contains("php 8.4.24 will be removed"),
            "{rendered}"
        );
        assert!(rendered.contains("--keep"), "{rendered}");
    }

    #[test]
    fn a_plan_kept_for_a_reason_names_the_reason() {
        let rendered = upgrade_plan(&a_plan(
            OldVersion::WillBeKept {
                because: vec!["blog pins php 8.4.24 in /work/blog/mixengine.toml".to_owned()],
            },
            UpgradeOutcome::Planned {},
        ));
        assert!(rendered.contains("will be kept"), "{rendered}");
        assert!(rendered.contains("/work/blog/mixengine.toml"), "{rendered}");
    }

    #[test]
    fn a_finished_update_marks_each_line() {
        let rendered = upgrade_plan(&a_plan(
            OldVersion::Kept {
                because: vec!["the front end refused the sites".to_owned()],
            },
            UpgradeOutcome::Skipped {},
        ));
        assert!(rendered.contains("(not done)"), "{rendered}");
        assert!(
            rendered.contains("php 8.4.24 was kept: the front end refused the sites"),
            "{rendered}"
        );
    }
}
