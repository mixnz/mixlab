//! `mix service` against a real daemon supervising a real process.
//!
//! Task **T19b**, and the reason it is an end-to-end test rather than a unit one: everything below
//! the client has been proved in its own crate — the registry walks a plan, the handlers answer
//! `service.*` — and what has never been proved is that a person typing four words gets a process
//! started and an exit code that means it. That claim spans two operating-system processes and a
//! socket, so nothing here is mocked.
//!
//! **The services are `fakeservice` rows**, written by `mixengine_testkit::declare` — which is
//! Phase 3's `service.create` in the same sense — and rendered by the daemon's own generator (T30)
//! through the fixture recipe in `crates/mixengine-daemon/src/services/fakeservice.rs`. So what a
//! test below writes is a *declaration*, exactly as a user's would be, and the whole path from a row
//! to a running process is the one under test.
//!
//! **The five that declare a service are ignored in a release build**, because that recipe is
//! compiled into debug builds only: a release `mixengined` has nothing that can run a `fakeservice`,
//! so the home declares something it cannot start and those tests would fail for a reason that has
//! nothing to do with `mix service`. `ignore` rather than `#[cfg]`, so `cargo test --release` says
//! why they did not run.

mod harness;

use harness::{Home, json, stdout};
use mixengine_testkit::Service;
use serde_json::Value;

/// A home with a daemon in it, declaring `services`.
///
/// The order is forced: the migrations that create the schema run when the daemon opens the home, so
/// there is nothing to insert a row into until it is up.
fn running(services: &[Service]) -> (Home, harness::Daemon) {
    let home = Home::new();
    let daemon = home.start_daemon();

    home.declare(services);

    (home, daemon)
}

/// The number `fakeservice`'s recipe wishes for, and how far above it the daemon will look.
///
/// `preferred_port` and `SEARCH`, from `crates/mixengine-daemon/src/services/fakeservice.rs` and
/// `mixengine_core::services::ports`. Restated rather than imported for the reason that recipe's own
/// `READY_LINE` is restated: a client's suite may not reach into the daemon's internals. The test
/// below is what pins them together — a recipe that moved its wish leaves every instance outside
/// this band and fails here.
const PREFERRED: u64 = 41_000;
const SEARCH: u64 = 64;

/// **T34c from the end a person is at**: two services of one recipe cannot both have its port.
///
/// The recipe names one number and every instance of it wants that number, so each is given the
/// lowest free one at or above it — and whoever did not get the number is told so at the moment of
/// creation, because a developer whose `.env` says 41000 will otherwise find out from a connection
/// that is refused.
///
/// **What this deliberately does not assert is that the three are consecutive.** Free means free on
/// the machine, which the allocator finds out by binding: anything else on the runner holding 41000
/// for an instant moves the first instance up and leaves the number sitting there for the second —
/// the rule obeyed to the letter, and a test expecting `first + 1` red. That is not a hypothesis;
/// it is what turned `test (windows-latest)` red on master at 8b9a394, and nothing in this workspace
/// binds 41000, so what held it was the machine. The exact-successor claim is a claim about a band
/// of ports somebody controls, and it lives where one exists:
/// `a_moved_service_is_given_the_very_next_port` in `mixengine_core::services::ports`.
#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
fn every_instance_of_a_recipe_gets_a_port_of_its_own_and_the_moved_one_is_told() {
    let (home, _daemon) = running(&[
        Service::new("fakeservice@main"),
        Service::new("fakeservice@second"),
    ]);

    let first = json(&home.mix(&["service", "status", "fakeservice@main", "--json"]));
    let second = json(&home.mix(&["service", "status", "fakeservice@second", "--json"]));
    let created = json(&home.mix(&[
        "service",
        "create",
        "fakeservice@third",
        mixengine_testkit::VERSION,
        "--json",
    ]));

    let port = |summary: &Value| {
        summary["port"]
            .as_u64()
            .unwrap_or_else(|| panic!("a port in {summary}"))
    };
    let given = [port(&first), port(&second), port(&created["service"])];

    for port in given {
        assert!(
            (PREFERRED..=PREFERRED + SEARCH).contains(&port),
            "an instance was given {port}, outside the band the recipe's {PREFERRED} names: \
             {first} {second} {created}"
        );
    }

    let mut ascending = given;
    ascending.sort_unstable();
    assert!(
        ascending[0] < ascending[1] && ascending[1] < ascending[2],
        "two instances of one recipe were given one port: {given:?}"
    );

    // The half a status cannot show. The third cannot have the recipe's number whatever the machine
    // is doing — either an instance above holds it, or something else does and moved that instance
    // too — so it is always the one that has to be told, and told the number it wanted rather than
    // the number it got.
    assert_eq!(
        created["moved_from"]["preferred"].as_u64(),
        Some(PREFERRED),
        "the third was not told what it wanted and did not get: {created}"
    );
}

