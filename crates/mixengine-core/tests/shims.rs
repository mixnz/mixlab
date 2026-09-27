//! Filling `<root>/bin` — roadmap task **T26**.
//!
//! What is proved here is the *arithmetic* of a refresh: which files appear, which are left alone,
//! which are replaced and which are swept away. That a shim copied under a name then dispatches on
//! it is `crates/mixengine-shim/tests/shim.rs`', which runs the real binary; this suite copies a
//! few bytes instead, because nothing it asks depends on the copy being a program.

use std::path::{Path, PathBuf};

use mixengine_core::shims;

/// A `bin/` and a file standing in for the shim binary, in a directory this test owns.
struct Fixture {
    root: tempfile::TempDir,

    /// The commands beyond [`shims::COMMANDS`] this fixture hands to every refresh — roadmap tasks
    /// **T130** and **T131**. Empty for the suite that predates them, which is what keeps every
    /// assertion below about the compiled table alone.
    extra: Vec<shims::Extra>,

    /// The runtime kinds installed, which decides whose names a refresh writes — roadmap task
    /// **T185b**. Every kind, for the suite that predates it.
    installed: std::collections::BTreeSet<mixengine_proto::RuntimeKind>,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("a temporary directory");
        let fixture = Self {
            root,
            extra: Vec::new(),
            installed: shims::every_kind(),
        };

        // T185: on Windows what `bin/` is filled from is the trampoline, and the resolver beside it
        // is only named in the pointer. Written once, because nothing here upgrades it.
        if cfg!(windows) {
            std::fs::write(fixture.resolver(), b"the resolver").expect("a resolver");
        }

        fixture.publish(b"the shim, build one");
        fixture
    }

    /// The same fixture, with `names` handed over as discovered Node tools.
    ///
    /// The origin is arbitrary here: what a refresh does with an extra does not depend on where it
    /// came from, and the suites that care about the origin are the daemon's.
    fn fronting(mut self, names: &[&str]) -> Self {
        self.extra = names
            .iter()
            .map(|name| shims::Extra {
                name: (*name).to_owned(),
                origin: shims::Origin::Global {
                    kind: mixengine_proto::RuntimeKind::Node,
                },
            })
            .collect();

        self
    }

    /// (Re)write the source binary with `contents`, which is how an upgrade is spelled here.
    ///
    /// A different length is what [`shims::refresh`] compares on first, and a length that happens
    /// to match is the case the modification time carries — `written_at` below is for that one.
    ///
    /// **The old file is unlinked rather than truncated**, because that is what shipping a new
    /// build is: an installer renames one into place and a linker writes a new output, and neither
    /// hands the new bytes to a file the old ones were in. The difference is visible from `bin/`
    /// now that a shim there may be a second *name* for this file rather than a second copy of it —
    /// truncating would make every name in `bin/` hold build two the instant this returns, and the
    /// refresh below would have nothing to report, which is a fixture describing an upgrade nobody
    /// performs.
    fn publish(&self, contents: &[u8]) {
        let _ = std::fs::remove_file(self.shim());
        std::fs::write(self.shim(), contents).expect("a source binary");
    }

    fn bin(&self) -> PathBuf {
        self.root.path().join("bin")
    }

    /// The file `bin/` is filled from: the trampoline on Windows since T185, the shim elsewhere.
    fn shim(&self) -> PathBuf {
        let name = match cfg!(windows) {
            true => shims::TRAMPOLINE,
            false => shims::BINARY,
        };

        self.root
            .path()
            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
    }

    fn resolver(&self) -> PathBuf {
        self.root
            .path()
            .join(format!("{}{}", shims::BINARY, std::env::consts::EXE_SUFFIX))
    }

    fn refresh(&self) -> shims::Refreshed {
        let source = shims::source(&mixengined_in(self.root.path())).expect("the pair is there");

        shims::refresh(&self.bin(), &source, &self.installed, &self.extra)
            .expect("a writable temporary directory")
    }

    fn copy_of(&self, name: &str) -> PathBuf {
        self.bin().join(name)
    }

    fn php(&self) -> PathBuf {
        self.copy_of(&format!("php{}", std::env::consts::EXE_SUFFIX))
    }
}

