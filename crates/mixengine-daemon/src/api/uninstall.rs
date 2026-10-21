//! `daemon.uninstall` — roadmap task **T87**: the job, and this daemon's own end when the home goes
//! with it. What an uninstall removes is [`crate::uninstall`]'s; `daemon.uninstall_plan` is an arm
//! in [`super::rpc`] that asks it directly.

use std::sync::Arc;

use mixengine_proto::rpc;
use mixengine_proto::{Error, ErrorCode, JobKind, JobSummary, UninstallQuery};

use super::Api;

impl Api {
    /// `daemon.uninstall` — one job, and this daemon's own end when the home goes with it.
    ///
    /// **The guard that ends the daemon is taken after the job, not around it.** A grant nobody
    /// answers keeps the job open for as long as the dialog is on the screen, and a `Going` held
    /// across that wait would end the daemon the moment the wait gave up — with the prompt still on
    /// the person's screen. So this waits for the job to finish, and only then asks whether the
    /// uninstall finished: a declined grant or a refused row finishes nothing, the daemon stays up,
    /// and the same command works when the person is ready (the T87 design, D9). **A finished one
    /// ends the daemon whatever was kept** — a kept home is no reason to go on serving a machine
    /// that has just been told to forget it (the T182 design, D1).
    pub(super) async fn uninstall_now(&self, query: UninstallQuery) -> Result<JobSummary, Error> {
        let uninstall = Arc::clone(&self.uninstall);
        let started = self
            .jobs
            .begin(
                &JobKind::parse(rpc::method::DAEMON_UNINSTALL).expect("a valid kind"),
                move |handle| async move {
                    let report = uninstall.run(&query, &handle).await?;

                    serde_json::to_value(report).map_err(|error| {
                        Error::new(
                            ErrorCode::Internal,
                            format!("an uninstall report could not be encoded: {error}"),
                        )
                    })
                },
            )
            .await?;

        let jobs = Arc::clone(&self.jobs);
        let armed = Arc::clone(&self.armed);
        let token = self.shutdown.token().clone();
        let id = started.id;

        tokio::spawn(async move {
            // In long steps rather than one deadline, because what this is waiting on has none: the
            // job ends when a person answers a prompt. Every other end of the wait — a job that was
            // cancelled, a row that has gone — leaves nothing armed and therefore ends nothing.
            const STEP: mixengine_proto::Millis = mixengine_proto::Millis(60_000);

            loop {
                match jobs.wait(id, STEP).await {
                    Ok(summary) if summary.state.is_finished() => break,
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(job = %id, %error, "an uninstall could not be followed");
                        return;
                    }
                }
            }

            if !armed.is_finished() {
                return;
            }

            tracing::info!(
                "this home has been removed from this machine; the daemon is stopping, and takes \
                 whatever of its own directories were not kept with it"
            );

            token.cancel();
        });

        Ok(started)
    }
}

#[cfg(test)]
mod tests {
    use mixengine_testkit::FakeService;
    use serde_json::Value;

    use super::*;
    use crate::api::test_daemon::{Daemon, daemon_on, undeclared};
    use crate::services::fixture;

    /// T87. The plan is a read: it answers on a home with nothing in it, it names every row, and it
    /// asks for nothing — a home whose queue is still empty afterwards is the assertion that it
    /// enqueued nothing.
    #[tokio::test]
    async fn the_uninstall_plan_answers_every_row_and_asks_for_nothing() {
        let daemon = undeclared().await;

        let answer = daemon
            .ask(rpc::method::DAEMON_UNINSTALL_PLAN, Value::Null)
            .await;

        let report: mixengine_proto::UninstallReport =
            serde_json::from_value(answer["result"].clone())
                .unwrap_or_else(|error| panic!("{error}: {answer}"));

        // Eleven rows before any relocation, and this fixture relocates nothing. On Windows a
        // twelfth: the fixture's copy is self-updatable, which there is what the installer placed,
        // so the plan keeps a `package` row naming the Windows uninstaller (T182a, D3).
        let expected = if cfg!(windows) { 12 } else { 11 };
        assert_eq!(report.items.len(), expected, "{report:?}");

        for item in &report.items {
            assert!(!item.what.is_empty(), "{item:?}");
            assert!(!item.location.is_empty(), "{item:?}");
            assert!(
                !matches!(item.outcome, mixengine_proto::Removal::Removed { .. }),
                "a plan removes nothing: {item:?}"
            );
        }

        // Every id exactly once, which is what a client renders against.
        let mut ids: Vec<mixengine_proto::ResidueId> =
            report.items.iter().map(|item| item.id).collect();
        let counted = ids.len();
        ids.sort_by_key(|id| format!("{id:?}"));
        ids.dedup();
        assert_eq!(ids.len(), counted, "{report:?}");

        let waiting = daemon
            .expect::<mixengine_proto::ElevationStatus>(rpc::method::ELEVATION_STATUS, Value::Null)
            .await;

        assert!(waiting.pending.is_empty(), "the plan enqueued something");
    }

