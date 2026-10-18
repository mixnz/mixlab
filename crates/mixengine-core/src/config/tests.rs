use super::*;

/// T191 D2: the template tells a person on Windows to write `/`; what the daemon uses is spelled
/// natively, so nothing built under it mixes the two.
#[test]
fn a_relocation_written_with_slashes_is_read_natively() {
    let written = if cfg!(windows) {
        "D:/bulk/data"
    } else {
        "/mnt/bulk/data"
    };
    let config: Config =
        toml::from_str(&format!("[paths]\ndata = \"{written}\"\n")).expect("a configuration");
    let data = config.paths.data.expect("set");

    // Compared as text: `Path`'s own `==` goes by components, and on Windows it calls
    // `D:/bulk/data` and `D:\bulk\data` equal — the very difference this is about.
    let expected = if cfg!(windows) {
        r"D:\bulk\data"
    } else {
        written
    };
    assert_eq!(data.as_os_str(), expected);
}

/// Single quotes and backslashes, the template's other advice, read exactly as before.
#[test]
fn a_relocation_written_with_backslashes_is_unchanged() {
    let config: Config =
        toml::from_str("[paths]\ndata = 'D:\\bulk\\data'\n").expect("a configuration");

    assert_eq!(config.paths.data.expect("set").as_os_str(), r"D:\bulk\data");
}

/// A share stays a share.
#[test]
fn a_relocation_onto_a_share_keeps_its_prefix() {
    let config: Config =
        toml::from_str("[paths]\ndata = '\\\\server\\share/bulk'\n").expect("a configuration");
    let data = config.paths.data.expect("set");

    if cfg!(windows) {
        assert_eq!(data.as_os_str(), r"\\server\share\bulk");
    }
}

/// `set_paths` writes the native spelling, escaped by `toml_edit`, and `load` reads it back.
#[test]
fn set_paths_writes_what_load_reads_back() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let file = directory.path().join(FILE_NAME);
    std::fs::write(&file, "").expect("an empty configuration");

    let asked = if cfg!(windows) {
        "D:/bulk/data"
    } else {
        "/mnt/bulk/data"
    };
    set_paths(
        &file,
        &RequestedPaths {
            data: Some(PathBuf::from(asked)),
            ..RequestedPaths::default()
        },
    )
    .expect("written");

    let written = std::fs::read_to_string(&file).expect("the file");
    if cfg!(windows) {
        assert!(!written.contains("D:/"), "{written}");
    }

    let read = load(&file).expect("read back").paths.data.expect("set");
    let expected = if cfg!(windows) {
        r"D:\bulk\data"
    } else {
        asked
    };
    assert_eq!(read.as_os_str(), expected);
}

/// **On unless the user says otherwise**, and the shipped template says so in words —
/// roadmap task **T124**.
#[test]
fn the_welcome_page_is_on_by_default() {
    let config: Config = toml::from_str("").expect("an empty configuration");
    assert!(config.sites.welcome_page);

    let off: Config = toml::from_str(
        "[sites]
welcome_page = false
",
    )
    .expect("a configuration");
    assert!(!off.sites.welcome_page);

    assert!(
        TEMPLATE.contains("[sites]"),
        "a key nothing in the template mentions is a key nobody finds"
    );
}

/// The default, and the one value this key refuses — roadmap task **T71a**.
///
/// A test at all because the key exists for the daemon's own suite to move: one that could not
/// be set would be a constant with a longer name.
#[test]
fn a_memory_watchdog_may_not_act_on_a_single_minute() {
    assert_eq!(
        Services::default().memory_over_minutes,
        DEFAULT_MEMORY_OVER_MINUTES
    );

    assert!(
        toml::from_str::<Config>("[services]\nmemory_over_minutes = 0\n").is_err(),
        "zero would restart a service on one reading, which is what a minute holds when \
             nobody is watching"
    );

    let taken: Config = toml::from_str("[services]\nmemory_over_minutes = 10\n")
        .expect("a number of minutes is a setting");

    assert_eq!(taken.services.memory_over_minutes, 10);
}
