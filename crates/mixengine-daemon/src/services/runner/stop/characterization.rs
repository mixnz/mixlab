//! What asking a service to stop does today, pinned from the outside before the walk is rewritten.
//!
//! Every behaviour a spec can name, against a real process where an [`Exit`] has to come back —
//! there is no other way to make one — and against [`Failing`] where the OS has to refuse.

use std::collections::BTreeMap;

use mixengine_platform::process::{self, Limits};
use mixengine_proto::{Millis, ServiceSpec};
use mixengine_testkit::FakeService;

use super::super::super::fixture::{arguments, home, spec};
use super::super::tests::runner;
use super::*;

/// The grace period every spec here asks for.
///
/// Long enough that a stop which waited it out and one which did not are seconds apart, whatever a
/// process spawn costs on a loaded Windows runner.
const GRACE: Duration = Duration::from_secs(5);

/// Anything quicker than this did not wait for the grace period.
const AT_ONCE: Duration = Duration::from_secs(3);

/// A process the OS will tell nothing about, and will not pass a request to.
struct Failing;

impl Stoppable for Failing {
    fn ask_to_stop(&self) -> mixengine_platform::Result<()> {
        Err(refused())
    }

    fn exited(&mut self) -> mixengine_platform::Result<Option<Exit>> {
        Err(refused())
    }
}

fn refused() -> mixengine_platform::Error {
    mixengine_platform::Error::UnsupportedPlatform {
        capability: "a test process",
        reason: "this test refuses every question".to_owned(),
    }
}

/// A `fakeservice` this test is the parent of.
fn spawn(fake: &FakeService) -> Supervised {
    process::spawn_supervised(
        &FakeService::program(),
        fake.args(),
        &std::env::temp_dir(),
        &BTreeMap::new(),
        &Limits::default(),
    )
    .expect("a fakeservice starts")
}

fn stopped_by(behaviour: StopBehaviour) -> ServiceSpec {
    spec("svc").stop(behaviour).build().expect("a usable spec")
}

fn by_signal() -> ServiceSpec {
    stopped_by(StopBehaviour::Signal {
        grace: Millis::from_secs(5),
    })
}

fn by_command(program: std::path::PathBuf, args: Vec<String>) -> ServiceSpec {
    stopped_by(StopBehaviour::Command {
        program,
        args,
        grace: Millis::from_secs(5),
    })
}

/// A command `fakeservice` that exits at once with `code`.
fn command_exiting_with(code: i32) -> ServiceSpec {
    by_command(
        FakeService::program(),
        arguments(&FakeService::new().exit_after(0).exit_code(code)),
    )
}

#[tokio::test]
async fn kill_returns_at_once_without_asking() {
    let (_home, paths, store) = home(&["svc"]).await;
    let runner = runner(stopped_by(StopBehaviour::Kill), &paths, &store);

    let began = Instant::now();
    let exit = runner.ask_to_stop(&mut Failing, FLUSH).await;

    assert!(exit.is_none(), "a kill asks nothing and reports nothing");
    assert!(began.elapsed() < AT_ONCE, "took {:?}", began.elapsed());
}

#[tokio::test]
async fn a_signal_the_process_obeys_reports_its_exit() {
    let (_home, paths, store) = home(&["svc"]).await;
    let runner = runner(by_signal(), &paths, &store);
    let mut service = spawn(&FakeService::new());

    let began = Instant::now();
    let exit = runner.ask_to_stop(&mut service, FLUSH).await;

    if CAN_ASK_TO_STOP {
        assert!(exit.is_some(), "the service went when it was asked");
    } else {
        assert!(
            exit.is_none(),
            "nothing can be asked here, so the caller kills"
        );
    }
    assert!(began.elapsed() < AT_ONCE, "took {:?}", began.elapsed());
}

#[tokio::test]
async fn a_signal_that_cannot_be_sent_kills_at_once() {
    let (_home, paths, store) = home(&["svc"]).await;
    let runner = runner(by_signal(), &paths, &store);

    let began = Instant::now();
    let exit = runner.ask_to_stop(&mut Failing, FLUSH).await;

    assert!(exit.is_none());
    assert!(began.elapsed() < AT_ONCE, "took {:?}", began.elapsed());
}

#[tokio::test]
async fn a_signal_the_process_ignores_waits_out_the_grace() {
    let (_home, paths, store) = home(&["svc"]).await;
    let runner = runner(by_signal(), &paths, &store);
    let mut service = spawn(&FakeService::new().ignoring_stop());

    let began = Instant::now();
    let exit = runner.ask_to_stop(&mut service, FLUSH).await;
    let took = began.elapsed();

    assert!(exit.is_none(), "it never went, so the caller kills");
    if CAN_ASK_TO_STOP {
        assert!(took >= GRACE - Duration::from_millis(500), "took {took:?}");
    } else {
        assert!(
            took < AT_ONCE,
            "nothing was asked, so nothing is waited for: {took:?}"
        );
    }
}