    /// And the home is always a row, said the way the caller asked for it to be — the one
    /// irreversible thing on the list is the one a person must not have to infer.
    #[tokio::test]
    async fn the_plan_says_whether_the_home_is_being_kept() {
        let daemon = undeclared().await;

        for (keep, kept) in [(false, false), (true, true)] {
            let report: mixengine_proto::UninstallReport = daemon
                .expect(
                    rpc::method::DAEMON_UNINSTALL_PLAN,
                    serde_json::json!({ "keep_home": keep }),
                )
                .await;

            let home = report
                .items
                .iter()
                .find(|item| item.id == mixengine_proto::ResidueId::Home)
                .expect("the home is always a row");

            assert_eq!(
                matches!(home.outcome, mixengine_proto::Removal::Kept { .. }),
                kept,
                "{home:?}"
            );
        }
    }

    /// T87. Without `grant`, nothing outside this home is touched and every privileged row that had
    /// something to remove says what it is waiting for. That is the two-call path T64 exists for: a
    /// person reads the batch before they allow it.
    #[tokio::test]
    async fn an_uninstall_that_was_not_asked_to_grant_removes_nothing_privileged() {
        let daemon = undeclared().await;

        let started: JobSummary = daemon
            .expect(
                rpc::method::DAEMON_UNINSTALL,
                serde_json::json!({ "keep_home": true, "grant": false }),
            )
            .await;

        let report = uninstall_result(&daemon, started).await;

        for item in &report.items {
            // The three that need no token may report a removal; nothing else may, because nothing
            // else was applied.
            let unprivileged = matches!(
                item.id,
                mixengine_proto::ResidueId::PathEntry
                    | mixengine_proto::ResidueId::AutostartEntry
                    | mixengine_proto::ResidueId::BrowserTrust
            );

            assert!(
                unprivileged || !matches!(item.outcome, mixengine_proto::Removal::Removed { .. }),
                "nothing that needs the helper may be removed without a grant: {item:?}"
            );
        }
    }

    /// And `keep_home` leaves the home where it is, says so on its row, and arms no directory. The
    /// daemon ends when the run finished and not otherwise — whatever was kept (the T182 design,
    /// D1).
    #[tokio::test]
    async fn keeping_the_home_arms_no_directory_and_says_so() {
        let daemon = undeclared().await;

        let started: JobSummary = daemon
            .expect(
                rpc::method::DAEMON_UNINSTALL,
                serde_json::json!({ "keep_home": true, "grant": false }),
            )
            .await;

        let report = uninstall_result(&daemon, started).await;

        let home = report
            .items
            .iter()
            .find(|item| item.id == mixengine_proto::ResidueId::Home)
            .expect("the home is always a row");

        assert!(
            matches!(home.outcome, mixengine_proto::Removal::Kept { .. }),
            "{home:?}"
        );
        assert!(daemon.api.armed.is_empty(), "the home was armed anyway");

        let waiting = report
            .items
            .iter()
            .any(|item| matches!(item.outcome, mixengine_proto::Removal::Enqueued { .. }));
        assert_eq!(
            daemon.api.armed.is_finished(),
            !waiting,
            "a run with a privileged row still waiting is not finished, and one without is: \
             {report:?}"
        );
    }

    /// T182, D5. Without a grant, the three things that need no token are not touched either: an
    /// uninstall that cannot finish the privileged half changes nothing else.
    #[tokio::test]
    async fn without_a_grant_nothing_that_needs_no_token_is_touched_either() {
        let daemon = undeclared().await;

        let started: JobSummary = daemon
            .expect(
                rpc::method::DAEMON_UNINSTALL,
                serde_json::json!({ "keep_home": true, "grant": false }),
            )
            .await;

        let report = uninstall_result(&daemon, started).await;

        let waiting = report
            .items
            .iter()
            .any(|item| matches!(item.outcome, mixengine_proto::Removal::Enqueued { .. }));

        if waiting {
            for item in &report.items {
                assert!(
                    !matches!(item.outcome, mixengine_proto::Removal::Removed { .. }),
                    "a privileged row is waiting and this was removed anyway: {item:?}"
                );
            }
        }
    }

