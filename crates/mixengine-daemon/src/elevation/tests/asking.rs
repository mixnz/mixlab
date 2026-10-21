//! What each producer asks the machine for, and when it asks for nothing.

use super::*;

/// D11: the machine already says what the database says it should, so nothing is queued. A row
/// here would put an operation on `mix status` whose only possible outcome is `AlreadyDone`.
#[tokio::test]
async fn a_machine_that_already_agrees_is_not_asked_for_permission() {
    let (_home, elevation, _events, _machine) = registry(mock::Host::with_hosts(
        "/tmp/mixengine",
        ["127.0.0.1 blog.test"],
    ))
    .await;
    a_site_named(&elevation.store, "blog.test").await;

    elevation.require_hosts().await.unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty(),
        "nothing to do is not something to ask about"
    );
}

/// And when it disagrees, exactly one operation is waiting and one event was published.
#[tokio::test]
async fn a_machine_that_disagrees_is_asked_once() {
    let (home, elevation, events, _machine) =
        registry(mock::Host::with_hosts("/tmp/mixengine", [])).await;
    with_an_installed_helper(&home);
    let mut watching = events.subscribe();
    a_site_named(&elevation.store, "blog.test").await;

    elevation.require_hosts().await.unwrap();

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();
    assert_eq!(waiting.len(), 1);
    assert!(waiting[0].description.contains("blog.test"), "{waiting:?}");

    assert!(matches!(
        watching.next().await,
        Some(crate::api::events::Frame::Event(
            DaemonEvent::ElevationRequired { .. }
        ))
    ));
}

/// **The seam T44 built, in both directions** — the T44 design, D4.
///
/// A home on the hosts file asks for the entry its sites declare. A home on DNS asks for an
/// *empty* block, which is not the same as asking for nothing: the server answers the whole
/// managed TLD by pattern, so an entry adds nothing, and the operation is what clears the names
/// the other mode wrote. Skipping the queue would leave them resolving to loopback for ever.
///
/// This is the test that stops D4 being tidied into "if DNS is on, do nothing".
#[tokio::test]
async fn a_home_on_dns_asks_for_an_empty_block_rather_than_for_nothing() {
    let (home, elevation, _events, _machine) = registry_resolving(
        mock::Host::with_hosts("/tmp/mixengine", ["127.0.0.1 blog.test"]),
        crate::dns::Dns::wired_for_tests(),
    )
    .await;
    with_an_installed_helper(&home);
    a_site_named(&elevation.store, "blog.test").await;

    elevation.require_hosts().await.unwrap();

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();

    assert_eq!(
        waiting.len(),
        1,
        "the block the machine holds is not the block a DNS home wants"
    );
    assert_eq!(
        waiting[0].op,
        PrivilegedOp::hosts_apply(
            Some(mixengine_platform::hosts::tag_for(home.path())),
            Vec::new()
        ),
        "a home on DNS wants no managed names in its own block"
    );
}

/// T211g: a row queued before each home kept its own block asks to write the shared one, which the
/// helper now refuses. Once this home's own block says what it should, that row is withdrawn.
#[tokio::test]
async fn a_pending_shared_block_from_before_t211g_is_withdrawn() {
    let (home, elevation, _events, _machine) = registry_resolving(
        mock::Host::with_hosts("/tmp/mixengine", ["127.0.0.1 blog.test"]),
        crate::dns::Dns::hosts_only_for_tests(),
    )
    .await;
    with_an_installed_helper(&home);
    a_site_named(&elevation.store, "blog.test").await;

    let shared = PrivilegedOp::hosts_apply(
        None,
        [mixengine_proto::privileged::HostEntry {
            address: "127.0.0.1".parse().unwrap(),
            domain: "blog.test".to_owned(),
        }],
    );
    elevation.enqueue(&shared).await.unwrap();

    elevation.require_hosts().await.unwrap();

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();
    assert!(waiting.is_empty(), "{waiting:?}");
}

/// The same home, resolving the way every machine does until T45: the entry is asked for.
#[tokio::test]
async fn a_home_on_the_hosts_file_asks_for_the_names_its_sites_declare() {
    let (home, elevation, _events, _machine) = registry_resolving(
        mock::Host::with_hosts("/tmp/mixengine", []),
        crate::dns::Dns::hosts_only_for_tests(),
    )
    .await;
    with_an_installed_helper(&home);
    a_site_named(&elevation.store, "blog.test").await;

    elevation.require_hosts().await.unwrap();

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();

    assert_eq!(waiting.len(), 1);
    assert!(waiting[0].description.contains("blog.test"), "{waiting:?}");
}

