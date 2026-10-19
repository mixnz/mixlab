//! What a runtime upgrade decides, tested without a daemon.

use super::*;

fn survey(extension_pools: Vec<ExtensionPool>) -> Survey {
    Survey {
        pools: None,
        sites: Vec::new(),
        extension_pools,
        dropped: Vec::new(),
        pins: Vec::new(),
        default: false,
        manifests: Vec::new(),
        tools: Vec::new(),
    }
}

fn pool(id: &str, moves: bool) -> ExtensionPool {
    ExtensionPool {
        pool: ServiceId::parse(id).expect("a valid id"),
        moves,
        requires: None,
    }
}

#[test]
fn the_old_version_is_kept_when_asked_or_needed() {
    assert_eq!(
        kept_without_trying(true, vec!["blog pins php ^8.3.33".to_owned()]),
        Some(OldVersion::Kept {
            because: vec!["you asked to keep it".to_owned()],
        }),
        "being asked outranks every other reason"
    );
    assert_eq!(
        kept_without_trying(false, vec!["blog pins php ^8.3.33".to_owned()]),
        Some(OldVersion::Kept {
            because: vec!["blog pins php ^8.3.33".to_owned()],
        })
    );
    assert_eq!(
        kept_without_trying(false, Vec::new()),
        None,
        "nothing keeps it, so the uninstall is tried"
    );
}

#[test]
fn only_the_extension_pools_that_move_are_moved() {
    let surveyed = survey(vec![
        pool("wiki@8.3.33", true),
        pool("shop@8.3.33", false),
        pool("forum@8.3.33", true),
    ]);

    assert_eq!(
        moving_pools(&surveyed),
        vec![
            ServiceId::parse("wiki@8.3.33").expect("a valid id"),
            ServiceId::parse("forum@8.3.33").expect("a valid id"),
        ]
    );
}
