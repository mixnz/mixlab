use std::path::Path;

use super::*;
use crate::ServiceError;

/// A `services` row, without the Phase 3 machinery that will eventually create one.
///
/// A service cannot exist without a parent even in a test — the `CHECK` demands one of the two
/// columns and `foreign_keys=ON` demands the row it names — which is the constraint doing its
/// job, not an obstacle to route around.
async fn service_row(store: &Store, id: &str, state: ServiceState) -> ServiceId {
    sqlx::query(
        "INSERT INTO packages (name, version, install_path, installed_at, source_url, sha256)
             VALUES (?, '1.0.0', '/packages/x', '2026-08-12T00:00:00Z', 'https://example', 'ab')
             ON CONFLICT (name, version) DO NOTHING",
    )
    .bind(id)
    .execute(store.pool())
    .await
    .expect("a package for the service to belong to");

    sqlx::query(
        "INSERT INTO services (id, package_id, instance_name, state)
             VALUES (?, (SELECT id FROM packages WHERE name = ?), 'main', ?)",
    )
    .bind(id)
    .bind(id)
    .bind(state.as_str())
    .execute(store.pool())
    .await
    .expect("the service row");

    ServiceId::parse(id).expect("a valid id")
}

/// A `packages` row for a service to be an instance of.
async fn package(store: &Store, name: &str) {
    sqlx::query(
        "INSERT INTO packages (name, version, install_path, installed_at, source_url, sha256)
             VALUES (?, '1.0.0', '/packages/x', '2026-08-21T00:00:00Z', 'https://example', 'ab')",
    )
    .bind(name)
    .execute(store.pool())
    .await
    .expect("a package for the service to belong to");
}

/// What a caller hands [`create`], with everything but the port left at its plainest.
fn declaration(id: &str, package: &str, port: Port) -> Declaration {
    Declaration {
        service: ServiceId::parse(id).expect("a valid id"),
        origin: Origin::Package {
            name: package.to_owned(),
            version: PackageVersion::parse("1.0.0").expect("a version"),
        },
        instance_name: "main".to_owned(),
        port,
        bind_addr: None,
        data_dir: None,
        autostart: false,
        overrides: "{}".to_owned(),
    }
}

mod declarations;
mod state;

async fn store() -> (tempfile::TempDir, Store) {
    let home = tempfile::tempdir().expect("a temporary directory");
    let store = Store::open(&home.path().join(crate::paths::DATABASE_FILE_NAME))
        .await
        .expect("a database");
    (home, store)
}