/// D2 asserted rather than described: two sites before anybody clicks Allow are one row holding
/// the *second* state, and one event per change rather than one row per change.
#[tokio::test]
async fn two_sites_before_a_grant_are_one_row_holding_the_second_state() {
    let (home, elevation, events, _machine) =
        registry(mock::Host::with_hosts("/tmp/mixengine", [])).await;
    with_an_installed_helper(&home);
    let mut watching = events.subscribe();

    a_site_named(&elevation.store, "blog.test").await;
    elevation.require_hosts().await.unwrap();

    a_site_named(&elevation.store, "shop.test").await;
    elevation.require_hosts().await.unwrap();

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert!(waiting[0].description.contains("blog.test"), "{waiting:?}");
    assert!(waiting[0].description.contains("shop.test"), "{waiting:?}");

    for expected in ["blog.test", "shop.test"] {
        let published = watching.next().await.expect("an event");
        let crate::api::events::Frame::Event(DaemonEvent::ElevationRequired { pending }) =
            published
        else {
            panic!("the wrong event: {published:?}")
        };
        assert!(pending[0].description.contains(expected), "{pending:?}");
    }

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), watching.next())
            .await
            .is_err(),
        "and no third event"
    );
}

/// A read that fails is not a reason to refuse a site. The helper is the authority on what is in
/// that file, and it will say so on the screen T64 built — a better place for "your hosts file
/// has two BEGIN markers" than a site creation's error.
#[tokio::test]
async fn a_hosts_file_that_cannot_be_read_is_still_asked_about() {
    let (home, elevation, _events, _machine) = registry(mock::Host::unable_to_read_the_hosts_file(
        "/tmp/mixengine",
        "two BEGIN markers",
    ))
    .await;
    with_an_installed_helper(&home);
    a_site_named(&elevation.store, "blog.test").await;

    elevation.require_hosts().await.unwrap();

    assert_eq!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .len(),
        1
    );
}

/// D7: a machine that needs a grant and has not got one leaves exactly one row in the queue and
/// announces it once.
#[tokio::test]
async fn a_front_end_on_a_machine_with_no_grant_asks_for_one() {
    let (home, elevation, events, _machine) = registry(mock::Host::without_port_access(
        "/tmp/mixengine",
        mixengine_platform::PortAccessMethod::Capability,
        "the binary holds no capability",
    ))
    .await;
    with_an_installed_helper(&home);
    let mut watching = events.subscribe();

    elevation
        .require_port_access(Some(Path::new("/packages/caddy/caddy")))
        .await
        .unwrap();

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].op.name(), "port-access-grant");
    assert!(matches!(
        watching.next().await,
        Some(crate::api::events::Frame::Event(
            DaemonEvent::ElevationRequired { .. }
        ))
    ));

    // A second start adds no second row: the dedupe key is the kind, and the state has not
    // changed — the T41 design, D2, which this operation reuses unchanged.
    elevation
        .require_port_access(Some(Path::new("/packages/caddy/caddy")))
        .await
        .unwrap();

    assert_eq!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .len(),
        1
    );
}

/// A machine that already allows it spends no prompt, which is the whole reason the disk is read
/// before the queue is written.
#[tokio::test]
async fn a_machine_that_already_allows_it_asks_for_nothing() {
    let (_home, elevation, _events, _machine) = registry(mock::Host::with_port_access(
        "/tmp/mixengine",
        mixengine_platform::PortAccessMethod::Capability,
    ))
    .await;

    elevation
        .require_port_access(Some(Path::new("/packages/caddy/caddy")))
        .await
        .unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty()
    );
}

/// A home with no front end has no binary to name, and on Linux the question cannot even be
/// asked without one — D12's reason for the producer being one-directional.
#[tokio::test]
async fn a_home_with_no_front_end_asks_for_nothing() {
    let (_home, elevation, _events, _machine) = registry(mock::Host::without_port_access(
        "/tmp/mixengine",
        mixengine_platform::PortAccessMethod::Capability,
        "the binary holds no capability",
    ))
    .await;

    elevation.require_port_access(None).await.unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty()
    );
}

