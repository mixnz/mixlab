//! `mix service`: the list, one service, a walk, and what a service may take.

use super::table::{MISSING, table};
use super::time::ago;
use mixengine_proto::{
    Enforcement, FrontEndOutcome, FrontEndReport, IdleExemption, IdleProbe, IdleReport, IdleSource,
    MemoryMeasure, MemoryWatchdog, Priority, ServiceCreation, ServiceId, ServiceLimitsReport,
    ServiceList, ServiceRemoval, ServiceState, ServiceSummary, ServiceWalk, StateReason,
    WhenExceeded,
};
use std::time::SystemTime;

/// What a walk was aiming for, in the one place the three commands differ.
///
/// `service.start`, `service.stop` and `service.restart` answer the same [`ServiceWalk`], so the
/// only thing a rendering needs from the command is the verb — and having it as a type rather than a
/// string is what stops "stopped" being printed by the one that starts things.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Walked {
    Start,
    Stop,
    Restart,

    /// A credential reset. The walk it reports is the *start* that puts back what the repair took
    /// down.
    ResetCredential,
}

impl Walked {
    /// What a service that got there did.
    const fn reached(self) -> &'static str {
        match self {
            Self::Start => "started",
            Self::Stop => "stopped",
            Self::Restart => "restarted",
            Self::ResetCredential => "started again",
        }
    }

    /// What the one that did not get there failed to do.
    const fn failed(self) -> &'static str {
        match self {
            Self::Start => "failed to start",
            Self::Stop => "failed to stop",
            Self::Restart => "failed to restart",
            Self::ResetCredential => "did not come back",
        }
    }

    /// The verb in the present, for a walk nobody is waiting for.
    const fn ongoing(self) -> &'static str {
        match self {
            Self::Start => "starting",
            Self::Stop => "stopping",
            Self::Restart => "restarting",
            Self::ResetCredential => "re-setting a credential",
        }
    }
}

/// `mix service list`, for a person.
///
/// A table because the question it answers is a comparison — which of these is up — and one block
/// per service would put the states four lines apart. `supervised` gets a column of its own rather
/// than being folded into the state: a row that says `running` with nothing supervising it is a
/// daemon that was killed, and merging the two would hide exactly the case worth seeing.
pub(crate) fn service_list(list: &ServiceList) -> String {
    if list.services.is_empty() {
        return "no services are declared in this home\n".to_owned();
    }

    let rows: Vec<[String; 7]> = list
        .services
        .iter()
        .map(|service| {
            [
                service.id.to_string(),
                service
                    .version
                    .as_ref()
                    .map_or_else(|| MISSING.to_owned(), |version| version.as_str().to_owned()),
                state(service),
                yes_no(service.autostart),
                yes_no(service.supervised),
                service
                    .pid
                    .map_or_else(|| MISSING.to_owned(), |pid| pid.to_string()),
                names(&service.depends_on),
            ]
        })
        .collect();

    // `AUTOSTART` beside `STATE` rather than out at the end. The two are the question somebody
    // scanning this table is actually asking: what is running, and what will be running after the
    // next login. `VERSION` beside the id, because the id is a name somebody chose and the version
    // is what it actually is.
    let mut out = table(
        [
            "SERVICE",
            "VERSION",
            "STATE",
            "AUTOSTART",
            "SUPERVISED",
            "PID",
            "DEPENDS ON",
        ],
        &rows,
    );

    // **Why each failed service failed.** A line each, after the table, because the sentence is too
    // long for a column and is the reason anybody reads this.
    for service in &list.services {
        if let Some(note) = &service.last_failure {
            out.push_str(&format!(
                "{} failed: {}
",
                service.id, note.detail
            ));
        }
    }

    out
}

/// A boolean as a table cell.
fn yes_no(value: bool) -> String {
    match value {
        true => "yes".to_owned(),
        false => "no".to_owned(),
    }
}

/// `mix service autostart <service>`, for a person.
///
/// **Two sentences and not one word.** The setting alone would leave somebody reading `no` beside a
/// service that does start at every login, because a start plan pulls in what the flagged services
/// depend on — so the second line says that out loud rather than leaving it to the help text of a
/// command they have already run.
pub(crate) fn service_autostart(service: &ServiceSummary) -> String {
    let answer = match service.autostart {
        true => "starts with MixEngine",
        false => "does not start with MixEngine",
    };

    format!(
        // aligned on purpose: `note` sits in the label column the lines above it use.
        "{}: {answer}\n  note        anything a service that does start depends on is started too, \
         whether or not it is set here\n",
        service.id
    )
}