/// The whole of what a first start does to a home that has never had one.
#[test]
fn a_bin_that_does_not_exist_yet_is_created_and_filled() {
    let fixture = Fixture::new();
    let refreshed = fixture.refresh();

    assert_eq!(refreshed.commands.len(), shims::COMMANDS.len());
    assert_eq!(refreshed.written, refreshed.commands, "all of them, once");
    assert!(refreshed.removed.is_empty() && refreshed.refused.is_empty());

    for command in shims::COMMANDS {
        let copy = fixture.copy_of(&shims::file_name(command));
        assert_eq!(
            std::fs::read(&copy).expect("a copy"),
            b"the shim, build one",
            "{} is not the shim",
            copy.display()
        );
    }
}

/// What that first pass **costs**, which is the half the case above does not ask about.
///
/// A start used to move one whole shim binary per row, and `mixengine-shim` with its debug info in
/// it is tens of megabytes: four daemons filling four fresh homes on one CI runner spent thirty
/// seconds each doing it, and the fourth was still copying when the client waiting for it gave up.
/// Nineteen names for one file is the answer, and the inode is the only way to state it — the
/// contents are equal either way, which is exactly why the assertion above cannot see the
/// difference.
///
/// **Unix only, and with no Windows twin on purpose.** A shim there stays alive as the parent of
/// the program it started, so a link would let a `php -S` somebody left running hold the shim binary
/// itself open against the next upgrade; `shims::place` copies there deliberately, and a test
/// asserting the copy would be asserting the cost rather than the reason for it.
#[cfg(unix)]
#[test]
fn nineteen_commands_are_nineteen_names_for_one_file() {
    use std::os::unix::fs::MetadataExt as _;

    let fixture = Fixture::new();
    fixture.refresh();

    let shim = fixture.shim().metadata().expect("the source binary");

    for command in shims::COMMANDS {
        let copy = fixture.copy_of(&shims::file_name(command));
        let placed = copy.metadata().expect("a shim in bin/");

        assert_eq!(
            (placed.dev(), placed.ino()),
            (shim.dev(), shim.ino()),
            "{} is a second copy of the shim rather than a second name for it",
            copy.display()
        );
    }
}

/// A daemon is restarted many times a day, and every start calls this. Copying nineteen multi-megabyte
/// files each time would be the most expensive thing a start does.
#[test]
fn a_second_pass_over_an_intact_bin_copies_nothing() {
    let fixture = Fixture::new();
    fixture.refresh();

    let refreshed = fixture.refresh();

    assert!(refreshed.written.is_empty(), "{:?}", refreshed.written);
    assert_eq!(refreshed.commands.len(), shims::COMMANDS.len());
}

/// The upgrade case: a new build of the shim replaces every copy of the old one.
#[test]
fn a_shim_of_a_different_length_replaces_every_copy() {
    let fixture = Fixture::new();
    fixture.refresh();

    fixture.publish(b"the shim, build two, which is longer than build one");
    let refreshed = fixture.refresh();

    assert_eq!(refreshed.written, refreshed.commands, "all of them again");
    assert_eq!(
        std::fs::read(fixture.php()).expect("a copy"),
        b"the shim, build two, which is longer than build one"
    );
}

/// The repair a person performs by deleting a file: a start puts back what is missing, and only
/// what is missing.
#[test]
fn a_deleted_command_comes_back_and_the_rest_are_left_alone() {
    let fixture = Fixture::new();
    fixture.refresh();

    std::fs::remove_file(fixture.php()).expect("removable");

    let refreshed = fixture.refresh();

    assert_eq!(
        refreshed.written,
        vec![format!("php{}", std::env::consts::EXE_SUFFIX)],
        "one file was missing"
    );
    assert!(fixture.php().is_file());
}

