use super::{
    collect, downloadable, expand_dir, install, locate, sweep_staging_older_than, Source, Suite,
    Tool,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What the sweep takes and what it leaves.
///
/// The age is a parameter here and a constant in the app, because the alternative is a test
/// that has to move a directory's modification time backwards — which is a different thing on
/// each platform, and would be testing the clock rather than the rule.
#[test]
fn the_sweep_takes_abandoned_downloads_and_leaves_everything_else() {
    let root = std::env::temp_dir().join(format!("mixlab-test-{}", uuid::Uuid::new_v4()));
    let live = root.join("download-live");
    // An install that got as far as unpacking: what the sweep must be able to remove whole.
    std::fs::create_dir_all(root.join("download-dead/unpacked/bin")).unwrap();
    std::fs::write(root.join("download-dead/unpacked/bin/mysqldump"), b"x").unwrap();
    std::fs::create_dir_all(&live).unwrap();
    // The installed tools, which live in the same directory and must never be touched.
    std::fs::create_dir_all(root.join("mysql/bin")).unwrap();
    std::fs::write(root.join("mysql/bin/mysqldump"), b"x").unwrap();

    // Nothing is old enough yet — which is the case of a second copy of MixLab downloading.
    sweep_staging_older_than(&root, Duration::from_secs(6 * 60 * 60));
    assert!(root.join("download-dead").is_dir());
    assert!(live.is_dir());

    // Everything is, now.
    sweep_staging_older_than(&root, Duration::ZERO);
    assert!(!root.join("download-dead").exists());
    assert!(!live.exists());
    assert!(
        root.join("mysql/bin/mysqldump").is_file(),
        "the installed tools were swept"
    );

    // A tools directory that does not exist is not a failure: it is what a machine that has
    // never downloaded anything looks like.
    sweep_staging_older_than(&root.join("nowhere"), Duration::ZERO);

    std::fs::remove_dir_all(&root).unwrap();
}

/// Fetches a suite the way the app does and checks the result is usable: the pinned URL still
/// serves a file, that file still hashes to what {@link archive_source} pins, the archive still
/// holds the programs {@link collect} goes looking for, and — the part that cannot be answered
/// from any other machine — those programs start here, with the libraries copied out beside
/// them and nothing else.
///
/// A platform with no archive for this suite passes without downloading: that is the vendor's
/// answer, not a fault.
fn check_pinned_download(suite: Suite) {
    if !downloadable(suite) {
        eprintln!(
            "{}: no archive for this platform, nothing to check",
            suite.slug()
        );
        return;
    }
    let tools_dir = std::env::temp_dir().join(format!("mixlab-download-{}", uuid::Uuid::new_v4()));
    let installed = install(suite, &tools_dir, &|_| {});
    // Every check runs before the cleanup, so that a failure still takes the download with it
    // rather than leaving hundreds of megabytes behind on the runner.
    let ran: Vec<(&str, String)> = suite
        .tools()
        .into_iter()
        .map(|tool| {
            // `locate` would happily return a copy already on the machine, which would make
            // this pass without the download having worked at all.
            let Some((path, Source::Downloaded)) = locate(tool, &tools_dir) else {
                return (tool.stem(), "was not in the download".to_string());
            };
            match std::process::Command::new(&path).arg("--version").output() {
                Ok(out) if out.status.success() => (tool.stem(), String::new()),
                Ok(out) => (
                    tool.stem(),
                    format!(
                        "exited {}: {}",
                        out.status,
                        String::from_utf8_lossy(&out.stderr)
                    ),
                ),
                // A missing library reads as the program not starting, which is the whole
                // reason this runs on a real machine of each kind.
                Err(e) => (tool.stem(), format!("did not start: {e}")),
            }
        })
        .collect();
    let _ = std::fs::remove_dir_all(&tools_dir);

    if let Err(e) = installed {
        panic!("installing the {} tools failed: {e}", suite.slug());
    }
    for (tool, problem) in ran {
        assert!(problem.is_empty(), "{tool} {problem}");
    }
}

/// Ignored because each of these fetches tens to hundreds of megabytes from a vendor, which is
/// no part of an ordinary test run. `.github/workflows/tool-downloads.yml` runs them on a real
/// machine of each kind — see `docs/standards/desktop/bumping-tool-downloads.md`.
#[test]
#[ignore = "downloads from the vendor; run it through the tool-downloads workflow"]
fn the_pinned_mysql_download_still_installs_and_runs() {
    check_pinned_download(Suite::Mysql);
}

#[test]
#[ignore = "downloads from the vendor; run it through the tool-downloads workflow"]
fn the_pinned_postgres_download_still_installs_and_runs() {
    check_pinned_download(Suite::Postgres);
}

#[test]
#[ignore = "downloads from the vendor; run it through the tool-downloads workflow"]
fn the_pinned_mongo_download_still_installs_and_runs() {
    check_pinned_download(Suite::Mongo);
}

/// The empty file's SHA-256, which is a fine stand-in for a real archive: what is under test is
/// the comparison, not the hashing.
/// A `*` in a search directory stands for one level of subdirectory, whether it is a whole
/// path segment (`Tools\*\bin`) or only part of one (`MariaDB *`).
#[test]
fn a_star_expands_against_what_is_on_disk() {
    let root = std::env::temp_dir().join(format!("mixlab-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("MariaDB 11.4").join("bin")).unwrap();
    std::fs::create_dir_all(root.join("Postgres").join("bin")).unwrap();

    let whole_segment = expand_dir(&format!("{}/*/bin", root.display()));
    let partial_segment = expand_dir(&format!("{}/MariaDB */bin", root.display()));
    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(whole_segment.len(), 2);
    assert_eq!(partial_segment, vec![root.join("MariaDB 11.4").join("bin")]);
}

/// The programs go where the tools are looked for, and the library they are linked against
/// goes where they will look for it — which is a different place on each platform, and the
/// whole reason a download is unpacked rather than copied out flat.
#[test]
fn a_download_is_laid_out_the_way_the_tools_are_run_from() {
    let root = std::env::temp_dir().join(format!("mixlab-test-{}", uuid::Uuid::new_v4()));
    let unpacked = root.join("unpacked").join("mysql-8.0.40");
    // Where each platform's archive keeps its OpenSSL, and where the clients then look for it.
    let (from, into, library) = if cfg!(windows) {
        ("bin", "bin", "libssl-3-x64.dll")
    } else if cfg!(target_os = "macos") {
        ("lib", "lib", "libssl.3.dylib")
    } else {
        ("lib/private", "lib/private", "libssl.so.3")
    };
    std::fs::create_dir_all(unpacked.join("bin")).unwrap();
    std::fs::create_dir_all(unpacked.join(from)).unwrap();
    for tool in Suite::Mysql.tools() {
        std::fs::write(unpacked.join("bin").join(tool.file_name()), b"").unwrap();
    }
    std::fs::write(unpacked.join(from).join(library), b"").unwrap();
    // Something the tools have no use for, which should be left in the archive.
    std::fs::write(unpacked.join("bin").join("mysqladmin"), b"").unwrap();

    let tools_dir = root.join("tools");
    let found = collect(
        &root.join("unpacked"),
        Suite::Mysql,
        &tools_dir.join("mysql"),
    )
    .unwrap();
    let placed = tools_dir.join("mysql").join(into).join(library).is_file();
    let spare = tools_dir
        .join("mysql")
        .join("bin")
        .join("mysqladmin")
        .exists();
    // The layout is only worth anything if it is also the one the tools are then found in.
    let located = locate(Tool::MysqlDump, &tools_dir);
    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(found, 2);
    assert!(
        placed,
        "the library the clients load was not put where they look for it"
    );
    assert!(!spare, "a program the suite does not use was copied out");
    let (path, source) = located.expect("the downloaded copy was not found again");
    assert!(matches!(source, Source::Downloaded));
    assert_eq!(path.parent().and_then(Path::file_name).unwrap(), "bin");
}

/// A PostgreSQL archive carries the whole install's libraries, most of which belong to the
/// server, the ECPG precompiler or the Stack Builder rather than to the two programs taken out
/// of it. What the tools load has to come along; the collation data beside it must not.
///
/// Only where there is a download to shape: on Linux the tools come from the machine, and
/// nothing has had to decide which of their libraries to keep.
#[cfg(any(windows, target_os = "macos"))]
#[test]
fn a_postgres_download_leaves_the_libraries_its_tools_never_load() {
    // Where each platform's archive keeps its libraries, what they are called there, and where
    // the tools then look for them.
    let (from, into, kept, spare): (_, _, &[&str], &[&str]) = if cfg!(windows) {
        (
            "bin",
            "bin",
            &[
                "libpq.dll",
                "libssl-3-x64.dll",
                "libintl-9.dll",
                "libzstd.dll",
            ],
            &[
                "icudt77.dll",
                "wxbase3211u_vc_x64_custom.dll",
                "libxml2.dll",
            ],
        )
    } else {
        (
            "lib",
            "lib",
            &[
                "libpq.5.dylib",
                "libssl.3.dylib",
                "libintl.8.dylib",
                "libzstd.1.dylib",
            ],
            &[
                "libicudata.77.1.dylib",
                "libxml2.16.dylib",
                "libecpg.6.dylib",
            ],
        )
    };

    let root = std::env::temp_dir().join(format!("mixlab-test-{}", uuid::Uuid::new_v4()));
    let unpacked = root.join("unpacked").join("pgsql");
    std::fs::create_dir_all(unpacked.join("bin")).unwrap();
    std::fs::create_dir_all(unpacked.join(from)).unwrap();
    for tool in Suite::Postgres.tools() {
        std::fs::write(unpacked.join("bin").join(tool.file_name()), b"").unwrap();
    }
    for name in kept.iter().chain(spare.iter()) {
        std::fs::write(unpacked.join(from).join(name), b"").unwrap();
    }

    let target = root.join("tools").join("postgres");
    let found = collect(&root.join("unpacked"), Suite::Postgres, &target).unwrap();
    let landed = target.join(into);
    let all_kept = kept.iter().all(|name| landed.join(name).is_file());
    let none_spare = spare.iter().all(|name| !landed.join(name).exists());
    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(found, 2);
    assert!(all_kept, "a library the tools load was left behind");
    assert!(none_spare, "a library the tools never load was copied out");
}

#[test]
fn a_directory_without_a_star_stands_for_itself() {
    assert_eq!(
        expand_dir("/usr/local/bin"),
        vec![PathBuf::from("/usr/local/bin")]
    );
}
