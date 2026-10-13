//! `mix home`: what a previous copy of the home left, and restoring it.

/// `mix home previous`, for a person.
pub(crate) fn home_previous(previous: &mixengine_proto::HomePrevious) -> String {
    let Some(copy) = &previous.copy else {
        return "no copy of an earlier install is in the folders this home keeps\n".to_owned();
    };

    let mut rendered = format!(
        "a copy of an earlier install: {} projects, {} sites, {} services, {} runtimes, {} packages\n  {}\n",
        copy.projects, copy.sites, copy.services, copy.runtimes, copy.packages, copy.path
    );
    rendered.push_str(match copy.newer {
        true => "  a newer MixEngine wrote it; install that version to restore it\n",
        false => "  `mix home restore` brings it back\n",
    });
    rendered
}

/// `mix home restore`, for a person.
pub(crate) fn home_restored(report: &mixengine_proto::HomeRestoreReport) -> String {
    let mut rendered = format!(
        "restored {} projects, {} sites, {} services, {} runtimes, {} packages\n",
        report.projects, report.sites, report.services, report.runtimes, report.packages
    );
    for line in &report.skipped {
        rendered.push_str(&format!("  skipped  {line}\n"));
    }
    for line in &report.problems {
        rendered.push_str(&format!("  problem  {line}\n"));
    }
    rendered.push_str("  restored databases are stopped, with a new admin password\n");
    rendered
}
