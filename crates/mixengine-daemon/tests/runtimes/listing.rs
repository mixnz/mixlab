//! A list that cannot read one kind, and a runtime folder deleted under a home.

use super::*;

/// [`index`], plus a Node.js — two kinds, so that one of them can go wrong.
fn index_with_a_node(packed: &Packed, url: &str) -> Value {
    let mut published = index(packed, url);
    published["packages"]
        .as_array_mut()
        .expect("packages")
        .push(json!({
            "kind": "node",
            "version": "22.1.0",
            "channel": "stable",
            "artifacts": [{
                "os": os(),
                "arch": arch(),
                "url": "https://example.invalid/node-22.1.0.tar.zst",
                "sha256": "00",
                "size": 1,
                "provides": { "node": "bin/node" },
            }],
        }));
    published
}

/// **One kind's file that is not the one the root names costs that kind and no other** — roadmap
/// task **T196**, its design's D5. The list says which, and a question about that kind is answered
/// with "could not be read" rather than with "is not published".
#[tokio::test]
async fn a_kind_that_cannot_be_read_is_named_and_the_rest_are_listed() {
    let fixture = Fixture::start_with(index_with_a_node).await;
    let mut client = fixture.client().await;
    fixture.registry.corrupt_kind("node");

    let listed = client
        .call("runtime.list_available", json!({"refresh": true}))
        .await;

    let kinds: Vec<&str> = listed["runtimes"]
        .as_array()
        .expect("runtimes")
        .iter()
        .filter_map(|release| release["kind"].as_str())
        .collect();
    assert_eq!(kinds, ["php"], "the kind that was read is listed: {listed}");

    let gaps = listed["unavailable"]
        .as_array()
        .expect("a daemon that says what it could not read");
    assert_eq!(gaps.len(), 1, "{listed}");
    assert_eq!(gaps[0]["name"], "node");
    assert!(
        gaps[0]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("does not hash")),
        "the reason is the refusal's own sentence: {listed}"
    );

    let refused = client
        .refuse(
            "runtime.requirements",
            json!({"kind": "node", "version": "22.1.0"}),
        )
        .await;
    let message = refused["message"].as_str().expect("a message");
    assert!(
        message.contains("could not be read for node"),
        "it is unread, which is not the same as unpublished: {refused}"
    );
    assert!(!message.contains("does not publish"), "{refused}");
}

/// A listing that read everything says so with an empty list, which is not the same as a daemon
/// from before T196 that says nothing.
#[tokio::test]
async fn a_list_that_read_everything_has_nothing_unavailable() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;

    let listed = client.call("runtime.list_available", json!({})).await;

    assert_eq!(listed["unavailable"], json!([]), "{listed}");
}

/// **A restored default is still the default** — roadmap task **T206d**, D3.
#[tokio::test]
async fn a_deleted_runtime_folder_is_restored_as_the_default_it_was() {
    let fixture = Fixture::start().await;
    let mut client = fixture.client().await;
    assert_eq!(client.install(VERSION).await["state"], "succeeded");

    std::fs::remove_dir_all(fixture.installed_at(VERSION)).expect("deleted by hand");
    let listed = client.call("runtime.list_installed", json!({})).await;
    assert_eq!(listed["runtimes"][0]["missing"], true, "{listed}");

    let restored = client.install(VERSION).await;
    assert_eq!(restored["state"], "succeeded", "{restored}");
    let listed = client.call("runtime.list_installed", json!({})).await;
    assert_eq!(listed["runtimes"][0]["missing"], false, "{listed}");
    assert_eq!(listed["runtimes"][0]["default"], true, "{listed}");
}
