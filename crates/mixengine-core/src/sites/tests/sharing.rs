//! Sharing, a site created whole, and HTTPS redirects.

use super::*;

/// A site is created unshared, shares whole, and unshares back to nothing — roadmap task
/// **T74**.
#[tokio::test]
async fn sharing_is_written_and_taken_away_as_one_value() {
    let (_temp, store, project) = home().await;

    let created = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: true,
            https_redirect: false,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a site");

    assert!(created.sharing.is_none(), "a site is created unshared");

    let sharing = Sharing {
        interface: "Wi-Fi".to_owned(),
        address: std::net::Ipv4Addr::new(192, 168, 1, 10),
        since: Timestamp(1_700_000_000_000),
        until: None,
    };

    let shared = set_sharing(&store, created.id, Some(&sharing))
        .await
        .expect("a share");

    assert_eq!(shared.sharing.as_ref(), Some(&sharing));

    // Read back through the ordinary listing rather than the writer's own answer: what a
    // consumer sees is what is on disk.
    let listed = records(&store, None).await.expect("the sites");
    assert_eq!(listed[0].sharing.as_ref(), Some(&sharing));

    let unshared = set_sharing(&store, created.id, None)
        .await
        .expect("an unshare");

    assert!(unshared.sharing.is_none());
}

/// A deadline rides with the rest of the sharing row and leaves with it — roadmap task **T76**.
///
/// **`until` is the one of the four that is optional on its own.** A share without an expiry is
/// the ordinary case, which is why it sits outside the trigger's all-or-nothing rule rather than
/// inside it — and why an *unshared* site carrying one is still refused.
#[tokio::test]
async fn a_share_carries_a_deadline_and_loses_it_with_the_rest() {
    let (_temp, store, project) = home().await;

    let created = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: false,
            https_redirect: false,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a site");

    let sharing = Sharing {
        interface: "Wi-Fi".to_owned(),
        address: std::net::Ipv4Addr::new(192, 168, 1, 10),
        since: Timestamp(1_700_000_000_000),
        until: Some(Timestamp(1_700_000_007_200_000)),
    };

    let shared = set_sharing(&store, created.id, Some(&sharing))
        .await
        .expect("a share");
    assert_eq!(shared.sharing.as_ref(), Some(&sharing));

    // Read back through the ordinary listing: what a consumer sees is what is on disk.
    let listed = records(&store, None).await.expect("the sites");
    assert_eq!(
        listed[0].sharing.as_ref().and_then(|one| one.until),
        Some(Timestamp(1_700_000_007_200_000))
    );

    // A share with no deadline is the ordinary one, and not a half-written row.
    let forever = Sharing {
        until: None,
        ..sharing.clone()
    };
    let shared = set_sharing(&store, created.id, Some(&forever))
        .await
        .expect("a share with no deadline");
    assert_eq!(shared.sharing.as_ref(), Some(&forever));

    let unshared = set_sharing(&store, created.id, None)
        .await
        .expect("an unshare");
    assert!(unshared.sharing.is_none());
}

/// The trigger in `0001_initial.sql`, from the side that would break it: two of the three
/// columns set is a state no reader could make sense of, so the database refuses it outright.
#[tokio::test]
async fn a_half_written_share_is_refused_by_the_database() {
    let (_temp, store, project) = home().await;

    let created = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: true,
            https_redirect: false,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a site");

    let refused = sqlx::query!(
        "UPDATE sites SET shared_interface = 'Wi-Fi' WHERE id = ?",
        created.id
    )
    .execute(store.pool())
    .await;

    assert!(refused.is_err(), "the database allowed half a share");
}

/// The URL is what somebody types into a phone, so 80 is left off and anything else is not.
#[test]
fn a_shared_url_carries_the_port_only_when_it_is_not_eighty() {
    let address = std::net::Ipv4Addr::new(192, 168, 1, 10);

    assert_eq!(shared_url(address, 80), "http://192.168.1.10");
    assert_eq!(shared_url(address, 8080), "http://192.168.1.10:8080");
}

/// A site, its ordered domains and its links are one write.    /// A site, its ordered domains and its links are one write.
#[tokio::test]
async fn a_site_is_created_whole_and_read_back_whole() {
    let (_temp, store, project) = home().await;

    let created = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: "public".to_owned(),
            kind: php(),
            https_enabled: true,
            https_redirect: false,
            domains: vec!["blog.test".to_owned(), "www.blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a site");

    assert_eq!(created.domains, ["blog.test", "www.blog.test"]);
    assert_eq!(created.state, SiteState::Enabled);

    let found = by_domain(&store, "www.blog.test")
        .await
        .expect("a read")
        .expect("an alias reaches the site as surely as the primary does");
    assert_eq!(found.id, created.id);
    assert_eq!(found.domains[0], "blog.test", "the head is the primary");
}

