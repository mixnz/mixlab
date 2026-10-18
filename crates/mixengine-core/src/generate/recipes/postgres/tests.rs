use std::path::Path;

use mixengine_proto::ServiceId;

use super::*;
use crate::ServiceError;
use crate::generate::Upstream;
use crate::generate::first_run::FirstRun;
use crate::generate::recipe;
use crate::generate::settings::Settings;

/// An absolute path on whichever system this is compiled for.
const fn root() -> &'static str {
    if cfg!(windows) {
        r"C:\MixEngine"
    } else {
        "/opt/mixengine"
    }
}

/// What the artifact publishes, as `mixengine-packages` publishes it.
///
/// One spelling per command on every route — the Debian cell buys that with a symlink — so this
/// is a layout rather than a search. The five required ones only: the artifact publishes
/// fifteen, and a recipe may rely on these.
fn provides() -> BTreeMap<String, String> {
    ["postgres", "initdb", "pg_ctl", "psql", "pg_isready"]
        .into_iter()
        .map(|name| {
            (
                name.to_owned(),
                format!("bin/{name}{}", std::env::consts::EXE_SUFFIX),
            )
        })
        .collect()
}

mod bootstrap;
mod server;

/// A `postgres@main` in a home at [`root`], with `overrides` applied.
fn context(overrides: &str) -> Context {
    with_provides(provides(), overrides)
}

/// The same, for an install that publishes something else — or nothing.
fn with_provides(provides: BTreeMap<String, String>, overrides: &str) -> Context {
    let service = ServiceId::parse("postgres@main").expect("an id");
    let settings =
        Settings::merge(Postgres.settings(), overrides, &service).expect("usable overrides");

    let context = Context::for_test(
        service,
        PACKAGE,
        Path::new(root()),
        provides,
        Some(5432),
        settings,
    );
    let endpoints = Postgres
        .endpoints(&context)
        .expect("a home this short has a usable socket path");

    context.with_endpoints(endpoints)
}

/// The built spec for `overrides`.
fn built(overrides: &str) -> ServiceSpec {
    Postgres
        .spec(&context(overrides))
        .expect("a spec")
        .build()
        .expect("a valid spec")
}