/// `mix service save-resources`, for a person.
pub(crate) fn save_resources(answer: mixengine_proto::SaveResources) -> String {
    match answer.on {
        true => "on: a service nobody is using is stopped, and started again when it is needed\n"
            .to_owned(),
        false => "off: nothing is stopped for being idle unless you gave it a time with \
                  `mix service idle`\n"
            .to_owned(),
    }
}

/// `mix service status <service>`, for a person.
pub(crate) fn service_status(service: &ServiceSummary) -> String {
    let mut rendered = format!("{}: {}\n", service.id, state(service));

    let mut field = |label: &str, value: &str| {
        rendered.push_str(&format!("  {label:11} {value}\n"));
    };

    if let Some(version) = &service.version {
        field("version", version.as_str());
    }
    field("supervised", if service.supervised { "yes" } else { "no" });
    field("autostart", if service.autostart { "yes" } else { "no" });

    if let Some(pid) = service.pid {
        field("pid", &pid.to_string());
    }
    if let Some(port) = service.port {
        field("port", &port.to_string());
    }
    if let Some(started) = service.last_started_at {
        // The label, not the value: `last_started_at` outlives the run it names, so a service that
        // has been stopped still has one — and `stopped` with `started 4m ago` under it reads as a
        // contradiction rather than as the history it is.
        let label = match in_the_run_it_names(service.state) {
            true => "started",
            false => "last start",
        };
        field(label, &ago(started, SystemTime::now()));
    }
    if let Some(code) = service.last_exit_code {
        field("last exit", &code.to_string());
    }
    if !service.depends_on.is_empty() {
        field("depends on", &names(&service.depends_on));
    }

    // The two states that need a sentence rather than a word, for the same reason `mix status`
    // explains a daemon from another build: neither is wrong, and neither is what a user assumes.
    if service.state.is_none() {
        field(
            "note",
            "this service is declared and has never been created, so there is nothing to start yet",
        );
    } else if !service.supervised && service.pid.is_some() {
        field(
            "note",
            "the row names a process and nothing in this daemon is supervising it; that is what a \
             daemon which was killed leaves behind",
        );
    }

    rendered
}

/// `mix service front-end`, for a person.
///
/// **What the daemon said, not what this client worked out.** The row is picked by
/// [`ServiceSummary::role`], which is the whole point of that member: no client anywhere maps a
/// package name to a meaning.
pub(crate) fn front_end(found: Option<&ServiceSummary>) -> String {
    let Some(service) = found else {
        return "this home has no front end, so nothing is serving its sites\n  \
                `mix service set-front-end caddy` makes one out of an installed package\n"
            .to_owned();
    };

    format!(
        "every site in this home is reached through {}\n{}",
        service.id,
        service_status(service)
    )
}

/// `mix service set-front-end`, for a person.
///
/// **The outcome leads**, on [`service_walk`]'s rule: three of the five endings leave the home where
/// it started, and somebody reading this needs to know which one happened before anything else.
pub(crate) fn front_end_report(report: &FrontEndReport) -> String {
    let named =
        |id: Option<&ServiceId>| id.map_or_else(|| "nothing".to_owned(), ToString::to_string);

    let mut rendered = match &report.outcome {
        FrontEndOutcome::Unchanged {} => format!(
            "{} is already this home's front end; nothing was stopped, deleted or created\n",
            named(report.was.as_ref())
        ),

        FrontEndOutcome::Switched { started } => {
            let mut said = format!(
                "every site in this home is now reached through {}, and was reached through {}\n",
                named(report.now.as_ref()),
                named(report.was.as_ref())
            );

            match started {
                Some(walk) if walk.failed.is_some() => said.push_str(
                    "  it was started and did not come up; `mix service logs` has what it \
                     printed\n",
                ),
                Some(_) => said.push_str("  started, because the one it replaced was running\n"),
                None => said.push_str(
                    "  left stopped, because the one it replaced was; `mix service start` brings \
                     it up\n",
                ),
            }

            said
        }

        FrontEndOutcome::NotGranted { because } => format!(
            "this home is still reached through {}, and nothing was changed\n  {because}\n  \
             `mix elevation grant` allows it, and then this command works\n",
            named(report.now.as_ref())
        ),

        FrontEndOutcome::RolledBack { because } => format!(
            "the switch did not happen and {} was put back\n  {because}\n",
            named(report.now.as_ref())
        ),

        FrontEndOutcome::Failed { because } => format!(
            "the switch failed and this home is now reached through {}\n  {because}\n",
            named(report.now.as_ref())
        ),
    };

    if !report.answering {
        rendered.push_str(
            "  it has not been allowed to answer on 80 and 443 on this machine, so it will not \
             start; `mix doctor` says what to do\n",
        );
    }

    if let Some(data) = &report.kept_data {
        rendered.push_str(&format!("  the data left where it was: {data}\n"));
    }

    if !report.not_carried.is_empty() {
        rendered.push_str("  what did not travel with the switch:\n");

        for left in &report.not_carried {
            rendered.push_str(&format!("    - {left}\n"));
        }
    }

    rendered
}

