//! What a plan reads out of a `[scaffold]` command: its program, when that can be told.
//!
//! Roadmap task **T78b**. A command is a line for `cmd.exe` or `sh`, and the one thing a plan can
//! decide about it up front is whether the program its first word names is there to be run. Only
//! that, and only when the first word is a bare name — every doubt resolves to *not judging*,
//! because a false `blocked` stops a blueprint that would have worked (the design's D2).

use std::collections::BTreeSet;
use std::ffi::OsStr;

use mixengine_proto::{Disposition, RuntimeKind};

/// Words either shell answers itself, without ever consulting `PATH`.
///
/// Small on purpose and written down once: `echo hello> made.txt` is what the scaffold suite runs
/// on Windows, and `printf` is a builtin to `dash`. A word here leaves the step to the shell.
const BUILTINS: &[&str] = &[
    "cd", "echo", "set", "exit", "type", "printf", "test", "true", "false", "export", ".", ":",
    "call", "start", "rem", "if", "for",
];

/// What makes a first word something other than a bare program name.
const NOT_A_BARE_NAME: &[char] = &[
    '"', '\'', '`', '$', '(', ')', '{', '}', '|', '&', ';', '<', '>', '/', '\\', '=',
];

/// The program `command` would run, when its first word is a bare name — [`None`] otherwise.
#[must_use]
pub fn bare_name(command: &str) -> Option<&str> {
    let first = command.split_whitespace().next()?;

    if first.contains(NOT_A_BARE_NAME) || BUILTINS.contains(&first.to_ascii_lowercase().as_str()) {
        return None;
    }

    Some(first)
}

/// The programs that read a package name out of the directory they are run in.
///
/// Short, and every entry measured rather than assumed: all four read `package.json`'s `name` from
/// the folder's basename when they initialise into `.`, which is the property
/// [`crate::blueprints::plan::not_an_npm_name`] judges.
const NPM_FAMILY: &[&str] = &["npx", "npm", "yarn", "pnpm"];

/// Whether `command` runs one of the npm family — roadmap task **T120c**.
///
/// **A guess, and admitted to be one**, which is why nothing that *blocks* anybody uses it: the
/// design's D3 has the blueprint declare `needs_npm_safe_dir` rather than have a plan read tea
/// leaves out of a command string. This answers a smaller question, asked only after a scaffold has
/// already failed — may the failure add npm's rule to what it says? A miss there costs one hint. A
/// wrong guess costs a false sentence: `composer create-project` takes its package name from its
/// argument, so telling somebody their folder is the problem would send them renaming it for
/// nothing.
///
/// [`bare_name`]'s own doubts are inherited whole. A command whose first word is quoted, a path or
/// a shell construct is not judged, so `./node_modules/.bin/create-next-app .` is left alone.
#[must_use]
pub fn is_an_npm_command(command: &str) -> bool {
    bare_name(command).is_some_and(|name| NPM_FAMILY.contains(&name.to_ascii_lowercase().as_str()))
}

/// The shim `bin/` would hold under `name`, when one of [`crate::shims::COMMANDS`] answers to it.
///
/// Compared the way [`crate::shims::dispatch`] compares, case folded only on Windows — but on the
/// whole word, so `composer.phar` is not taken for the `composer` shim.
fn shim_named(name: &str) -> Option<&'static crate::shims::Command> {
    crate::shims::COMMANDS.iter().find(|command| {
        if cfg!(windows) {
            command.name.eq_ignore_ascii_case(name)
        } else {
            command.name == name
        }
    })
}