#[tokio::test]
async fn a_command_that_works_waits_for_the_exit() {
    let (home_dir, paths, store) = home(&["svc"]).await;
    let flag = home_dir.path().join("asked-to-stop");
    let runner = runner(
        by_command(
            FakeService::program(),
            arguments(&FakeService::new().touch(&flag)),
        ),
        &paths,
        &store,
    );
    let mut service = spawn(&FakeService::new().ignoring_stop().exit_when(&flag));

    let began = Instant::now();
    let exit = runner.ask_to_stop(&mut service, FLUSH).await;

    let exit = exit.expect("the command made the service go, and its exit is reported");
    assert_eq!(exit.code(), Some(0));
    assert!(began.elapsed() < AT_ONCE, "took {:?}", began.elapsed());
}

#[tokio::test]
async fn a_failed_command_on_a_service_that_went_reports_its_exit() {
    let (_home, paths, store) = home(&["svc"]).await;
    let runner = runner(command_exiting_with(1), &paths, &store);
    let mut service = spawn(&FakeService::new().exit_after(0));
    tokio::time::sleep(Duration::from_secs(1)).await;

    let exit = runner.ask_to_stop(&mut service, FLUSH).await;

    let exit = exit.expect("a service that went is never reported as killed");
    assert_eq!(exit.code(), Some(0));
}

#[tokio::test]
async fn a_failed_command_on_a_running_service_kills() {
    let (_home, paths, store) = home(&["svc"]).await;
    let runner = runner(command_exiting_with(1), &paths, &store);
    let mut service = spawn(&FakeService::new().ignoring_stop());

    let began = Instant::now();
    let exit = runner.ask_to_stop(&mut service, FLUSH).await;

    assert!(
        exit.is_none(),
        "the command failed and the service is still up"
    );
    assert!(began.elapsed() < AT_ONCE, "took {:?}", began.elapsed());
}

#[tokio::test]
async fn a_command_that_cannot_start_kills_or_reports_the_exit() {
    let (home_dir, paths, store) = home(&["svc"]).await;
    let missing = home_dir.path().join("no-such-program");

    let runner = runner(by_command(missing.clone(), Vec::new()), &paths, &store);
    let mut running = spawn(&FakeService::new().ignoring_stop());
    assert!(
        runner.ask_to_stop(&mut running, FLUSH).await.is_none(),
        "nothing asked the running service to go"
    );

    let mut gone = spawn(&FakeService::new().exit_after(0));
    tokio::time::sleep(Duration::from_secs(1)).await;
    let exit = runner.ask_to_stop(&mut gone, FLUSH).await;
    assert_eq!(
        exit.expect("a service that went is reported as having gone")
            .code(),
        Some(0)
    );
}

#[tokio::test]
async fn an_exit_that_cannot_be_read_while_waiting_kills() {
    let (_home, paths, store) = home(&["svc"]).await;
    let runner = runner(command_exiting_with(0), &paths, &store);

    let began = Instant::now();
    let exit = runner.ask_to_stop(&mut Failing, FLUSH).await;

    assert!(exit.is_none());
    assert!(began.elapsed() < AT_ONCE, "took {:?}", began.elapsed());
}

mod plan {
    use std::path::Path;

    use super::*;

    #[test]
    fn plan_stop_covers_every_behaviour() {
        let grace = Millis::from_secs(5);
        let signal = StopBehaviour::Signal { grace };
        let command = StopBehaviour::Command {
            program: "mariadb-admin".into(),
            args: vec!["shutdown".to_owned()],
            grace,
        };
        let asked = StopPlan::Command {
            program: Path::new("mariadb-admin"),
            args: &["shutdown".to_owned()],
            grace,
        };

        assert_eq!(plan_stop(&StopBehaviour::Kill, true), StopPlan::Kill);
        assert_eq!(plan_stop(&StopBehaviour::Kill, false), StopPlan::Kill);
        assert_eq!(plan_stop(&signal, true), StopPlan::Signal { grace });
        assert_eq!(
            plan_stop(&signal, false),
            StopPlan::Kill,
            "Windows has no request to send a process it gave no console to (ADR 0008)"
        );
        assert_eq!(plan_stop(&command, true), asked);
        assert_eq!(
            plan_stop(&command, false),
            asked,
            "a command is how a service is asked where a signal cannot be sent"
        );
    }
}