/// `mix service start|stop|restart`, for a person.
///
/// **The failure leads**, where everything that went right is a list underneath it. A walk of six
/// services that stopped at the fourth is read by somebody who wants the name of the one to fix,
/// and putting five lines of `started` above it is five lines between them and the answer.
pub(crate) fn service_walk(walked: Walked, walk: &ServiceWalk) -> String {
    if walk.planned.is_empty() {
        return format!(
            "nothing to {}: this home declares no services\n",
            match walked {
                Walked::Start => "start",
                Walked::Stop => "stop",
                Walked::Restart => "restart",
                Walked::ResetCredential => "repair",
            }
        );
    }

    if !walk.complete {
        return format!(
            "accepted; mixengined is {} {} in the background\n",
            walked.ongoing(),
            names(&walk.planned)
        );
    }

    let Some(failure) = &walk.failed else {
        return format!("{} {}\n", walked.reached(), names(&walk.reached));
    };

    // The runner's sentence first, the state machine's reason when there is none, and the admission
    // when there is neither — a reason is `None` only when the failure was the daemon's own, and
    // inventing one would be worse than saying so.
    let mut rendered = match (&failure.detail, &failure.reason) {
        (Some(detail), _) => format!("{} {}: {detail}\n", failure.service, walked.failed()),
        (None, Some(reason)) => format!("{} {}: {reason}\n", failure.service, walked.failed()),
        (None, None) => format!(
            "{} {}: mixengined did not say why; logs/daemon.log has it\n",
            failure.service,
            walked.failed()
        ),
    };

    // The evidence, and the only part of a reason a client lays out itself: `StateReason`'s own
    // sentence says what happened, and these are the lines that show it. Two reasons carry any —
    // `CrashLoop`'s `tail`, and `SuperuserRefused`'s one line.
    match &failure.reason {
        Some(StateReason::CrashLoop { tail, .. }) => {
            for line in tail {
                rendered.push_str(&format!("    {line}\n"));
            }
        }

        // **And then the command that undoes it.** A walk carries no hint field and is not being
        // given one for this: the daemon said what happened, and which of `mix`'s own verbs repairs
        // it is what this binary knows and the daemon does not.
        Some(StateReason::SuperuserRefused { said }) => {
            rendered.push_str(&format!("    {said}\n"));
            rendered.push_str(&format!(
                "    `mix service reset-credential {}` writes this home's password into the data \
                 directory and keeps every database in it\n",
                failure.service
            ));
        }

        // `StateReason` is `#[non_exhaustive]`, and every other reason is its own sentence and
        // nothing more.
        _ => {}
    }

    if !walk.reached.is_empty() {
        rendered.push_str(&format!(
            "  {:9} {}\n",
            walked.reached(),
            names(&walk.reached)
        ));
    }

    if !walk.blocked.is_empty() {
        rendered.push_str(&format!("  {:9} {}\n", "blocked", names(&walk.blocked)));
    }

    rendered
}

/// What a summary says a service is doing, in one word.
fn state(service: &ServiceSummary) -> String {
    service
        .state
        .map_or_else(|| "not created".to_owned(), |state| state.to_string())
}