/// The scaffold step's disposition: `Confirm` unless its program is a bare name nothing will
/// answer to by the time the step runs, which is `Blocked` — decided here rather than at the end of
/// a job (the design's D1 and D3).
///
/// **`available` is every runtime kind there will be once the plan's runtimes are installed** —
/// installed already, or in the blueprint's `[runtimes]`. Since T185b `bin/` fronts only what is
/// installed, so on a fresh machine `bin/composer` is not there when the plan is made and is there
/// when the scaffold runs: the runtime steps ahead of it put it there. Judging the PATH alone
/// blocked every Composer entry in the gallery on exactly the machine it was meant for.
#[must_use]
pub fn disposition(
    command: &str,
    scaffold_path: &OsStr,
    available: &BTreeSet<RuntimeKind>,
) -> Disposition {
    let confirm = || Disposition::Confirm {
        what: command.to_owned(),
    };

    let Some(name) = bare_name(command) else {
        return confirm();
    };

    let shim = shim_named(name);

    if shim.is_some_and(|shim| {
        available.contains(&shim.kind) && shim.via.is_none_or(|via| available.contains(&via))
    }) {
        return confirm();
    }

    if mixengine_platform::process::program_on_path(name, scaffold_path).is_some() {
        return confirm();
    }

    // **The sentence names what is missing**, when that can be told: `composer` installed and no
    // PHP leaves no `bin/composer` at all, and "not on the PATH" sent somebody who had just
    // installed Composer looking for a PATH problem.
    let reason = match shim {
        Some(shim) if !available.contains(&shim.kind) => format!(
            "`{name}` comes with the {kind} runtime, which is not installed and this blueprint's \
             [runtimes] does not ask for",
            kind = shim.kind
        ),
        Some(crate::shims::Command { via: Some(via), .. }) => format!(
            "`{name}` runs on {via}, which is not installed and this blueprint's [runtimes] does \
             not ask for"
        ),
        _ => format!(
            "`{name}` is not on the PATH the command would run with (<home>/bin, then the \
             daemon's own PATH)"
        ),
    };

    Disposition::Blocked { reason }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Only a bare first word is judged** — roadmap task **T78b**, its design's D2.
    #[test]
    fn a_bare_first_word_is_the_program() {
        assert_eq!(
            bare_name("composer create-project laravel/laravel ."),
            Some("composer")
        );
        assert_eq!(
            bare_name("  npx --yes create-next-app@latest ."),
            Some("npx")
        );
    }

    /// Quotes, shell syntax, paths, assignments and builtins are all left to the shell.
    #[test]
    fn anything_that_is_not_a_bare_name_is_left_to_the_shell() {
        for command in [
            "echo hello> made.txt",
            "printf hello > made.txt",
            "ECHO hello",
            r#""C:\tools\run.exe" --flag"#,
            "'./run' now",
            "VAR=1 program",
            "./local --flag",
            r".\local.cmd",
            "$HOME/bin/tool",
            "",
            "   ",
        ] {
            assert_eq!(bare_name(command), None, "{command:?}");
        }
    }

    /// **Which commands D8's guess is allowed to speak about** — roadmap task **T120c**.
    #[test]
    fn the_npm_family_is_recognised_and_nothing_else_is() {
        for command in [
            "npx --yes create-next-app@latest . --yes",
            "npm init vite@latest .",
            "YARN create vite",
            "pnpm create next-app",
        ] {
            assert!(is_an_npm_command(command), "{command:?}");
        }

        for command in [
            "composer create-project laravel/laravel . --no-interaction",
            "php artisan install",
            "django-admin startproject .",
            // Not a bare name, so nothing is known about it — and D8 stays quiet.
            "./node_modules/.bin/create-next-app .",
            "",
        ] {
            assert!(!is_an_npm_command(command), "{command:?}");
        }
    }

    /// A program nothing on the PATH answers to is `Blocked`, naming it and both halves of the
    /// PATH it was looked for on (D3).
    #[test]
    fn a_missing_program_is_blocked_with_its_name_and_where_it_was_looked_for() {
        let judged = disposition("symfony new .", OsStr::new(""), &BTreeSet::new());

        let Disposition::Blocked { reason } = judged else {
            panic!("a missing program is blocked: {judged:?}");
        };
        assert!(reason.contains("`symfony`"), "{reason}");
        assert!(reason.contains("<home>/bin"), "{reason}");
        assert!(reason.contains("daemon's own PATH"), "{reason}");
    }

    /// A word the rule does not judge is `Confirm` even on an empty PATH.
    #[test]
    fn a_word_that_is_not_judged_is_something_to_agree_to() {
        assert!(matches!(
            disposition("echo hello> made.txt", OsStr::new(""), &BTreeSet::new()),
            Disposition::Confirm { what } if what == "echo hello> made.txt"
        ));
    }

    /// **A shim the plan's own runtimes will put in `bin/` is there to be run** — T78b as T185b
    /// left it. `bin/` is empty on a fresh machine and holds `composer` once PHP and Composer are
    /// installed, which the runtime steps do before the scaffold.
    #[test]
    fn a_shim_whose_runtimes_will_be_there_is_something_to_agree_to() {
        let both = BTreeSet::from([RuntimeKind::Php, RuntimeKind::Composer]);

        assert!(matches!(
            disposition(
                "composer create-project laravel/laravel .",
                OsStr::new(""),
                &both
            ),
            Disposition::Confirm { .. }
        ));
        assert!(matches!(
            disposition(
                "npx --yes create-next-app@latest .",
                OsStr::new(""),
                &BTreeSet::from([RuntimeKind::Node])
            ),
            Disposition::Confirm { .. }
        ));
    }

    /// **Composer without a PHP is no `composer` at all**, and the reason says which is missing
    /// rather than pointing at the PATH — reported from a machine with Composer installed alone.
    #[test]
    fn a_shim_missing_the_runtime_that_runs_it_names_that_runtime() {
        let judged = disposition(
            "composer create-project laravel/laravel .",
            OsStr::new(""),
            &BTreeSet::from([RuntimeKind::Composer]),
        );

        let Disposition::Blocked { reason } = judged else {
            panic!("composer without a PHP is blocked: {judged:?}");
        };
        assert!(reason.contains("`composer` runs on php"), "{reason}");
    }

    /// And a shim whose own runtime is missing names that one.
    #[test]
    fn a_shim_missing_its_own_runtime_names_it() {
        let judged = disposition("npx create-vite .", OsStr::new(""), &BTreeSet::new());

        let Disposition::Blocked { reason } = judged else {
            panic!("npx without a Node is blocked: {judged:?}");
        };
        assert!(reason.contains("comes with the node runtime"), "{reason}");
    }

    /// `composer.phar` is a file somebody keeps in their project, not the shim.
    #[test]
    fn only_the_whole_word_is_a_shim() {
        assert!(shim_named("composer.phar").is_none());
        assert!(shim_named("composer").is_some());
    }
}
