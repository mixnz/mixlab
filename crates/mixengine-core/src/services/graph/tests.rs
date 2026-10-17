use std::path::PathBuf;

use mixengine_proto::{Millis, ReadyCheck};

use super::*;

fn id(value: &str) -> ServiceId {
    ServiceId::parse(value).expect("a valid service id")
}

/// A spec that describes nothing interesting, so a test can be about its edges and nothing else.
fn spec(name: &str, depends_on: &[&str]) -> ServiceSpec {
    let root: PathBuf = if cfg!(windows) {
        r"C:\MixEngine".into()
    } else {
        "/opt/mixengine".into()
    };

    let mut builder = ServiceSpec::builder(id(name), root.join(name))
        .cwd(root)
        .ready(ReadyCheck::PidAlive {
            settle: Millis::from_secs(1),
        });

    for dependency in depends_on {
        builder = builder.depends_on(id(dependency));
    }

    builder.build().expect("a usable spec")
}

/// A spec carrying edges [`mixengine_proto::ServiceSpecBuilder::build`] would refuse.
///
/// Not a contrived shape: `ServiceSpec`'s own documentation says deserialisation runs no checks
/// and leaves them to whoever loads it, so this is exactly what a `services` row or an
/// `extension.toml` hands the graph when nobody called `validate` on the way.
fn unchecked(name: &str, depends_on: &[&str]) -> ServiceSpec {
    let edges = depends_on
        .iter()
        .map(|id| format!(r#""{id}""#))
        .collect::<Vec<_>>()
        .join(",");

    let encoded = serde_json::to_string(&spec(name, &[])).expect("a spec serialises");
    serde_json::from_str(
        &encoded.replace(r#""depends_on":[]"#, &format!(r#""depends_on":[{edges}]"#)),
    )
    .expect("deserialisation deliberately does not validate")
}

fn graph(specs: &[(&str, &[&str])]) -> ServiceGraph {
    ServiceGraph::new(
        specs
            .iter()
            .map(|(name, depends_on)| spec(name, depends_on)),
    )
    .expect("an acyclic graph")
}

/// `["caddy", "mariadb"]` reads better in an assertion than a `Vec<ServiceId>` does.
fn names<'a>(ids: impl IntoIterator<Item = &'a ServiceId>) -> Vec<&'a str> {
    ids.into_iter().map(ServiceId::as_str).collect()
}

fn tiers(plan: &Plan) -> Vec<Vec<&str>> {
    plan.tiers().iter().map(names).collect()
}

#[test]
fn a_dependency_is_in_an_earlier_tier_than_what_needs_it() {
    let graph = graph(&[
        ("caddy", &[]),
        ("mariadb", &[]),
        ("php-fpm", &["mariadb", "redis"]),
        ("redis", &[]),
    ]);

    assert_eq!(
        tiers(&graph.start_order()),
        vec![vec!["caddy", "mariadb", "redis"], vec!["php-fpm"]],
        "the three that wait for nothing share a tier; the one that waits comes after"
    );
}

/// The property the whole module exists for, asserted as a property rather than as one expected
/// list: a plan is valid exactly when nothing appears in it before something it needs.
#[test]
fn nothing_is_ever_planned_before_what_it_depends_on() {
    let graph = graph(&[
        ("caddy", &["php-fpm"]),
        ("php-fpm", &["mariadb", "redis"]),
        ("mariadb", &[]),
        ("redis", &["mariadb"]),
        ("mailpit", &[]),
    ]);

    let plan = graph.start_order();
    let order = names(plan.flat());

    for (position, service) in order.iter().enumerate() {
        for dependency in graph
            .dependencies_of(&id(service))
            .expect("a service of this graph")
        {
            let at = order
                .iter()
                .position(|planned| *planned == dependency.as_str())
                .expect("a full plan holds every dependency");

            assert!(at < position, "{service} was planned before {dependency}");
        }
    }
}

#[test]
fn stopping_is_the_other_way_round() {
    let graph = graph(&[
        ("caddy", &["php-fpm"]),
        ("php-fpm", &["mariadb"]),
        ("mariadb", &[]),
    ]);

    assert_eq!(
        names(graph.stop_order().flat()),
        vec!["caddy", "php-fpm", "mariadb"],
        "the database is the last thing to go"
    );
}

/// Starting one service is a request to have what it needs, so the plan grows downwards.
#[test]
fn a_start_plan_pulls_in_what_the_roots_need() {
    let graph = graph(&[
        ("caddy", &["php-fpm"]),
        ("php-fpm", &["mariadb"]),
        ("mariadb", &[]),
        ("mailpit", &[]),
    ]);

    let plan = graph
        .start_plan(std::iter::once(&id("caddy")))
        .expect("caddy is in the graph");

    assert_eq!(names(plan.flat()), vec!["mariadb", "php-fpm", "caddy"]);
    assert!(
        !plan.contains(&id("mailpit")),
        "a service nothing asked for and nothing needs is not started"
    );
}

/// And stopping one is a decision about everything that would be left pointing at it, so the
/// same graph plans the other way.
#[test]
fn a_stop_plan_pulls_in_what_needs_the_roots() {
    let graph = graph(&[
        ("caddy", &["php-fpm"]),
        ("php-fpm", &["mariadb"]),
        ("mariadb", &[]),
        ("mailpit", &[]),
    ]);

    let plan = graph
        .stop_plan(std::iter::once(&id("mariadb")))
        .expect("mariadb is in the graph");

    assert_eq!(
        names(plan.flat()),
        vec!["caddy", "php-fpm", "mariadb"],
        "what depends on the database goes down before the database does"
    );
    assert!(!plan.contains(&id("mailpit")));
}

/// Both directions are public, answer in the same shape, and name the service rather than
/// shrugging when it is not here — T19 reads one to spawn and the other to give up.
#[test]
fn a_service_knows_both_what_it_needs_and_what_needs_it() {
    let graph = graph(&[
        ("caddy", &["php-fpm"]),
        ("php-fpm", &["mariadb"]),
        ("mariadb", &[]),
    ]);

    assert_eq!(
        names(
            graph
                .dependencies_of(&id("php-fpm"))
                .expect("a known service")
        ),
        vec!["mariadb"]
    );
    assert_eq!(
        names(
            graph
                .dependents_of(&id("php-fpm"))
                .expect("a known service")
        ),
        vec!["caddy"]
    );
    assert!(
        graph
            .dependencies_of(&id("mariadb"))
            .expect("a known service")
            .is_empty(),
        "the bottom of the graph needs nothing"
    );
    assert!(
        graph
            .dependents_of(&id("caddy"))
            .expect("a known service")
            .is_empty(),
        "and the top holds nothing up"
    );

    for outcome in [
        graph.dependencies_of(&id("mailpit")),
        graph.dependents_of(&id("mailpit")),
    ] {
        assert_eq!(
            outcome.expect_err("mailpit is not in this graph"),
            GraphError::NoSuchService { id: id("mailpit") }
        );
    }
}

/// What T19 spawns from: the plan names a service, and the graph is where its spec comes from.
#[test]
fn the_spec_that_went_in_is_the_spec_that_comes_back() {
    let graph = graph(&[("caddy", &[]), ("mariadb", &[])]);

    assert_eq!(graph.spec(&id("caddy")), Some(&spec("caddy", &[])));
    assert_eq!(graph.spec(&id("mailpit")), None);
}

#[test]
fn a_plan_for_nothing_asks_for_nothing() {
    let graph = graph(&[("caddy", &[]), ("mariadb", &[])]);

    let plan = graph.start_plan(std::iter::empty()).expect("no roots");

    assert!(plan.is_empty());
    assert_eq!(plan.len(), 0);
}

#[test]
fn a_root_that_is_not_a_service_is_named() {
    let graph = graph(&[("caddy", &[])]);

    let error = graph
        .start_plan(std::iter::once(&id("mariadb")))
        .expect_err("no such service");

    assert_eq!(error, GraphError::NoSuchService { id: id("mariadb") });
}

/// What T19 asks after a service fails to start: everything that can now never come up.
#[test]
fn a_failure_blocks_everything_downstream_of_it() {
    let graph = graph(&[
        ("caddy", &["php-fpm"]),
        ("php-fpm", &["mariadb"]),
        ("mariadb", &[]),
        ("redis", &[]),
    ]);

    let blocked = graph.blocked_by(&id("mariadb")).expect("a known service");

    assert_eq!(
        names(&blocked),
        vec!["caddy", "php-fpm"],
        "the whole chain, not only the service that named it"
    );
    assert!(
        graph
            .blocked_by(&id("redis"))
            .expect("a known service")
            .is_empty(),
        "nothing declared a dependency on redis"
    );
}

#[test]
fn a_loop_is_refused_and_written_out() {
    let error = ServiceGraph::new([
        spec("caddy", &["php-fpm"]),
        spec("php-fpm", &["mariadb"]),
        spec("mariadb", &["caddy"]),
    ])
    .expect_err("a loop is not a graph");

    let GraphError::Cycle { path } = &error else {
        panic!("{error:?}");
    };

    assert_eq!(path.len(), 3, "each service once, closed by the rendering");
    assert_eq!(
        error.to_string(),
        "these services depend on each other in a loop: caddy → php-fpm → mariadb → caddy"
    );
}

/// The tail of a chain can be a loop without the head being in it, and the message has to name
/// the loop rather than the walk that found it.
#[test]
fn a_loop_reached_through_a_chain_names_only_the_loop() {
    let error = ServiceGraph::new([
        spec("caddy", &["php-fpm"]),
        spec("php-fpm", &["mariadb"]),
        spec("mariadb", &["redis"]),
        spec("redis", &["mariadb"]),
    ])
    .expect_err("a loop is not a graph");

    let GraphError::Cycle { path } = &error else {
        panic!("{error:?}");
    };

    assert_eq!(
        names(path),
        vec!["mariadb", "redis"],
        "caddy and php-fpm lead to the loop but are not in it"
    );
}

/// A spec cannot be *built* depending on itself, so the only way to hold one is the way a
/// `services` row or an `extension.toml` arrives: deserialised, where nothing is checked.
#[test]
fn a_service_that_depends_on_itself_is_a_loop_of_one() {
    let error = ServiceGraph::new([unchecked("caddy", &["caddy"])]).expect_err("a loop of one");

    assert_eq!(
        error,
        GraphError::Cycle {
            path: vec![id("caddy")]
        }
    );
}

/// The other thing only an unvalidated spec can carry, and the one that must **not** be read as
/// a loop: an edge written twice is one edge. Counted twice it would leave `php-fpm` waiting on
/// a `mariadb` that can only ever be discharged once, which is indistinguishable from a cycle
/// from the inside — and would be reported as one, with no loop to name.
#[test]
fn a_dependency_listed_twice_is_one_edge_rather_than_a_loop() {
    let graph = ServiceGraph::new([
        unchecked("php-fpm", &["mariadb", "mariadb"]),
        spec("mariadb", &[]),
    ])
    .expect("a repeated edge is still one edge");

    assert_eq!(
        names(
            graph
                .dependencies_of(&id("php-fpm"))
                .expect("a known service")
        ),
        vec!["mariadb"],
        "the graph holds edges, not the list as it was written"
    );
    assert_eq!(
        names(graph.start_order().flat()),
        vec!["mariadb", "php-fpm"]
    );
    assert_eq!(names(graph.stop_order().flat()), vec!["php-fpm", "mariadb"]);
}

#[test]
fn a_dependency_that_is_not_here_is_refused_rather_than_ignored() {
    let error =
        ServiceGraph::new([spec("php-fpm", &["mariadb"])]).expect_err("mariadb is not declared");

    assert_eq!(
        error,
        GraphError::UnknownDependency {
            service: id("php-fpm"),
            dependency: id("mariadb"),
        }
    );
}

#[test]
fn the_same_id_twice_is_refused_rather_than_merged() {
    let error = ServiceGraph::new([spec("caddy", &[]), spec("caddy", &[])])
        .expect_err("one of the two would have won silently");

    assert_eq!(error, GraphError::Duplicate { id: id("caddy") });
}

/// A graph of nothing is a legal graph — it is what a fresh install has — and every question
/// must answer rather than panic.
#[test]
fn an_empty_graph_answers_everything() {
    let graph = ServiceGraph::new([]).expect("nothing cannot contain a loop");

    assert!(graph.is_empty());
    assert_eq!(graph.len(), 0);
    assert!(graph.start_order().is_empty());
    assert!(graph.stop_order().is_empty());
    assert!(graph.spec(&id("caddy")).is_none());
}

/// The same specs in a different order must produce the same plan, or a boot order is a coin
/// toss and a dependency bug only reproduces on somebody else's machine.
#[test]
fn the_plan_does_not_depend_on_the_order_the_specs_arrived_in() {
    let forwards = graph(&[
        ("caddy", &["php-fpm"]),
        ("mailpit", &[]),
        ("mariadb", &[]),
        ("php-fpm", &["mariadb"]),
    ]);
    let backwards = graph(&[
        ("php-fpm", &["mariadb"]),
        ("mariadb", &[]),
        ("mailpit", &[]),
        ("caddy", &["php-fpm"]),
    ]);

    assert_eq!(forwards.start_order(), backwards.start_order());
    assert_eq!(forwards.stop_order(), backwards.stop_order());
}