/// Whether the run `last_started_at` names is the one the service is still in.
///
/// Matched exhaustively on purpose, which is what [`ServiceState`] being a closed enum is for: a
/// state added later has to face this question rather than fall into a default. `Restarting` is on
/// the false side — a service waiting out a backoff has no process at all, so its last start is as
/// much history as a stopped one's.
const fn in_the_run_it_names(state: Option<ServiceState>) -> bool {
    match state {
        Some(
            ServiceState::Starting
            | ServiceState::Running
            | ServiceState::Degraded
            | ServiceState::Stopping,
        ) => true,
        Some(ServiceState::Stopped | ServiceState::Restarting | ServiceState::Failed) | None => {
            false
        }
    }
}

/// `mix service found`, for a person: one line per directory, what opens it or why nothing does.
pub(crate) fn service_found(found: &mixengine_proto::ServiceFoundList) -> String {
    if found.found.is_empty() {
        return "nothing an earlier install left is waiting to be adopted\n".to_owned();
    }

    let mut rendered = String::new();
    for row in &found.found {
        let state = match (&row.opens_with, &row.why_not) {
            (Some(version), _) => format!("opens with {version}"),
            (None, Some(why)) => why.clone(),
            (None, None) => "cannot be adopted".to_owned(),
        };
        rendered.push_str(&format!("{}  {state}\n  {}\n", row.service, row.path));
    }

    rendered
}

/// `mix service adopt`, for a person.
pub(crate) fn service_adopted(summary: &ServiceSummary) -> String {
    format!(
        "adopted {}, stopped\n  a new admin password was set where it keeps one; the databases and \
         accounts in it are as they were\n  `mix service start {}` starts it\n",
        summary.id, summary.id
    )
}

/// `mix service create`, for a person.
///
/// **The second paragraph is the whole reason the answer is not just the service.** A recipe's
/// preferred port is the number a person has in their `.env` and in their muscle memory, and a
/// service that was quietly given the next one along would be discovered as a connection that is
/// refused, hours later. So a move is stated at the moment it happens, with as much of the program
/// that took the port as this machine would give up.
#[must_use]
pub(crate) fn service_creation(creation: &ServiceCreation) -> String {
    let mut rendered = format!(
        "created {}
",
        creation.service.id
    );

    if let Some(port) = creation.service.port {
        rendered.push_str(&format!(
            "  it listens on port {port}
"
        ));
    }

    if let Some(moved) = &creation.moved_from {
        let holder = match (&moved.program, moved.pid) {
            (Some(program), _) => format!("{program} has it"),
            (None, Some(pid)) => format!("pid {pid} has it"),
            (None, None) => "another service or program on this machine has it".to_owned(),
        };

        rendered.push_str(&format!(
            "  it asked for {}; {holder}, so it was moved
",
            moved.preferred
        ));
    }

    rendered
}

/// `mix service delete`, for a person.
///
/// **The second line is the whole reason the answer is not just the service.** A delete keeps the
/// data directory, and a person who is not told which one it was has no way to find it later — or to
/// know that deleting the service did not delete their databases.
#[must_use]
pub(crate) fn service_removal(removal: &ServiceRemoval) -> String {
    let mut rendered = format!(
        "deleted {}
",
        removal.removed.id
    );

    match &removal.data_kept {
        Some(path) => rendered.push_str(&format!(
            "  its data is kept at {path}
"
        )),
        None => rendered.push_str(
            "  it had no data directory
",
        ),
    }

    rendered
}