    /// The prompt an uninstall raises on a machine whose hosts file holds `own` in this home's
    /// block and `shared` in the block written before each home kept its own.
    async fn the_uninstall_prompt_with(
        own: &'static [&'static str],
        shared: &'static [&'static str],
    ) -> String {
        let daemon = daemon_on(Arc::new(fixture::Declared(Vec::new())), &[], move |home| {
            mixengine_platform::mock::Host::with_hosts(home, own.iter().copied())
                .with_legacy_hosts(shared.iter().copied())
        })
        .await;

        std::fs::write(
            daemon
                .api
                .paths()
                .root()
                .join("installed-beside")
                .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
            b"a helper the mock never runs",
        )
        .expect("a helper beside the program");

        let started: JobSummary = daemon
            .expect(
                rpc::method::DAEMON_UNINSTALL,
                serde_json::json!({ "keep_home": true, "grant": true }),
            )
            .await;
        let _ = finished_job(&daemon, started).await;

        daemon
            .host
            .prompts_raised()
            .last()
            .map(|prompt| prompt.body.clone())
            .unwrap_or_default()
    }

    /// T211g: this home's block and the shared one are two operations in one prompt.
    #[tokio::test]
    async fn an_uninstall_removes_this_homes_block_and_the_shared_one() {
        let body =
            the_uninstall_prompt_with(&["127.0.0.1 blog.test"], &["127.0.0.1 old.test"]).await;

        assert_eq!(body.matches("hosts-apply").count(), 2, "{body}");
        assert!(
            body.contains("\"home\":\""),
            "this home's block names its tag: {body}"
        );
    }

    /// T211g: a shared block alone is still something this home's uninstall removes.
    #[tokio::test]
    async fn the_hosts_row_counts_the_shared_block() {
        let body = the_uninstall_prompt_with(&[], &["127.0.0.1 old.test"]).await;

        assert!(body.contains("hosts-apply"), "{body}");
    }

    /// T182, D4. A process running from the home refuses the act before anything is enqueued.
    /// T182b, D6. The uninstall's prompt carries only what the uninstall asked for: a producer's
    /// operation waiting in the same queue is not in the batch, and is still waiting afterwards.
    #[tokio::test]
    async fn an_uninstall_grants_only_its_own_operations() {
        let daemon = daemon_on(Arc::new(fixture::Declared(Vec::new())), &[], |home| {
            mixengine_platform::mock::Host::with_hosts(home, ["127.0.0.1 blog.test"])
        })
        .await;

        let producers = mixengine_proto::privileged::PrivilegedOp::FirewallApply {
            plan: mixengine_proto::privileged::FirewallPlan {
                ports: vec![8080],
                label: "MixEngine — a producer".to_owned(),
            },
        };
        daemon
            .api
            .elevation
            .enqueue(&producers)
            .await
            .expect("a producer's want");

        // The fixture ships a shim beside its `mixengined` and no helper; a prompt needs one.
        std::fs::write(
            daemon
                .api
                .paths()
                .root()
                .join("installed-beside")
                .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
            b"a helper the mock never runs",
        )
        .expect("a helper beside the program");

        let started: JobSummary = daemon
            .expect(
                rpc::method::DAEMON_UNINSTALL,
                serde_json::json!({ "keep_home": true, "grant": true }),
            )
            .await;
        let _ = finished_job(&daemon, started).await;

        let raised = daemon.host.prompts_raised();
        let body = &raised.last().expect("the uninstall raised a prompt").body;
        assert!(body.contains("hosts-apply"), "{body}");
        assert!(
            !body.contains("firewall-apply"),
            "a producer's operation rode the uninstall's prompt: {body}"
        );
        // T182b. The batch removes the audit log it writes itself, even where the inventory found
        // none. The inventory reads this machine's real log directory, so on a machine that has a log
        // the row was planned anyway; CI's runners have none, which is where this is decided.
        assert!(
            body.contains("audit-log-remove"),
            "a batch that writes the audit log did not remove it: {body}"
        );

        let pending = daemon
            .api
            .elevation
            .status()
            .await
            .expect("a queue")
            .pending;
        assert!(
            pending.iter().any(|waiting| waiting.op == producers),
            "{pending:?}"
        );
    }

    /// T182b, D6. A declined uninstall leaves the queue as it found it — including a site's hosts
    /// block, which the uninstall's own empty block had displaced under the same key.
    #[tokio::test]
    async fn a_declined_uninstall_puts_back_what_it_displaced() {
        let daemon = daemon_on(Arc::new(fixture::Declared(Vec::new())), &[], |home| {
            mixengine_platform::mock::Host::with_hosts(home, ["127.0.0.1 blog.test"]).declining()
        })
        .await;

        // Before the site's want, as on a real machine: the helper is there from the install.
        std::fs::write(
            daemon
                .api
                .paths()
                .root()
                .join("installed-beside")
                .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
            b"a helper the mock never runs",
        )
        .expect("a helper beside the program");

        let sites = mixengine_proto::privileged::PrivilegedOp::hosts_apply(
            Some(mixengine_platform::hosts::tag_for(
                daemon.api.paths().root(),
            )),
            [
                mixengine_proto::privileged::HostEntry {
                    address: "127.0.0.1".parse().expect("an address"),
                    domain: "blog.test".to_owned(),
                },
                mixengine_proto::privileged::HostEntry {
                    address: "127.0.0.1".parse().expect("an address"),
                    domain: "shop.test".to_owned(),
                },
            ],
        );
        daemon
            .api
            .elevation
            .enqueue(&sites)
            .await
            .expect("a site's want");
        let before = daemon
            .api
            .elevation
            .status()
            .await
            .expect("a queue")
            .pending;

        let started: JobSummary = daemon
            .expect(
                rpc::method::DAEMON_UNINSTALL,
                serde_json::json!({ "keep_home": true, "grant": true }),
            )
            .await;
        let _ = finished_job(&daemon, started).await;

        assert_eq!(
            daemon.host.prompts_raised().len(),
            1,
            "the prompt was raised, and declined"
        );

        let after = daemon
            .api
            .elevation
            .status()
            .await
            .expect("a queue")
            .pending;
        let ops = |queue: &[mixengine_proto::PendingOp]| -> Vec<mixengine_proto::privileged::PrivilegedOp> {
            queue.iter().map(|waiting| waiting.op.clone()).collect()
        };
        assert_eq!(ops(&after), ops(&before), "{after:?}");
    }

    #[tokio::test]
    async fn a_process_in_the_home_refuses_the_uninstall() {
        let daemon = undeclared().await;
        let root = daemon._home.path().to_path_buf();

        // `fakeservice`, and not a copy of a system program: macOS refuses to launch a copy of one
        // of its own platform binaries from anywhere else (AMFI launch constraints), so a copied
        // `/bin/sleep` was killed on start and there was nothing in the home to refuse over — on
        // every Mac with SIP on. Its own clock ends it if the kill below is never reached.
        let source = FakeService::program();
        let fake = FakeService::new().exit_after(30_000);
        let args = fake.args();
        let occupant = root.join("t182-occupant");
        std::fs::create_dir_all(&occupant).expect("a directory in the home");
        let copy = occupant.join(source.file_name().expect("a file name"));
        std::fs::copy(&source, &copy).expect("copy the program");

        // Not a child of this test process: `processes_under` spares the daemon's descendants, and
        // in this harness the daemon *is* the test process. `cmd /c start` and a backgrounded `sh`
        // job both leave the program behind a launcher that has exited — `setsid` would too, but
        // macOS has none.
        let mut launcher = if cfg!(windows) {
            let mut command = std::process::Command::new("cmd");
            command
                .args(["/c", "start", "/b", ""])
                .arg(&copy)
                .args(args);
            command
        } else {
            let mut command = std::process::Command::new("sh");
            command
                .args(["-c", "\"$0\" \"$@\" >/dev/null 2>&1 &"])
                .arg(&copy)
                .args(args);
            command
        };
        launcher
            .stdout(std::process::Stdio::null())
            .status()
            .expect("start the occupant");

        let pending_before = daemon
            .api
            .elevation
            .status()
            .await
            .expect("a queue")
            .pending;

        let answer = daemon
            .ask(
                rpc::method::DAEMON_UNINSTALL,
                serde_json::json!({ "keep_home": false, "grant": false }),
            )
            .await;

        let refused = match answer.get("result") {
            Some(result) => {
                let started: JobSummary =
                    serde_json::from_value(result.clone()).expect("a job summary");
                matches!(
                    finished_job(&daemon, started).await.outcome,
                    Some(mixengine_proto::JobOutcome::Failed { .. })
                )
            }
            None => true,
        };

        let pending_after = daemon
            .api
            .elevation
            .status()
            .await
            .expect("a queue")
            .pending;

        for occupant in
            mixengine_platform::occupants::processes_under(std::slice::from_ref(&occupant), None)
        {
            let _ = kill(occupant.pid);
        }

        assert!(refused, "{answer}");
        assert_eq!(
            pending_before.len(),
            pending_after.len(),
            "something was enqueued"
        );
        assert!(daemon.api.armed.is_empty());
        assert!(!daemon.api.armed.is_finished());
    }

    /// End a process this test started by pid, on any system.
    fn kill(pid: u32) -> std::io::Result<std::process::ExitStatus> {
        if cfg!(windows) {
            std::process::Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .status()
        } else {
            std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .status()
        }
    }

    /// A complete uninstall keeps the home when something outside it is still there, and takes it
    /// when nothing is — and never any other combination.
    ///
    /// **Asserted as the relation and not as one outcome**, because which of the two arms this
    /// machine takes is a property of the machine: a workstation carries a privileged helper and an
    /// audit log of its own, and a fresh CI runner carries neither. Both arms are meaningful and
    /// neither is a skip.
    ///
    /// **The regression this pins was found by CI**: the fold that settles each row against the
    /// second reading was applied to *every* row, including the home — which is still `Planned` at
    /// that point, because `arm_the_home` rewrites it two statements later. Settled, the home read
    /// as an operation the helper had been asked about and had not managed, which made the run look
    /// unfinished, which kept the home. Every complete uninstall left the home behind and reported
    /// that it had meant to. Counting the machine's rows and not the home's is what makes this
    /// assertion able to say so.
    #[tokio::test]
    async fn a_complete_uninstall_keeps_the_home_exactly_when_the_machine_is_not_clear() {
        let daemon = undeclared().await;

        let started: JobSummary = daemon
            .expect(
                rpc::method::DAEMON_UNINSTALL,
                serde_json::json!({ "keep_home": false, "grant": false }),
            )
            .await;

        let report = uninstall_result(&daemon, started).await;

        let outstanding = report
            .items
            .iter()
            .filter(|item| {
                !matches!(
                    item.id,
                    mixengine_proto::ResidueId::Home
                        | mixengine_proto::ResidueId::RelocatedDirectory
                )
            })
            .any(|item| {
                matches!(
                    item.outcome,
                    mixengine_proto::Removal::Enqueued { .. }
                        | mixengine_proto::Removal::Failed { .. }
                )
            });

        let home = report
            .items
            .iter()
            .find(|item| item.id == mixengine_proto::ResidueId::Home)
            .expect("the home is always a row");

        match outstanding {
            true => {
                assert!(
                    matches!(home.outcome, mixengine_proto::Removal::Kept { .. }),
                    "something outside this home is still there and the home went anyway: \
                     {home:?}\n{report:?}"
                );
                assert!(daemon.api.armed.is_empty(), "{report:?}");
            }
            false => {
                assert!(
                    matches!(home.outcome, mixengine_proto::Removal::OnExit { .. }),
                    "nothing is outstanding and the home was kept anyway: {home:?}\n{report:?}"
                );
                assert!(
                    !daemon.api.armed.is_empty(),
                    "the home says it is going and nothing was armed to remove it"
                );
            }
        }
    }

    /// The report a `daemon.uninstall` job leaves behind, decoded.
    async fn uninstall_result(
        daemon: &Daemon,
        started: JobSummary,
    ) -> mixengine_proto::UninstallReport {
        match finished_job(daemon, started).await.outcome {
            Some(mixengine_proto::JobOutcome::Succeeded { result }) => {
                serde_json::from_value(result).expect("an uninstall report")
            }
            other => panic!("{other:?}"),
        }
    }

    /// A job, once it has finished.
    async fn finished_job(daemon: &Daemon, started: JobSummary) -> JobSummary {
        daemon
            .expect(
                rpc::method::JOB_WAIT,
                serde_json::json!({ "job": started.id, "timeout": 30_000 }),
            )
            .await
    }
}
