use super::*;

#[test]
fn a_shim_knows_which_command_it_was_copied_to_be() {
    let bin = Path::new(if cfg!(windows) {
        r"C:\Users\someone\AppData\Local\MixEngine\bin"
    } else {
        "/home/someone/.local/share/mixengine/bin"
    });

    let php = dispatch(&bin.join(format!("php{}", std::env::consts::EXE_SUFFIX)))
        .expect("php is a command");
    assert_eq!(php.kind, RuntimeKind::Php);
    assert_eq!(php.executable, "php");

    // A name of the user's world that is a different name in the artifact's, which is the pair
    // of fields' whole reason.
    let python3 = dispatch(&bin.join("python3")).expect("python3 is a command");
    assert_eq!(python3.executable, "python");
    assert_eq!(python3.kind, RuntimeKind::Python);

    // The binary before it is copied into `bin/` under a name that means something.
    assert_eq!(dispatch(Path::new("mixengine-shim")), None);
    assert_eq!(dispatch(Path::new("cargo")), None, "not in any artifact");
}

/// The filesystem's rule, not a courtesy — see [`dispatch`].
#[test]
fn case_is_folded_exactly_where_the_filesystem_folds_it() {
    assert_eq!(dispatch(Path::new("PHP")).is_some(), cfg!(windows));
}

/// **One row runs through another kind** — roadmap task **T27c**, its design's D4. `composer`
/// is a file for a PHP, and every other row is a program of its own.
#[test]
fn only_composer_runs_through_another_kind() {
    for command in COMMANDS {
        match command.name {
            "composer" => {
                assert_eq!(command.kind, RuntimeKind::Composer);
                assert_eq!(command.executable, "composer");
                assert_eq!(command.via, Some(RuntimeKind::Php));
            }
            _ => assert_eq!(command.via, None, "{}", command.name),
        }
    }
}

/// Go's two commands — roadmap task **T27d**, its design's D4.
#[test]
fn go_fronts_go_and_gofmt() {
    for name in ["go", "gofmt"] {
        let command = dispatch(Path::new(name)).unwrap_or_else(|| panic!("{name} is a command"));

        assert_eq!(command.kind, RuntimeKind::Go);
        assert_eq!(command.executable, name);
        assert_eq!(command.via, None);
    }
}

/// Java's six commands — roadmap task **T27e**, its design's D4.
#[test]
fn java_fronts_the_six_commands_its_artifact_provides() {
    for name in ["java", "javac", "jar", "jshell", "keytool", "jlink"] {
        let command = dispatch(Path::new(name)).unwrap_or_else(|| panic!("{name} is a command"));

        assert_eq!(command.kind, RuntimeKind::Java);
        assert_eq!(command.executable, name);
        assert_eq!(command.via, None);
    }
}

/// Two rows with one name would make `bin/` a directory whose entries are decided by the order
/// of this table, which is not a thing anybody should have to know.
#[test]
fn no_two_commands_answer_to_the_same_name() {
    let mut names: Vec<&str> = COMMANDS.iter().map(|command| command.name).collect();
    names.sort_unstable();

    let mut unique = names.clone();
    unique.dedup();

    assert_eq!(names, unique, "a name is listed twice");
}

/// Every name has to be a filename on all three systems, since `bin/` is where it lands.
#[test]
fn every_command_is_a_name_a_file_can_have() {
    for command in COMMANDS {
        assert!(
            !command.name.is_empty()
                && command
                    .name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "{} is not a name to put in bin/",
            command.name
        );
    }
}