/// `bin/` is entirely MixEngine's, which is what lets a refresh remove what it does not recognise —
/// a command that was renamed between releases would otherwise stay on the user's PATH forever,
/// running a shim that answers to nobody.
#[test]
fn a_name_no_command_answers_to_is_removed() {
    let fixture = Fixture::new();
    fixture.refresh();

    let stranger = fixture.copy_of("php7");
    std::fs::write(&stranger, b"from a MixEngine that is not this one").expect("a file");

    let refreshed = fixture.refresh();

    assert_eq!(refreshed.removed, vec!["php7".to_owned()]);
    assert!(refreshed.refused.is_empty());
    assert!(!stranger.exists());
}

/// What removing the home does first, and the one place a copy that will not go is reported rather
/// than fatal.
#[test]
fn clearing_takes_bin_back_to_nothing() {
    let fixture = Fixture::new();
    fixture.refresh();

    let cleared = shims::clear(&fixture.bin()).expect("a readable directory");

    assert_eq!(cleared.removed.len(), shims::COMMANDS.len());
    assert!(cleared.refused.is_empty());
    assert_eq!(
        std::fs::read_dir(fixture.bin())
            .expect("still a directory")
            .count(),
        0
    );

    // A home that never had a `bin/` is cleared without one being created to report on.
    let empty = tempfile::tempdir().expect("a temporary directory");
    let nothing = empty.path().join("bin");
    assert!(
        shims::clear(&nothing)
            .expect("nothing to do")
            .removed
            .is_empty()
    );
    assert!(!nothing.exists());
}

/// Where the shim is looked for, and what a broken installation is told.
#[test]
fn the_shim_is_found_beside_the_program_that_asks_for_it() {
    let fixture = Fixture::new();
    let mixengined = fixture
        .root
        .path()
        .join(format!("mixengined{}", std::env::consts::EXE_SUFFIX));

    assert_eq!(
        shims::source(&mixengined).expect("beside it").placed,
        fixture.shim()
    );

    let elsewhere = fixture.root.path().join("sbin").join("mixengined");
    let error = shims::source(&elsewhere).expect_err("nothing is beside it");

    assert!(
        matches!(error, mixengine_core::Error::ShimMissing { .. }),
        "{error}"
    );
}

/// Not a claim about any one filesystem: the table is what `bin/` is named from, and a row whose
/// name could not be a file would be a command nobody can type on the system that refuses it.
#[test]
fn every_command_becomes_a_file_name_this_system_can_run() {
    for command in shims::COMMANDS {
        let name = shims::file_name(command);

        assert!(name.starts_with(command.name));
        assert!(
            Path::new(&name)
                .file_name()
                .is_some_and(|found| found == name.as_str()),
            "{name} is not a single path component"
        );
        assert_eq!(
            name.ends_with(".exe"),
            cfg!(windows),
            "the suffix is the loader's rule: {name}"
        );
    }
}

/// **A command beyond the compiled table is a copy of the same shim** — roadmap tasks **T130** and
/// **T131**. The whole of what `bin/` had to learn: `mysqldump` and `yarn` are names, and the
/// program behind each of them is the one binary that reads its own file name.
#[test]
fn a_command_beyond_the_table_is_put_in_bin() {
    let fixture = Fixture::new().fronting(&["yarn", "mysqldump"]);
    let refreshed = fixture.refresh();

    for name in ["yarn", "mysqldump"] {
        let copy = fixture.copy_of(&format!("{name}{}", std::env::consts::EXE_SUFFIX));
        assert!(copy.is_file(), "{} was not written", copy.display());
    }

    assert_eq!(
        refreshed.commands.len(),
        mixengine_core::shims::COMMANDS.len() + 2
    );
}

/// And it is swept the moment it stops being handed over. `bin/` is a projection of what is
/// installed, so a `yarn` whose Node was uninstalled is a word on somebody's PATH that runs a
/// program with nothing behind it — which is exactly what this directory's sweep exists to prevent.
#[test]
fn a_command_no_longer_handed_over_is_removed() {
    let fixture = Fixture::new().fronting(&["yarn"]);
    fixture.refresh();

    let yarn = fixture.copy_of(&format!("yarn{}", std::env::consts::EXE_SUFFIX));
    assert!(yarn.is_file(), "the first pass wrote it");

    let fixture = Fixture {
        extra: Vec::new(),
        ..fixture
    };
    let refreshed = fixture.refresh();

    assert!(!yarn.exists(), "the second pass left it there");
    assert!(
        refreshed
            .removed
            .iter()
            .any(|name| name.starts_with("yarn")),
        "{:?}",
        refreshed.removed
    );
}