/// **T98.** `https_redirect: true` is refused at `create` when `https_enabled` is `false`,
/// before a row is written — the schema's own `CHECK` would refuse it a step later, in words
/// about the column rather than the request.
#[tokio::test]
async fn https_redirect_needs_https_enabled_to_create_a_site() {
    let (_temp, store, project) = home().await;

    let refusal = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: false,
            https_redirect: true,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect_err("a redirect with nothing to redirect to");

    assert!(
        matches!(
            refusal,
            Error::Project(ProjectError::HttpsRedirectNeedsHttps)
        ),
        "{refusal}"
    );
    assert!(
        by_domain(&store, "blog.test")
            .await
            .expect("a read")
            .is_none(),
        "the refusal left no row behind"
    );
}

/// **T98, D2.** `https_redirect: Some(true)` is refused whether the `false` it collides with
/// comes from this same `update` turning HTTPS off, or from the site already being
/// plaintext-only — the two cases the design calls out by name.
#[tokio::test]
async fn https_redirect_needs_https_enabled_to_update_a_site_either_way_it_arrives() {
    let (_temp, store, project) = home().await;

    let plaintext = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: false,
            https_redirect: false,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a plaintext-only site");

    let already_off = update(
        &store,
        plaintext.id,
        &Change {
            https_redirect: Some(true),
            ..Change::default()
        },
    )
    .await
    .expect_err("the site was already plaintext-only");
    assert!(
        matches!(
            already_off,
            Error::Project(ProjectError::HttpsRedirectNeedsHttps)
        ),
        "{already_off}"
    );

    let https = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: true,
            https_redirect: false,
            domains: vec!["shop.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("an https site");

    let turned_off_in_the_same_call = update(
        &store,
        https.id,
        &Change {
            https_enabled: Some(false),
            https_redirect: Some(true),
            ..Change::default()
        },
    )
    .await
    .expect_err("this update turns https off and asks for a redirect in the same breath");
    assert!(
        matches!(
            turned_off_in_the_same_call,
            Error::Project(ProjectError::HttpsRedirectNeedsHttps)
        ),
        "{turned_off_in_the_same_call}"
    );
}

/// **T98, D2.** Turning `https_enabled` off while saying nothing about the redirect carries the
/// redirect to `false` as well, because leaving it `true` on a plaintext-only site is a row the
/// schema's `CHECK` refuses to hold — and the caller asked for neither that refusal nor a
/// silent one either.
#[tokio::test]
async fn turning_https_off_silently_carries_an_untouched_redirect_to_false() {
    let (_temp, store, project) = home().await;

    let site = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: true,
            https_redirect: true,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("an https site with its redirect on");
    assert!(site.https_redirect, "the fixture's own precondition");

    let changed = update(
        &store,
        site.id,
        &Change {
            https_enabled: Some(false),
            ..Change::default()
        },
    )
    .await
    .expect("https turns off and the redirect is carried with it, not refused");

    assert!(!changed.https_enabled);
    assert!(
        !changed.https_redirect,
        "a stale redirect on a plaintext-only site is not \"leave it\""
    );
}

/// **T98.** Turning HTTPS on leaves an untouched redirect exactly where it was — the ordinary
/// "leave it" reading, asserted so the cascade above cannot be mistaken for a rule about every
/// `https_enabled` change rather than only the one that turns it off.
#[tokio::test]
async fn turning_https_on_leaves_an_untouched_redirect_alone() {
    let (_temp, store, project) = home().await;

    let site = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: false,
            https_redirect: false,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("a plaintext-only site");

    let changed = update(
        &store,
        site.id,
        &Change {
            https_enabled: Some(true),
            ..Change::default()
        },
    )
    .await
    .expect("turning https on");

    assert!(changed.https_enabled);
    assert!(!changed.https_redirect, "nothing asked for a redirect");
}

/// **T98.** Both columns round-trip through `create`, `update` and `records` together, the way
/// `https_enabled` alone already does.
#[tokio::test]
async fn https_redirect_round_trips_through_create_update_and_records() {
    let (_temp, store, project) = home().await;

    let created = create(
        &store,
        &NewSite {
            owner: SiteOwner::Project(project),
            doc_root: String::new(),
            kind: SiteKind::Static,
            https_enabled: true,
            https_redirect: true,
            domains: vec!["blog.test".to_owned()],
            services: Vec::new(),
            routes: Vec::new(),
        },
    )
    .await
    .expect("an https site with its redirect on");
    assert!(created.https_redirect);

    let found = by_domain(&store, "blog.test")
        .await
        .expect("a read")
        .expect("the site");
    assert!(found.https_redirect, "created is not the only writer");

    let changed = update(
        &store,
        created.id,
        &Change {
            https_redirect: Some(false),
            ..Change::default()
        },
    )
    .await
    .expect("turning the redirect off alone");
    assert!(changed.https_enabled, "untouched");
    assert!(!changed.https_redirect);
}