/// **What T30 added, from the end a person is at**: the configuration a service runs on is
/// generated from its row, and changing the row changes what the process does.
///
/// Every layer of that is proved where it lives — the merge, the template, the diff, the atomic
/// install, the spec — and none of those says that the file which reaches the disk is the file the
/// program is actually started with. That claim spans a database write, a daemon walk, a rendering
/// and a process, so it is here.
#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
fn changing_an_override_regenerates_the_config_and_the_service_runs_on_it() {
    let (home, _daemon) = running(&[Service::new("fakeservice@main")]);

    let started = json(&home.mix(&["service", "start", "fakeservice@main", "--json"]));
    assert_eq!(started["complete"], true, "{started}");

    // Rendered by the walk that started it, into the directory the service id names.
    let arguments = home
        .path()
        .join("etc")
        .join("fakeservice@main")
        .join("fakeservice.args");
    let rendered = std::fs::read_to_string(&arguments).expect("the generated arguments file");
    assert!(
        !rendered.contains("--exit-after"),
        "a service nobody configured to exit was told to: {rendered}"
    );

    home.mix(&["service", "stop", "fakeservice@main"]);

    // The one thing a user edits. Long enough that the start below is an ordinary one — a service
    // that died inside its own ready check would prove the opposite of what this is about.
    mixengine_testkit::declare::reconfigure_blocking(
        &home.database_file(),
        "fakeservice@main",
        r#"{"exit_after": 1500, "exit_code": 3}"#,
    );

    let restarted = json(&home.mix(&["service", "start", "fakeservice@main", "--json"]));
    assert_eq!(restarted["complete"], true, "{restarted}");

    let rendered = std::fs::read_to_string(&arguments).expect("the regenerated arguments file");
    assert!(rendered.contains("--exit-after"), "{rendered}");

    // And the process really is the one that file describes, which is the whole point: nothing else
    // in this home asked it to stop, so an exit is the generated configuration taking effect.
    let deadline = std::time::Instant::now() + mixengine_testkit::home::STARTUP;
    loop {
        let status = json(&home.mix(&["service", "status", "fakeservice@main", "--json"]));
        if status["state"] == "failed" {
            break;
        }

        assert!(
            std::time::Instant::now() < deadline,
            "the service ignored the configuration it was started with: {status}\n\
             --- {} ---\n{rendered}\n--- daemon.log ---\n{}",
            arguments.display(),
            home.daemon_log()
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// **What T31 added, from the same end**: a service that is *already running* when its
/// configuration changes is handed the new one, rather than being left on the old one until somebody
/// restarts it.
///
/// The test above is the other half of the pair and stops the service before changing anything,
/// which is the easy case — a start reads whatever is on disk. This one never stops it, and the two
/// assertions are one claim from two sides: the file the reload command creates is there, so the
/// command ran; and the service is still `running`, so nothing was restarted to make it happen.
///
/// **`service list` is what triggers it**, which looks incidental and is the design: the
/// configuration is regenerated at the top of every `service.*` call, so the walk that noticed the
/// change is the same one that reported it. Nothing here asks for a reload, because there is no such
/// command — a user edits their override and the next thing that looks finds it.
#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
fn changing_an_override_reaches_a_service_that_is_already_running() {
    let (home, _daemon) = running(&[Service::new("fakeservice@main")]);

    let started = json(&home.mix(&["service", "start", "fakeservice@main", "--json"]));
    assert_eq!(started["complete"], true, "{started}");

    let reloaded = home
        .path()
        .join("etc")
        .join("fakeservice@main")
        .join("reloaded");
    assert!(
        !reloaded.exists(),
        "starting a service is not the same as reloading one"
    );

    mixengine_testkit::declare::reconfigure_blocking(
        &home.database_file(),
        "fakeservice@main",
        r#"{"log_every": 250}"#,
    );

    // The walk that finds the rendering changed under a service that is up. What it does about it
    // reaches the runner as a request rather than as an answer, so the wait below is for the
    // runner's next turn and not for this command.
    let listed = json(&home.mix(&["service", "list", "--json"]));
    assert_eq!(listed["services"][0]["state"], "running", "{listed}");

    let deadline = std::time::Instant::now() + mixengine_testkit::home::STARTUP;
    while !reloaded.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the reload command was never run: {} is not there\n--- daemon.log ---\n{}",
            reloaded.display(),
            home.daemon_log()
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let status = json(&home.mix(&["service", "status", "fakeservice@main", "--json"]));
    assert_eq!(
        status["state"], "running",
        "a reload is not a restart: {status}"
    );
}

#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
fn a_service_starts_stops_and_says_so_in_both_renderings() {
    let (home, _daemon) = running(&[Service::new("fakeservice@main")]);

    // Nothing is running yet, and the listing says which service that is about.
    let listed = json(&home.mix(&["service", "list", "--json"]));
    assert_eq!(listed["services"][0]["id"], "fakeservice@main");
    assert_eq!(listed["services"][0]["state"], "stopped");
    assert_eq!(listed["services"][0]["supervised"], false);
    assert_eq!(
        listed["services"][0]["stopped_by"], "never",
        "a service that has not run was stopped by nobody (T167d)"
    );

    // The whole of T19b in one line: a person types four words and a process is running.
    let started = json(&home.mix(&["service", "start", "fakeservice@main", "--json"]));
    assert_eq!(started["complete"], true, "{started}");
    assert_eq!(started["reached"][0], "fakeservice@main", "{started}");
    assert!(started.get("failed").is_none(), "{started}");

    let status = json(&home.mix(&["service", "status", "fakeservice@main", "--json"]));
    assert_eq!(status["state"], "running", "{status}");
    assert_eq!(status["supervised"], true, "{status}");
    assert!(status["pid"].as_u64().is_some(), "{status}");
    assert!(
        status.get("stopped_by").is_none(),
        "a running service carries no stopped_by: {status}"
    );

    // The human rendering of the same answer, which is the half a person actually reads.
    let rendered = stdout(&home.mix(&["service", "status", "fakeservice@main"]));
    assert!(
        rendered.starts_with("fakeservice@main — running"),
        "{rendered}"
    );
    assert!(rendered.contains("supervised  yes"), "{rendered}");

    let stopped = home.mix(&["service", "stop", "fakeservice@main"]);
    assert!(stopped.status.success(), "{}", stdout(&stopped));
    assert_eq!(stdout(&stopped), "stopped fakeservice@main\n");

    let after = json(&home.mix(&["service", "status", "fakeservice@main", "--json"]));
    assert_eq!(after["state"], "stopped", "{after}");
    assert_eq!(after["supervised"], false, "{after}");
    assert_eq!(
        after["stopped_by"], "person",
        "a person's stop stays theirs: {after}"
    );
    assert!(after["last_started_at"].as_i64().is_some(), "{after}");

    // The field survives the stop, so the rendering has to be the part that stops calling it the
    // present: `stopped` with `started 4m ago` under it is a contradiction on one screen.
    let rendered = stdout(&home.mix(&["service", "status", "fakeservice@main"]));
    assert!(
        rendered.starts_with("fakeservice@main — stopped"),
        "{rendered}"
    );
    assert!(rendered.contains("last start"), "{rendered}");
}

#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
fn starting_one_service_starts_what_it_depends_on_and_says_which() {
    let (home, _daemon) = running(&[
        Service::new("fakeservice@main"),
        Service::new("fakeservice@php").depends_on("fakeservice@main"),
    ]);

    let walk = json(&home.mix(&["service", "start", "fakeservice@php", "--json"]));

    // The plan is the transitive set, and it is the only thing that tells a user that starting one
    // service is about to touch two — which is why it is rendered rather than summarised away.
    assert_eq!(
        walk["planned"],
        serde_json::json!(["fakeservice@main", "fakeservice@php"]),
        "{walk}"
    );
    assert_eq!(
        walk["reached"],
        serde_json::json!(["fakeservice@main", "fakeservice@php"]),
        "{walk}"
    );

    // And a stop of the dependency takes the dependent with it, in the opposite order.
    let stopped = json(&home.mix(&["service", "stop", "fakeservice@main", "--json"]));
    assert_eq!(
        stopped["planned"],
        serde_json::json!(["fakeservice@php", "fakeservice@main"]),
        "{stopped}"
    );

    let listed = json(&home.mix(&["service", "list", "--json"]));
    for service in listed["services"].as_array().expect("a list") {
        assert_eq!(service["state"], "stopped", "{listed}");
    }
}

#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
fn a_service_that_never_becomes_ready_fails_the_command_and_names_the_one_to_fix() {
    let (home, _daemon) = running(&[
        // Two seconds, because this is the one test that waits the timeout out on purpose.
        Service::new("fakeservice@main")
            .never_ready()
            .ready_timeout(2_000),
        Service::new("fakeservice@php").depends_on("fakeservice@main"),
    ]);

    let output = home.mix(&["service", "start"]);

    // **The exit code is the point of waiting.** `mix service start && …` has to stop here, and the
    // only way a client could know better without this is to re-derive the daemon's verdict from the
    // event stream.
    assert!(
        !output.status.success(),
        "a walk that failed exited zero: {}",
        stdout(&output)
    );

    // The answer is still on stdout: a walk that stopped at the first of two services is a described
    // failure, not a lost one.
    let rendered = stdout(&output);
    assert!(
        rendered.starts_with("fakeservice@main failed to start — not ready within 2s"),
        "{rendered}"
    );
    assert!(rendered.contains("blocked   fakeservice@php"), "{rendered}");

    // The dependent was never spawned, and its row says why the daemon did not try.
    let blocked: Value = json(&home.mix(&["service", "status", "fakeservice@php", "--json"]));
    assert_eq!(blocked["state"], "failed", "{blocked}");
    assert_eq!(blocked["pid"], Value::Null, "{blocked}");
}

/// **A timeout with nothing beside it says only that something was slow.**
///
/// The service's own lines are in `logs/services/<id>/current.log` whatever happens. What this is
/// about is `daemon.log`, which is the file a person reads when the failure reaches them somewhere
/// else — a CI log, a bug report, a screenshot. Measured on run 35470533602: a php-fpm pool on
/// Windows was reported as "not ready within 15s" and nothing the pool had printed travelled with
/// it, so there was nothing to diagnose and the only move left was to run it again.
#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
fn a_service_that_was_not_ready_in_time_has_its_last_lines_in_the_daemon_log() {
    let (home, _daemon) = running(&[Service::new("fakeservice@main")
        .never_ready()
        // Long enough that a few lines are printed before the check gives up, short enough that
        // this test is not two seconds of waiting for each of them.
        .log_every(100)
        .ready_timeout(2_000)]);

    let output = home.mix(&["service", "start"]);
    assert!(!output.status.success(), "{}", stdout(&output));

    let log = home.daemon_log();
    assert!(
        log.contains("these are its last lines"),
        "the daemon did not report what the service last said:\n{log}"
    );
    assert!(
        log.contains("fakeservice: line"),
        "the report holds no line the service printed:\n{log}"
    );
}

/// `mix service logs` — roadmap task **T16b**, from the side a person types it.
///
/// **The human rendering is the service's own output and nothing else**, which is what makes
/// `mix service logs caddy | grep …` work the way it does for every other program's log. The `--json`
/// rendering is the frame, because a script filtering on `stream` or ordering by `at` needs what the
/// human one deliberately drops.
#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
fn logs_print_what_a_service_printed_and_nothing_of_mixengines() {
    let (home, _daemon) = running(&[Service::new("fakeservice@main").log_every(50)]);

    home.mix(&["service", "start", "fakeservice@main"]);

    let printed = stdout(&home.mix(&["service", "logs", "fakeservice@main", "-n", "200"]));

    assert!(
        printed.contains(mixengine_testkit::service::READY_LINE),
        "the service's own line is there verbatim: {printed:?}"
    );
    assert!(
        !printed.contains("stdout") && !printed.contains('['),
        "nothing of ours is in front of it: {printed:?}"
    );

    // The same lines as frames, one JSON object per line, with the two things the text does not
    // carry: which stream it came from and when it was read.
    let framed = stdout(&home.mix(&["service", "logs", "fakeservice@main", "--json"]));
    let first: Value = serde_json::from_str(framed.lines().next().expect("at least one frame"))
        .expect("mix --json prints one frame per line");

    assert_eq!(first["type"], "line", "{first}");
    assert!(first["stream"].is_string(), "{first}");
    assert!(first["at"].as_i64().is_some(), "{first}");

    home.mix(&["service", "stop", "fakeservice@main"]);
}

/// A service id nothing declares is the daemon's `not_found`, in the shape every other command
/// fails in — the endpoint's status is the envelope and `mix` never invents a sentence of its own.
#[test]
fn logs_for_a_service_nothing_declares_fail_the_way_every_other_command_does() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let missing = home.mix(&["service", "logs", "fakeservice@main", "--json"]);

    assert!(!missing.status.success());

    let error: Value =
        serde_json::from_slice(&missing.stderr).expect("mix --json fails in JSON too");
    assert_eq!(error["code"], "not_found", "{error}");
}

#[test]
fn a_home_that_declares_nothing_says_so_rather_than_printing_nothing() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let listed = home.mix(&["service", "list"]);
    assert!(listed.status.success(), "{listed:?}");
    assert_eq!(stdout(&listed), "no services are declared in this home\n");

    // A `list` of nothing is a fact; a `status` of a service that is not there is a mistake, and the
    // two are answered differently on purpose.
    let missing = home.mix(&["service", "status", "fakeservice@main", "--json"]);
    assert!(!missing.status.success());
    let error: Value =
        serde_json::from_slice(&missing.stderr).expect("mix --json fails in JSON too");
    assert_eq!(error["code"], "not_found", "{error}");
}

/// **The project scope is a start's alone, and it is answered by the daemon** — roadmap task
/// **T125**.
///
/// What the set *is* is proved where it is derived (`core::sites::services_of`) and where an apply
/// uses it (`tests/blueprint.rs`). What this pins is the surface a person types at: a project this
/// home never heard of is refused by name rather than quietly starting every service — the answer
/// the empty target gives — and `stop` does not carry the flag at all, because that set includes the
/// front end every *other* site is reached through.
#[test]
fn a_project_scope_belongs_to_start_and_names_a_project_that_exists() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let missing = home.mix(&["service", "start", "--project", "nope", "--json"]);
    assert!(!missing.status.success(), "{missing:?}");
    let error: Value =
        serde_json::from_slice(&missing.stderr).expect("mix --json fails in JSON too");
    assert_eq!(error["code"], "not_found", "{error}");
    assert!(
        error["message"]
            .as_str()
            .unwrap_or_default()
            .contains("nope"),
        "{error}"
    );

    // clap's own usage exit code: `stop` has no such flag, so no call was made.
    let stopped = home.mix(&["service", "stop", "--project", "nope"]);
    assert_eq!(stopped.status.code(), Some(2), "{stopped:?}");
}

