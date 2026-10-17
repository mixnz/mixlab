use super::expand_home;
use std::path::{Path, PathBuf};

#[test]
fn a_path_under_home_is_read_from_this_machines_home() {
    let home = Path::new("/home/me");
    assert_eq!(
        expand_home("~/.ssh/id_rsa", Some(home)),
        home.join(".ssh/id_rsa")
    );
    assert_eq!(
        expand_home("~\\.ssh\\id_rsa", Some(home)),
        home.join(".ssh\\id_rsa")
    );
}

#[test]
fn any_other_path_is_left_as_it_was_written() {
    let home = Some(Path::new("/home/me"));
    assert_eq!(
        expand_home("/opt/keys/id", home),
        PathBuf::from("/opt/keys/id")
    );
    assert_eq!(expand_home("~other/id", home), PathBuf::from("~other/id"));
    assert_eq!(expand_home("~/id", None), PathBuf::from("~/id"));
}