/// **A compiled row is never displaced by an extra of the same name.** Somebody who runs
/// `npm install -g npm` has an `npm` in their Node's global directory; fronting *that* would make
/// `bin/npm` a shim that dispatches to a file found by a shim, and the first thing it would find is
/// itself.
#[test]
fn a_compiled_command_wins_over_an_extra_of_the_same_name() {
    let fixture = Fixture::new().fronting(&["npm"]);
    let refreshed = fixture.refresh();

    assert_eq!(
        refreshed.commands.len(),
        mixengine_core::shims::COMMANDS.len(),
        "npm was fronted twice: {:?}",
        refreshed.commands
    );
}

/// One name handed over twice is the caller describing one disk badly, not a reason to write the
/// file twice or to sweep it between the two writes.
#[test]
fn one_name_is_written_once() {
    let fixture = Fixture::new().fronting(&["yarn", "yarn"]);
    let refreshed = fixture.refresh();

    assert_eq!(
        refreshed.commands.len(),
        mixengine_core::shims::COMMANDS.len() + 1,
        "{:?}",
        refreshed.commands
    );
}

/// A resolver and a trampoline side by side with distinct bytes, the way a Windows release ships
/// them, and the `bin/` a refresh fills from them — T185.
fn an_install_with_both() -> (tempfile::TempDir, PathBuf) {
    let install = tempfile::tempdir().expect("a temporary directory");
    let exe = std::env::consts::EXE_SUFFIX;

    std::fs::write(
        install.path().join(format!("mixengine-shim{exe}")),
        b"resolver",
    )
    .expect("a resolver");
    std::fs::write(
        install.path().join(format!("mixengine-trampoline{exe}")),
        b"trampoline",
    )
    .expect("a trampoline");

    let bin = install.path().join("bin");
    (install, bin)
}

fn mixengined_in(directory: &Path) -> PathBuf {
    directory.join(format!("mixengined{}", std::env::consts::EXE_SUFFIX))
}

/// T185: on Windows each name is the trampoline, and `bin/` says where the resolver is.
#[test]
fn on_windows_bin_holds_the_trampoline_and_points_at_the_resolver() {
    if !cfg!(windows) {
        return;
    }

    let (install, bin) = an_install_with_both();
    let source = shims::source(&mixengined_in(install.path())).expect("both are there");

    shims::refresh(&bin, &source, &shims::every_kind(), &[]).expect("refresh");

    assert_eq!(
        std::fs::read(bin.join("php.exe")).expect("php"),
        b"trampoline"
    );
    assert_eq!(
        std::fs::read_to_string(bin.join("mixengine-shim.path")).expect("the pointer"),
        source.resolver.to_str().expect("a Unicode temporary path")
    );
}

/// The pointer is not a stranger: a second refresh keeps it and reports nothing removed.
#[test]
fn a_second_refresh_keeps_the_pointer() {
    if !cfg!(windows) {
        return;
    }

    let (install, bin) = an_install_with_both();
    let source = shims::source(&mixengined_in(install.path())).expect("both are there");

    shims::refresh(&bin, &source, &shims::every_kind(), &[]).expect("first");
    let second = shims::refresh(&bin, &source, &shims::every_kind(), &[]).expect("second");

    assert!(bin.join("mixengine-shim.path").is_file());
    assert!(second.removed.is_empty(), "{second:?}");
    assert!(second.written.is_empty(), "{second:?}");
}

/// **An install that updated itself onto T185 has no trampoline**, because an update never adds a
/// binary the install did not have (`updates::apply::swap`, rule 1). `bin/` must still be filled:
/// with the shim itself, as before T185 — heavier, and every command still works.
#[test]
fn on_windows_an_install_without_the_trampoline_fills_bin_with_the_shim() {
    if !cfg!(windows) {
        return;
    }

    let install = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(install.path().join("mixengine-shim.exe"), b"resolver").expect("a resolver");
    let bin = install.path().join("bin");

    let source = shims::source(&mixengined_in(install.path())).expect("the shim alone is enough");
    assert_eq!(source.placed, source.resolver);

    shims::refresh(&bin, &source, &shims::every_kind(), &[]).expect("refresh");
    assert_eq!(
        std::fs::read(bin.join("php.exe")).expect("php"),
        b"resolver"
    );
}

