//! Uninstalls that remove, and nothing of ours left on this machine.

use super::*;

/// `--keep-home` undoes what is outside the home, leaves the home, and ends the daemon.
///
/// **Nothing outside the home is asserted to have gone.** What is proved is the half this flag is
/// for — the home survives — and the half T182 changed: a finished uninstall ends the daemon
/// whatever was kept, because a kept home is no reason to go on serving a machine that has just been
/// told to forget it (the T182 design, D1).
#[tokio::test(flavor = "multi_thread")]
async fn keeping_the_home_leaves_the_home_and_ends_the_daemon() {
    let home = Home::new();
    let mut daemon = home.start_daemon();

    if needs_an_administrator(&home) {
        return;
    }

    let printed = home.mix(&["uninstall", "--keep-home", "--yes", "--json"]);
    let report = json(&printed);

    let kept = report["items"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["id"] == "home")
        .expect("the home is always a row");

    assert_eq!(kept["outcome"]["removal"], "kept", "{report}");
    assert!(
        daemon.wait_until_gone(),
        "a finished uninstall that kept the home left its daemon running: {report}"
    );
    assert!(home.path().exists());
}

/// A complete uninstall takes the home with it, and the daemon goes so that it can.
///
/// **The one test here that removes something**, and what it removes is a temporary home this test
/// made. Nothing outside it is touched, which [`needs_an_administrator`] is what establishes rather
/// than assumes.
#[tokio::test(flavor = "multi_thread")]
async fn a_complete_uninstall_takes_the_home_and_the_daemon_with_it() {
    let home = Home::new();
    let mut daemon = home.start_daemon();

    if needs_an_administrator(&home) {
        return;
    }

    let pid = json(&home.mix(&["status", "--json"]))["daemon"]["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .expect("the daemon's pid");

    let printed = home.mix(&["uninstall", "--yes"]);

    // T182b, D8: `mix` returns only once the daemon's process has ended — not merely its endpoint,
    // which goes quiet while the daemon is still removing the home and its image still mapped.
    assert_eq!(
        mixengine_platform::process::started_at(pid).ok().flatten(),
        None,
        "mix returned while the daemon (pid {pid}) was still running:\n{}",
        stderr(&printed)
    );

    assert!(
        daemon.wait_until_gone(),
        "the daemon outlived the home it was serving:\n{}\n{}",
        stdout(&printed),
        stderr(&printed)
    );
    assert!(
        !home.path().exists(),
        "{}\n{}",
        stdout(&printed),
        stderr(&printed)
    );
    assert_eq!(
        printed.status.code(),
        Some(0),
        "{}\n{}",
        stdout(&printed),
        stderr(&printed)
    );

    // And it said so on the home's row, rather than leaving a person to infer it from the exit code.
    assert!(stdout(&printed).contains("going"), "{}", stdout(&printed));
}

/// The same, for a daemon `mix` autostarted — which is the daemon every real machine has.
///
/// **T182b, found on the first real Windows uninstall.** An autostarted daemon is started with its
/// home as its working directory, and Windows will not rename a directory a process is standing in,
/// the daemon's own included: every such uninstall kept the home and exited non-zero. The test
/// above never saw it, because a daemon this suite starts itself stands somewhere else.
#[tokio::test(flavor = "multi_thread")]
async fn an_autostarted_daemon_takes_its_home_with_it() {
    let home = Home::new();

    // Autostarted: nobody's child, standing in its home, exactly as on a person's machine.
    assert!(
        home.mix(&["status"]).status.success(),
        "mix could not autostart a daemon for this home"
    );

    if needs_an_administrator(&home) {
        return;
    }

    let printed = home.mix(&["uninstall", "--yes"]);

    assert!(
        !home.path().exists(),
        "an autostarted daemon kept its home:\n{}\n{}",
        stdout(&printed),
        stderr(&printed)
    );
    assert_eq!(
        printed.status.code(),
        Some(0),
        "{}\n{}",
        stdout(&printed),
        stderr(&printed)
    );
}

/// Typing anything but yes is a decline, and a decline removes nothing and fails nothing.
#[tokio::test(flavor = "multi_thread")]
async fn answering_no_removes_nothing_and_is_not_a_failure() {
    let home = Home::new();
    let _daemon = home.start_daemon();

    let before = home.contents();
    let answered = home.mix_answering("n\n", &["uninstall"]);

    assert_eq!(answered.status.code(), Some(0), "{}", stderr(&answered));
    assert!(home.path().exists());
    assert_eq!(home.contents(), before);
}

/// The clean-machine round trip — roadmap task **T87**, and the smoke test the task asks for.
///
/// **A fresh CI runner is the clean VM.** It has never had MixEngine on it, so everything
/// [`machine::reading`] finds after a grant is what MixEngine put there and nothing else.
///
/// **Every reading is taken with the operating system's own tools and its own paths**, spelled out
/// again in `mod machine` rather than asked of `mixengine_platform` — the rule
/// `crates/mixengine-elevate/tests/system.rs` already keeps: a test that asked the code under test
/// where to look could not notice it looking in the wrong place.
///
/// **And nothing is asserted that this runner did not actually produce.** A machine with no resolver
/// mechanism writes no resolver wiring, so that assertion is skipped there — with what was found
/// printed either way, so a leg that has quietly stopped proving anything is visible in the log.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "changes this machine — set MIXENGINE_SYSTEM_TESTS=1"]
async fn nothing_of_ours_is_left_on_this_machine() {
    if std::env::var("MIXENGINE_SYSTEM_TESTS").as_deref() != Ok("1") {
        return;
    }

    let home = Home::new();
    let mut daemon = home.start_daemon();

    // Everything a first run asks for: the helper, the certificate authority, the resolver wiring
    // and the port grant, in one batch behind one prompt.
    let granted = home.mix(&["elevation", "grant", "--yes"]);
    println!(
        "--- the grant ---\n{}{}",
        stdout(&granted),
        stderr(&granted)
    );

    // **Did that grant actually apply, and it is asked rather than assumed.** An empty queue is the
    // one honest answer to *can this runner change its own machine*, and two of the three cannot:
    // a Linux runner has no polkit agent, so `probe()` answers `Unavailable` whoever is asking; and
    // CI's macOS runner was measured on 2026-09-04 ending the elevated helper **without a report**,
    // leaving every operation pending. Both are ADR 0005's worst branch rather than gaps in this
    // job, and `tests/cert.rs` already records the first about the same runner.
    //
    // Asserting the machine there would be asserting something the runner cannot do; skipping
    // outright would leave those legs proving nothing. So the branch is chosen from what actually
    // happened, and each side asserts something of its own.
    //
    // **It is the grant's own record and not the length of the queue**, which was this predicate's
    // first draft and was wrong on the one runner where everything worked: a flush that succeeded
    // reconciles the hosts block immediately afterwards, so the queue is refilled a moment later by
    // design (`Elevation::flush`, D8) and a run that applied everything reads as one that applied
    // nothing. What settles it is what the grant reported about itself.
    let waiting = json(&home.mix(&["elevation", "status", "--json"]));
    let applied = waiting["last"]["outcome"] == "completed"
        && waiting["last"]["still_pending"].as_u64() == Some(0);
    println!("--- did the grant apply? ---\n{applied}: {waiting}");

    // And a name of this home's own, so the hosts block has something in it on a machine with no
    // scoped resolver — which is what a Linux runner without a systemd user manager is. Neither call
    // is asserted on: what this is arranging is the *machine*, and `held` below is what says whether
    // the arrangement worked.
    let root = home.path().join("smoke-project");
    std::fs::create_dir_all(&root).expect("a directory inside this home");

    let project = home.mix(&[
        "project",
        "create",
        &root.display().to_string(),
        "--name",
        "smoke",
    ]);
    println!(
        "--- the project ---\n{}{}",
        stdout(&project),
        stderr(&project)
    );

    // `--kind static`, because the default is php and a runner has no PHP installed: measured on
    // 2026-09-04, where the site refused with "no php version is installed as the default". Nothing
    // here needs a program to run; what it needs is a *name*.
    let site = home.mix(&[
        "site",
        "create",
        "--project",
        "smoke",
        "--kind",
        "static",
        "--domain",
        "uninstall-smoke.test",
    ]);
    println!("--- the site ---\n{}{}", stdout(&site), stderr(&site));

    // **This home's authority by key-id, and not "any MixEngine".** The suites that ran before this
    // one in the same job put authorities of their own homes into the same store, and an uninstall
    // removes exactly one — so a reading that matched the product name would find somebody else's
    // afterwards and report it as ours left behind.
    let authority = json(&home.mix(&["cert", "ca-status", "--json"]))["ca"]["key_id"]
        .as_str()
        .expect("a started daemon has an authority")
        .to_owned();
    println!("--- this home's authority ---\n{authority}");

    let held = machine::reading(&authority);
    println!("--- what this runner now holds ---\n{held}");

    let printed = home.mix(&["uninstall", "--yes"]);
    let said = stdout(&printed);
    println!("--- the uninstall ---\n{said}{}", stderr(&printed));

    // **Two legs, two answers, and neither is a weaker version of the other.** A machine that can
    // raise a prompt is asked the question this task exists to answer; a machine that cannot is
    // asked the one the risk list names, and both are assertions rather than skips.
    match applied {
        true => {
            assert!(
                held.anything(),
                "this machine can raise a prompt and the grant still wrote nothing to it, so \
                 removing it would prove nothing"
            );

            assert!(
                daemon.wait_until_gone(),
                "the daemon outlived the home it was serving"
            );

            let after = machine::reading(&authority);
            println!("--- what is left ---\n{after}");
            held.gone(&after);

            assert!(
                !home.path().exists(),
                "{} is still there",
                home.path().display()
            );
            assert_eq!(printed.status.code(), Some(0), "{}", stderr(&printed));
        }

        // **The Linux runner, and ADR 0005's worst branch.** No polkit agent means nothing of the
        // machine's can be removed — so what is asserted here is the behaviour the design's risk
        // list promises for exactly that: the machine rows say they are still waiting, and the home
        // is **kept**, because a home removed while this machine is still wired for it is one
        // nothing could repair. A leg that removed the home anyway would pass every other assertion
        // in this file and be badly wrong.
        false => {
            println!(
                "this machine cannot raise an elevation prompt, so what is proved here is that \
                 nothing was removed and the home was kept"
            );

            assert!(
                home.path().exists(),
                "the home went while this machine was still wired for it"
            );
            assert!(said.contains("kept"), "{said}");
            assert_ne!(
                printed.status.code(),
                Some(0),
                "an uninstall that left things behind reported success: {said}"
            );

            let after = machine::reading(&authority);
            println!("--- what is left ---\n{after}");
        }
    }
}