/// T85's D6, the row that queues: a helper shipped beside the daemon and none installed.
///
/// The fixture's `Candidates::installed` is `None`, so this states the machine rather than
/// inheriting whichever one the tests are running on.
#[tokio::test]
async fn a_machine_with_no_installed_helper_is_asked_to_have_one() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    elevation
        .require_helper()
        .await
        .expect("the row is written");

    let pending = mixengine_core::elevation::pending(&elevation.store)
        .await
        .expect("the queue reads back");

    assert_eq!(
        pending
            .iter()
            .filter(|op| op.op == PrivilegedOp::HelperInstall {})
            .count(),
        1,
        "{pending:?}"
    );
}

/// And asking twice is still one row.
///
/// Worth its own test because this is the first operation whose dedupe key is a bare name with
/// no data behind it: `Probe`'s key is its serialisation, and every other operation's is derived
/// from a plan. A start that asked for this on every boot and got a row each time would put a
/// growing list on the one screen whose job is to say what a prompt will change.
#[tokio::test]
async fn asking_twice_for_the_helper_queues_one_operation() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    elevation.require_helper().await.expect("once");
    elevation.require_helper().await.expect("twice");

    let pending = mixengine_core::elevation::pending(&elevation.store)
        .await
        .expect("the queue reads back");

    assert_eq!(
        pending
            .iter()
            .filter(|op| op.op == PrivilegedOp::HelperInstall {})
            .count(),
        1,
        "{pending:?}"
    );
}

/// T88d. A machine whose helper an uninstall removed still ships a source, so the next thing
/// that needs root puts the installation in front of itself — without the daemon restart
/// `require_helper` alone would have needed.
#[tokio::test]
async fn a_producer_on_a_machine_with_no_installed_helper_asks_for_one() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_hosts("/tmp/mixengine", [])).await;
    a_site_named(&elevation.store, "blog.test").await;

    elevation.require_hosts().await.unwrap();

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();

    assert_eq!(
        waiting
            .iter()
            .filter(|row| row.op == PrivilegedOp::HelperInstall {})
            .count(),
        1,
        "{waiting:?}"
    );
}

/// **And a machine that already agrees is still asked for nothing.** The bootstrap sits at the
/// enqueue rather than at the top of the producer, so a site creation that needed no hosts
/// entry raises no prompt — which is the promise `docs/architecture/security-model.md` makes
/// about creating a site, and the reason this row is here beside the one above.
#[tokio::test]
async fn a_producer_that_asks_for_nothing_asks_for_no_helper_either() {
    let (_home, elevation, _events, _machine) = registry(mock::Host::with_hosts(
        "/tmp/mixengine",
        ["127.0.0.1 blog.test"],
    ))
    .await;
    a_site_named(&elevation.store, "blog.test").await;

    elevation.require_hosts().await.unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty(),
        "nothing to do is not something to ask about"
    );
}

/// Nothing installed **and** nothing to install from: a row that could never be applied is
/// worse than the refusal `elevation.grant` already gives, so none is written.
#[tokio::test]
async fn a_producer_with_no_helper_anywhere_asks_for_no_install() {
    let (home, elevation, _events, _machine) =
        registry(mock::Host::with_hosts("/tmp/mixengine", [])).await;
    a_site_named(&elevation.store, "blog.test").await;

    std::fs::remove_file(
        home.path()
            .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
    )
    .expect("the helper goes");

    elevation.require_hosts().await.unwrap();

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();

    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert!(
        !waiting
            .iter()
            .any(|row| row.op == PrivilegedOp::HelperInstall {}),
        "{waiting:?}"
    );
}

/// T88d, and the other half of the rule above: a helper installation already waiting is
/// dropped before an uninstall asks for the removal, so the batch does not install the file it
/// is about to remove.
///
/// **Here rather than in `crate::uninstall`'s own test module**, which has no fixture holding a
/// queue and a machine — this one does, and building a second would be two answers to what a
/// pending operation is.
#[tokio::test]
async fn an_uninstall_drops_a_helper_installation_that_is_already_waiting() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    elevation
        .require_helper()
        .await
        .expect("the installation is asked for");

    crate::uninstall::drop_helper_installations(&elevation)
        .await
        .expect("the row goes");

    elevation
        .enqueue(&PrivilegedOp::HelperRemove {})
        .await
        .expect("the removal is asked for");

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();

    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert_eq!(waiting[0].op, PrivilegedOp::HelperRemove {});
}