/// And the trampoline is still what a full install gets, once it has one: the next start after a
/// reinstall moves `bin/` over to it.
#[test]
fn on_windows_a_trampoline_that_arrives_later_replaces_the_shim_copies() {
    if !cfg!(windows) {
        return;
    }

    let install = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(install.path().join("mixengine-shim.exe"), b"resolver").expect("a resolver");
    let bin = install.path().join("bin");
    let mixengined = mixengined_in(install.path());

    shims::refresh(
        &bin,
        &shims::source(&mixengined).expect("shim"),
        &shims::every_kind(),
        &[],
    )
    .expect("first");
    std::fs::write(
        install.path().join("mixengine-trampoline.exe"),
        b"trampoline",
    )
    .expect("a trampoline");
    shims::refresh(
        &bin,
        &shims::source(&mixengined).expect("both"),
        &shims::every_kind(),
        &[],
    )
    .expect("second");

    assert_eq!(
        std::fs::read(bin.join("php.exe")).expect("php"),
        b"trampoline"
    );
}

/// Unix is unchanged: the resolver under every name, and no pointer.
#[test]
fn elsewhere_bin_is_the_resolver_and_there_is_no_pointer() {
    if cfg!(windows) {
        return;
    }

    let install = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(install.path().join("mixengine-shim"), b"resolver").expect("a resolver");
    let bin = install.path().join("bin");

    let source = shims::source(&mixengined_in(install.path())).expect("the shim is there");
    shims::refresh(&bin, &source, &shims::every_kind(), &[]).expect("refresh");

    assert_eq!(std::fs::read(bin.join("php")).expect("php"), b"resolver");
    assert!(!bin.join("mixengine-shim.path").exists());
}

/// **T185b.** A runtime's names are in `bin/` only while one of its versions is installed: none on
/// a home with nothing, exactly the kind's rows once one is, and gone again when the last goes.
#[test]
fn only_an_installed_runtime_has_its_names_in_bin() {
    use mixengine_proto::RuntimeKind;

    let mut fixture = Fixture::new();
    let exe = |name: &str| format!("{name}{}", std::env::consts::EXE_SUFFIX);

    fixture.installed = std::collections::BTreeSet::new();
    let refreshed = fixture.refresh();
    for name in ["php", "node", "npm", "python", "ruby", "composer"] {
        assert!(
            !fixture.bin().join(exe(name)).exists(),
            "{name} with nothing installed"
        );
    }
    assert!(
        refreshed
            .commands
            .iter()
            .all(|name| !name.starts_with("node")),
        "{:?}",
        refreshed.commands
    );

    fixture.installed = [RuntimeKind::Node].into_iter().collect();
    fixture.refresh();
    for name in ["node", "npm", "npx"] {
        assert!(
            fixture.bin().join(exe(name)).exists(),
            "{name} with Node installed"
        );
    }
    assert!(!fixture.bin().join(exe("php")).exists(), "php without PHP");

    fixture.installed = std::collections::BTreeSet::new();
    fixture.refresh();
    assert!(
        !fixture.bin().join(exe("node")).exists(),
        "node after the last Node went"
    );
}

/// **T185b.** Composer is run by PHP, so its name needs both installed.
#[test]
fn composer_needs_both_composer_and_php() {
    use mixengine_proto::RuntimeKind;

    let mut fixture = Fixture::new();
    let composer = fixture
        .bin()
        .join(format!("composer{}", std::env::consts::EXE_SUFFIX));

    fixture.installed = [RuntimeKind::Composer].into_iter().collect();
    fixture.refresh();
    assert!(!composer.exists(), "Composer with no PHP to run it");

    fixture.installed = [RuntimeKind::Composer, RuntimeKind::Php]
        .into_iter()
        .collect();
    fixture.refresh();
    assert!(composer.exists(), "Composer with a PHP");
}
