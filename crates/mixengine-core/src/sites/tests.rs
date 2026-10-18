use super::*;
use crate::ProjectError;
use mixengine_proto::Timestamp;

async fn home() -> (tempfile::TempDir, Store, i64) {
    let temp = tempfile::tempdir().expect("a temporary directory");
    let store = Store::open(&temp.path().join("mixengine.db"))
        .await
        .expect("a database");
    let project = crate::projects::create(
        &store,
        &crate::projects::Registration {
            name: "blog".to_owned(),
            root: temp.path().join("blog"),
            pins: std::collections::BTreeMap::new(),
        },
        Timestamp::from_system_time(std::time::SystemTime::UNIX_EPOCH),
    )
    .await
    .expect("a project");

    (temp, store, project.id)
}

fn php() -> SiteKind {
    SiteKind::PhpFpm { pool: None }
}

mod domains;
mod pools;
mod sharing;