/// A list of services, in the order the daemon gave them.
pub(super) fn names(services: &[ServiceId]) -> String {
    match services.is_empty() {
        true => MISSING.to_owned(),
        false => services
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// When a service is stopped for being unused, and what is holding it open right now.
///
/// **Four answers, and never fewer.** A service that stays running does so for one of four reasons
/// that look identical from outside — nothing idles it, somebody switched idling off for it,
/// something running depends on it, or a project is being kept warm. Two of those are settings and
/// two are not, so a rendering that showed only the policy would send half of the people who read
/// it to change something that was never the cause. This is `mix domain status`' rule, applied to a
/// smaller question.
pub(crate) fn service_idle(report: &IdleReport) -> String {
    let mut rendered = format!("{}\n", report.service);

    let policy = match &report.policy {
        Some(policy) => format!("after {}", policy.after),
        None => "never".to_owned(),
    };

    let source = match report.source {
        IdleSource::Row => "set for this service",
        IdleSource::Never => "switched off for this service",
        IdleSource::Recipe => "the default for this kind of service",
        // The state of every service in this build, and it is worth spelling out rather than
        // leaving as a blank: nothing is wrong, the feature simply has no default yet.
        IdleSource::Unset => "no default yet; nothing idles this",
        // Asked for, and nothing to measure it with. The line above still says "never", which is
        // what happens; this says why, which is what a person can act on.
        IdleSource::Unmeasurable => "asked for, but this service has nothing to measure",
        // A newer daemon distinguishing something this build does not. The policy line above is
        // still true, so what is lost is the provenance and not the answer.
        _ => "for a reason this version of `mix` does not know",
    };

    rendered.push_str(&idle_line("idle stop", &policy, source));

    if let Some(policy) = &report.policy {
        rendered.push_str(&idle_line("measured by", &probe(&policy.probe), ""));
    }

    for exemption in &report.exempt {
        let held = match exemption {
            IdleExemption::DependentRunning { service } => {
                format!("{service} is running and depends on it")
            }
            IdleExemption::ProjectKeptWarm { project } => {
                format!("the project {project} is being kept warm")
            }
            // A newer daemon knows a reason this build does not. Named as one rather than dropped:
            // what the reader needs is that *something* holds it open, and a blank line would say
            // the opposite.
            _ => "something this version of `mix` does not know about".to_owned(),
        };

        rendered.push_str(&idle_line("held open by", &held, ""));
    }

    rendered
}

/// How a probe is described to somebody who did not choose it.
///
/// A probe comes from the recipe rather than from the person reading this, so it is written as what
/// is being watched and not as the variant's name.
fn probe(probe: &IdleProbe) -> String {
    match probe {
        IdleProbe::Connections { port } => format!("connections to port {port}"),
        IdleProbe::HttpCounter { url, field } => format!("`{field}` at {url}"),
        IdleProbe::FastCgiStatus { socket, path } => {
            format!("`{path}` at {}", socket.display())
        }
        _ => "something this version of `mix` does not know about".to_owned(),
    }
}

/// One line of [`service_idle`], laid out as [`limit_line`] lays its own out.
fn idle_line(field: &str, value: &str, note: &str) -> String {
    format!("  {field:<13} {value:<24} {note}\n")
        .trim_end()
        .to_owned()
        + "\n"
}

/// What a service may take, and what this machine will actually do about each of it.
///
/// **The number and the verdict on one line, always.** A ceiling of 512 MB means one thing where it
/// is a commit charge enforced by a failed allocation and another where it is charged pages enforced
/// by the OOM killer — and a third where it is stored and enforced by nothing at all. Printing the
/// number alone would be telling a third of the truth.
///
/// **And every field, always, including the ones that are unset.** This is where `service.set_limits`
/// pays for taking the whole value rather than a patch: `mix service limits web set --cpu 50` clears
/// a memory ceiling that was there, and the only thing that keeps that from being a surprise is that
/// the cleared field is on the screen a line below the one that was set.
pub(crate) fn service_limits(report: &ServiceLimitsReport) -> String {
    let mut rendered = format!(
        "{}
",
        report.service
    );

    rendered.push_str(&limit_line(
        "cpu",
        &report.limits.cpu_percent.map_or_else(
            || "uncapped".to_owned(),
            |percent| format!("{percent}% of one core"),
        ),
        &enforcement(
            &report.support.cpu,
            report.support.memory_measure,
            false,
            report.limits.cpu_percent.is_some(),
        ),
    ));

    rendered.push_str(&limit_line(
        "memory",
        &report
            .limits
            .memory_mb
            .map_or_else(|| "uncapped".to_owned(), |mb| format!("{mb} MB")),
        &enforcement(
            &report.support.memory,
            report.support.memory_measure,
            true,
            report.limits.memory_mb.is_some(),
        ),
    ));

    rendered.push_str(&watchdog_line(report.watchdog));

    rendered.push_str(&limit_line(
        "priority",
        match report.limits.priority {
            Priority::Normal => "normal",
            Priority::Background => "background",
        },
        match report.support.priority {
            true => "enforced",
            false => "not enforced here",
        },
    ));

    rendered.push_str(&format!(
        "
cpu is a percentage of one core; this machine has {} of them
",
        report.support.cores
    ));

    rendered
}

/// The line about what is watching a ceiling this machine cannot hold.
///
/// **Both numbers and the ending, or nothing at all.** A client that printed only the restart would
/// say nothing about the services most worth saying something about: a database over its ceiling is
/// warned about and deliberately left alone, and a person who saw no line would think nothing was
/// watching. Empty for [`None`], which is a machine that enforces the ceiling itself or a service
/// that declared none — in both cases there is no loop to describe.
fn watchdog_line(watchdog: Option<MemoryWatchdog>) -> String {
    let Some(watchdog) = watchdog else {
        return String::new();
    };

    let minutes = watchdog.after_minutes;

    let ending = if watchdog.restarts {
        format!("restarted after {minutes} minutes over it")
    } else {
        "warned about; this service is not restarted automatically".to_owned()
    };

    format!("  {:<9} {:<18} {ending}\n", "watchdog", "checked a minute")
}

/// One field: what was asked for, and what happens to it here.
fn limit_line(field: &str, asked: &str, verdict: &str) -> String {
    format!("  {field:<9} {asked:<18} {verdict}\n")
}

/// What this machine does with one field, in words rather than in an enum's name.
///
/// **The tense depends on whether a ceiling is actually set**, and getting that wrong is a real way
/// to mislead: a field nobody has capped that reads *"enforced — at the ceiling, the service is
/// killed"* names a ceiling that does not exist. So an uncapped field is written conditionally, which
/// also makes this line useful *before* somebody sets one — it is where they find out what the
/// number would mean here.
///
/// `measured` is only true for the memory line: it is what the *number* counts, and a CPU percentage
/// counts the same thing everywhere.
fn enforcement(
    enforcement: &Enforcement,
    measure: MemoryMeasure,
    measured: bool,
    capped: bool,
) -> String {
    match enforcement {
        Enforcement::Hard { when } => {
            let ending = match when {
                WhenExceeded::AllocationFails => "the next allocation fails",
                WhenExceeded::Killed => "the service is killed",
            };

            let counts = counted(measure, measured);

            match capped {
                true => format!("enforced:{counts} at the ceiling, {ending}"),
                false => format!("would be enforced:{counts} at a ceiling, {ending}"),
            }
        }

        // The permanent fact: this operating system has no such mechanism, and none is coming.
        Enforcement::Unsupported => match capped {
            true => "stored, not enforced; this system has no such limit".to_owned(),
            false => "this system has no such limit".to_owned(),
        },

        // The fixable one, in the platform's own words, because they were written for this line.
        Enforcement::Unavailable { why } => match capped {
            true => format!("stored, not enforced: {why}"),
            false => format!("could not be enforced: {why}"),
        },

        // **Watched rather than capped.** Deliberately not the word "enforced": the service may go
        // over this number and keep running. What happens after it does is per service rather than
        // per machine, so it is the `watchdog` line below this one and not this sentence. The `why`
        // is carried where the platform gave one, which is a machine somebody could start
        // differently; macOS gives none and none is printed.
        Enforcement::Advisory { why } => {
            let counts = counted(measure, measured);

            let opening = match capped {
                true => format!("watched, not capped:{counts}"),
                false => format!("would be watched, not capped:{counts}"),
            };

            match why {
                Some(why) => format!("{opening} {why}"),
                None => format!("{opening} this system has no hard cap to give"),
            }
        }

        // A variant this build of the client has never heard of. The rest of the line is still true,
        // and saying so beats printing a word that was invented after this binary was compiled.
        _ => "this client does not know what this machine does with it".to_owned(),
    }
}

/// What the memory number counts here, as a clause to drop into a longer sentence.
///
/// Empty for every field but memory: a CPU percentage counts the same thing everywhere, and the
/// clause would be noise on the line that carries it.
fn counted(measure: MemoryMeasure, measured: bool) -> String {
    if !measured {
        return String::new();
    }

    match measure {
        MemoryMeasure::Commit => " counts committed memory;".to_owned(),
        MemoryMeasure::ChargedPages => " counts resident memory and page cache;".to_owned(),

        // Named as an overestimate on the line itself, because it is one: shared pages are counted
        // once per process, so a pool and its workers add up to more than they occupy.
        MemoryMeasure::Resident => {
            " counts resident memory, shared pages once per process;".to_owned()
        }

        _ => String::new(),
    }
}

#[cfg(test)]
pub(super) mod tests;