/// **The uninstall's own door is untouched by T88d's rule.** `crate::uninstall` enqueues
/// directly, so a batch that removes the helper never first installs one.
#[tokio::test]
async fn enqueueing_directly_asks_for_no_helper() {
    let (_home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    elevation
        .enqueue(&PrivilegedOp::HelperRemove {})
        .await
        .expect("the row is written");

    let waiting = mixengine_core::elevation::pending(&elevation.store)
        .await
        .unwrap();

    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert_eq!(waiting[0].op, PrivilegedOp::HelperRemove {});
}

/// D6's first row: a daemon with no helper beside it has nothing to install, so it asks for
/// nothing. Distinct from "it is already installed", and the reason it is a row of its own is
/// that this is the state a moved binary is in — and `elevation.grant` already tells a person
/// about *that* where they can act on it.
#[tokio::test]
async fn a_daemon_with_no_helper_beside_it_asks_for_no_install() {
    let (home, elevation, _events, _machine) =
        registry(mock::Host::with_home("/tmp/mixengine")).await;

    std::fs::remove_file(
        home.path()
            .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
    )
    .expect("the helper goes");

    elevation.require_helper().await.expect("nothing is asked");

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty()
    );
}

/// A probe that fails is not a reason to ask for a prompt, and not a reason to fail a start
/// either. It is logged, and the machine carries on — the opposite of `require_hosts`, and
/// deliberately: there the helper is the authority on the file's contents and can refuse with a
/// reason on the screen; here a probe that could not read one attribute tells us nothing about
/// what to ask for.
#[tokio::test]
async fn a_probe_that_fails_asks_for_nothing_and_does_not_fail() {
    let (_home, elevation, _events, _machine) = registry(mock::Host::unable_to_probe_port_access(
        "/tmp/mixengine",
        "this filesystem carries no extended attributes",
    ))
    .await;

    elevation
        .require_port_access(Some(Path::new("/packages/caddy/caddy")))
        .await
        .expect("a probe that failed is not an error to the caller");

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty()
    );
}

/// Windows: the method says nothing is needed, so nothing is asked for even though the front end
/// answers on 80. No `#[cfg]` anywhere above this crate is what makes that true.
#[tokio::test]
async fn a_machine_that_reserves_no_ports_asks_for_nothing() {
    let (_home, elevation, _events, _machine) = registry(mock::Host::with_port_access(
        "/tmp/mixengine",
        mixengine_platform::PortAccessMethod::Direct,
    ))
    .await;

    elevation
        .require_port_access(Some(Path::new("/packages/caddy/caddy.exe")))
        .await
        .unwrap();

    assert!(
        mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap()
            .is_empty()
    );
}

/// The probe after a grant is spent on the two operations that write the helper's file, and on
/// nothing else — least of all on an uninstall, whose last operation removes the audit log the
/// probe would put back on a machine whose token is already elevated.
#[test]
fn only_a_batch_that_installed_or_replaced_the_helper_is_worth_a_probe() {
    let batch = |ops: &[PrivilegedOp]| -> Vec<mixengine_proto::PendingOp> {
        ops.iter()
            .enumerate()
            .map(|(index, op)| mixengine_proto::PendingOp {
                id: mixengine_proto::PendingOpId(index as i64),
                description: op.describe(),
                op: op.clone(),
                requested_at: mixengine_proto::Timestamp(0),
                needs_the_screen: None,
            })
            .collect()
    };

    assert!(may_have_changed_the_helper(&batch(&[
        PrivilegedOp::hosts_apply(None, vec![]),
        PrivilegedOp::HelperInstall {},
    ])));
    assert!(may_have_changed_the_helper(&batch(&[
        PrivilegedOp::HelperReplace {}
    ])));

    assert!(!may_have_changed_the_helper(&batch(&[])));
    assert!(!may_have_changed_the_helper(&batch(&[
        PrivilegedOp::hosts_apply(None, vec![]),
    ])));
    assert!(!may_have_changed_the_helper(&batch(&[
        PrivilegedOp::HelperRemove {},
        PrivilegedOp::AuditLogRemove {},
    ])));
}