#[test]
fn a_service_id_that_cannot_exist_is_refused_before_a_daemon_is_started() {
    let home = Home::new();

    let output = home.mix(&["service", "status", "not a service id"]);

    // clap's own usage exit code, not ours: the value never became a `ServiceId`, so no call was
    // made — which is the point. Nothing was created in the home either.
    //
    // **"Nothing" means nothing beyond what the fixture seeded**, which since T44 is one
    // `config.toml` holding `[dns] port = 0` so that no suite binds the real DNS port.
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert_eq!(
        home.contents(),
        mixengine_testkit::Home::SEEDED,
        "a typo created something in {}",
        home.path().display()
    );
}

#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
fn a_walk_nobody_waits_for_is_reported_as_accepted_rather_than_as_finished() {
    let (home, _daemon) = running(&[Service::new("fakeservice@main").ready_after(1_000)]);

    let rendered = stdout(&home.mix(&["service", "start", "--no-wait"]));
    assert_eq!(
        rendered,
        "accepted — mixengined is starting fakeservice@main in the background\n"
    );

    // And the walk really is going on behind that answer, which is the difference between this and a
    // command that did nothing. Waiting for the service to reach `running` is the only proof of that
    // worth having: everything cheaper — a directory, a file, a row — is something the daemon had
    // already produced at startup, and would pass just as happily against an answer that was a lie.
    let deadline = std::time::Instant::now() + mixengine_testkit::home::STARTUP;
    loop {
        let status = json(&home.mix(&["service", "status", "fakeservice@main", "--json"]));
        if status["state"] == "running" {
            break;
        }

        assert!(
            std::time::Instant::now() < deadline,
            "the accepted walk never started the service: {status}\n--- daemon.log ---\n{}",
            home.daemon_log()
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// **`mix service autostart` reads it, writes it, and `mix service list` carries it** — T112.
///
/// The whole of what a client can do with the setting, from the end a person is at: nothing carries
/// it until somebody says so, `--on` writes it, the reading afterwards agrees, and the column is in
/// the table rather than only in `--json`.
#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the fakeservice recipe is compiled into debug builds only"
)]
fn a_service_is_told_to_start_with_mixengine_and_every_reading_agrees() {
    let (home, _daemon) = running(&[Service::new("fakeservice@flagged")]);

    let before = json(&home.mix(&["service", "autostart", "fakeservice@flagged", "--json"]));
    assert_eq!(
        before["autostart"],
        Value::Bool(false),
        "nothing carries the flag until somebody sets it: {before}"
    );

    let written = json(&home.mix(&[
        "service",
        "autostart",
        "fakeservice@flagged",
        "--on",
        "--json",
    ]));
    assert_eq!(
        written["autostart"],
        Value::Bool(true),
        "the answer is the service as it now is: {written}"
    );

    let read_back = stdout(&home.mix(&["service", "autostart", "fakeservice@flagged"]));
    assert!(
        read_back.contains("starts with MixEngine"),
        "a reading afterwards does not agree with the write: {read_back}"
    );

    let listed = stdout(&home.mix(&["service", "list"]));
    assert!(
        listed.contains("AUTOSTART"),
        "the column is in the table a person sees, not only in --json: {listed}"
    );

    let off = json(&home.mix(&[
        "service",
        "autostart",
        "fakeservice@flagged",
        "--off",
        "--json",
    ]));
    assert_eq!(
        off["autostart"],
        Value::Bool(false),
        "the setting goes back the other way too: {off}"
    );
}

/// **`--on` and `--off` are one group**, so asking for both is a refusal and not a coin toss — T112.
#[test]
fn asking_for_both_autostart_answers_at_once_is_refused() {
    let home = Home::new();

    let both = home.mix(&["service", "autostart", "fakeservice@any", "--on", "--off"]);

    assert!(
        !both.status.success(),
        "two contradictory flags were accepted"
    );
}

/// **T167b, ADR 0041: a home saves nothing until somebody asks it to**, and the switch goes both
/// ways and reads back — from the command line, which is the client-surface rule.
#[test]
fn save_resources_is_off_until_turned_on_and_reads_back() {
    let (home, _daemon) = running(&[]);

    let first = json(&home.mix(&["service", "save-resources", "--json"]));
    assert_eq!(
        first["on"],
        Value::Bool(false),
        "a new home saves nothing: {first}"
    );

    let on = json(&home.mix(&["service", "save-resources", "--on", "--json"]));
    assert_eq!(on["on"], Value::Bool(true), "{on}");

    let read_back = stdout(&home.mix(&["service", "save-resources"]));
    assert!(read_back.starts_with("on"), "{read_back}");

    let off = json(&home.mix(&["service", "save-resources", "--off", "--json"]));
    assert_eq!(off["on"], Value::Bool(false), "{off}");
}

/// **The repair T127 exists to provide is reachable from `mix`** — the client-surface rule.
///
/// Help only: the repair itself against a real server is `mariadb.rs`, `mysql.rs` and `postgres.rs`,
/// which is where there is a data directory to write into.
#[test]
fn a_credential_reset_is_a_service_subcommand() {
    let home = Home::new();

    let help = stdout(&home.mix(&["service", "--help"]));

    assert!(help.contains("reset-credential"), "{help}");
}

/// **A service that keeps no credential is refused, and the refusal names what does** — T127.
///
/// `--yes` so the prompt is not what stops it: the assertion is about the daemon's answer, and a
/// test that passed because nobody typed `y` would pass with the feature removed.
#[test]
fn a_service_with_no_credential_is_refused_a_reset() {
    let (home, _daemon) = running(&[Service::new("fakeservice@plain")]);

    let refused = home.mix(&["service", "reset-credential", "fakeservice@plain", "--yes"]);

    assert!(
        !refused.status.success(),
        "a service with no credential was given a reset"
    );

    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(
        said.contains("no superuser credential") || said.contains("database servers"),
        "the refusal does not say which services keep one: {said}"
    );
}

/// **T182g.** `mix service found` and `mix service adopt` reach the daemon: a fresh home has
/// nothing to adopt, and adopting something that is not there says where to look.
#[test]
fn found_and_adopt_reach_the_daemon_from_the_command_line() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let found = json(&home.mix(&["service", "found", "--json"]));
    assert_eq!(found["found"], serde_json::json!([]), "{found}");

    let table = stdout(&home.mix(&["service", "found"]));
    assert!(table.contains("nothing"), "{table}");

    let refused = home.mix(&["service", "adopt", "fakeservice@main"]);
    assert!(!refused.status.success(), "{refused:?}");
    let said = format!(
        "{}{}",
        stdout(&refused),
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(said.contains("mix service found"), "{said}");
}
