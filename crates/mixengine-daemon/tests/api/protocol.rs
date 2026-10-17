//! The API as HTTP and JSON-RPC: health, batches, errors, routes and the event stream.

use super::*;

#[tokio::test]
async fn health_is_answerable_and_says_which_protocol_this_daemon_speaks() {
    let daemon = Daemon::start().await;
    let response = daemon.get("/health").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).map(|v| v.as_bytes()),
        Some(&b"application/json"[..])
    );

    let health = json(response).await;
    assert_eq!(health["ok"], true);
    assert_eq!(health["protocol"], 1);
    assert_eq!(health["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn health_answers_a_head_with_the_headers_and_no_body() {
    let daemon = Daemon::start().await;
    let response = daemon.head("/health").await;

    // What a liveness probe reaches for, and what HTTP expects of anything that answers `GET`.
    // hyper writes the headers and drops the body, so this also pins that we are not relying on a
    // handler to remember it.
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).map(|v| v.as_bytes()),
        Some(&b"application/json"[..])
    );
    assert!(
        response
            .into_body()
            .collect()
            .await
            .expect("the empty body arrives")
            .to_bytes()
            .is_empty()
    );
}

#[tokio::test]
async fn a_request_that_spelled_its_id_null_is_answered_rather_than_ignored() {
    let daemon = Daemon::start().await;

    // A notification is a request with no `id` *member*. `"id":null` has one, the spec nowhere lets
    // it mean silence, and a client that sent it is waiting — so it is answered, to the id it gave.
    let answer = daemon
        .rpc(r#"{"jsonrpc":"2.0","method":"daemon.version","id":null}"#)
        .await;

    assert_eq!(answer["result"]["protocol"], 1);
    assert!(answer["id"].is_null(), "{answer}");
}

#[tokio::test]
async fn status_describes_the_home_the_daemon_was_actually_started_with() {
    let daemon = Daemon::start().await;
    let answer = daemon
        .rpc(r#"{"jsonrpc":"2.0","method":"daemon.status","id":1}"#)
        .await;

    let status = &answer["result"];
    assert_eq!(answer["id"], 1);
    assert_eq!(status["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(status["protocol"], 1);

    // The point of the field: somebody looking at a daemon they did not expect to be talking to.
    // Compared through `Path`, because the daemon prints the home it resolved and a temporary
    // directory on macOS arrives as `/var/…` where `/private/var/…` was asked for.
    let reported = Path::new(status["home"].as_str().expect("home is a string"));
    assert!(
        reported.ends_with(
            daemon
                .home()
                .file_name()
                .expect("the temporary home has a name")
        ),
        "{reported:?} is not the home the daemon was started with ({:?})",
        daemon.home()
    );

    assert_eq!(status["endpoint"], daemon.endpoint.to_string());
    assert!(
        status["pid"].as_u64().is_some_and(|pid| pid > 0),
        "{status}"
    );
    assert!(
        status["database"]
            .as_str()
            .is_some_and(|path| path.ends_with("mixengine.db")),
        "{status}"
    );
    assert!(status["started_at"].as_i64().is_some(), "{status}");
    assert!(status["uptime"].as_u64().is_some(), "{status}");
}

#[tokio::test]
async fn a_batch_comes_back_as_an_array_and_a_notification_is_left_out_of_it() {
    let daemon = Daemon::start().await;
    let answers = daemon
        .rpc(
            r#"[{"jsonrpc":"2.0","method":"daemon.version","id":1},
                {"jsonrpc":"2.0","method":"daemon.status"},
                {"jsonrpc":"2.0","method":"nope.nope","id":3}]"#,
        )
        .await;

    let answers = answers.as_array().expect("a batch is answered by an array");
    assert_eq!(answers.len(), 2);
    assert_eq!(answers[0]["result"]["protocol"], 1);
    assert_eq!(answers[1]["error"]["code"], -32601);
}

#[tokio::test]
async fn a_body_of_nothing_but_notifications_is_answered_with_no_content() {
    let daemon = Daemon::start().await;
    let response = daemon
        .send(build(
            Method::POST,
            "/rpc",
            Bytes::from_static(br#"{"jsonrpc":"2.0","method":"daemon.status"}"#),
        ))
        .await;

    // Not an empty `200`: a client that parses every response would be handed zero bytes where it
    // expects JSON.
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        response
            .into_body()
            .collect()
            .await
            .expect("an empty body still arrives")
            .to_bytes()
            .is_empty()
    );
}

#[tokio::test]
async fn a_failing_method_is_still_an_http_200_because_the_request_did_arrive() {
    let daemon = Daemon::start().await;

    // The rule the whole HTTP layer is built on: the status describes the envelope, the JSON-RPC
    // error describes the call. A 4xx here would make `not_found` on a site indistinguishable from
    // `/rpc` having been mistyped.
    // A namespace this build has not reached — `site.create` stood here until T39a made it a real
    // method, `blueprint.apply` until T77 did and `extension.install` until T81 did, which is
    // exactly the drift this test is worth keeping past.
    let answer = daemon
        .rpc(r#"{"jsonrpc":"2.0","method":"extension.open","id":1}"#)
        .await;

    assert_eq!(answer["error"]["code"], -32601);
    assert_eq!(answer["error"]["data"]["code"], "not_found");
}

/// **T77's D12, one task on.** One method, two answers, and a client reads which it got from the
/// object rather than from its own request — which is what lets `--dry-run` and a real apply stay
/// one method now that the second one does something.
///
/// A blueprint nothing is filed under is still a `not_found`, and that is the point of asking for
/// one here: what this task changed is the shape of a *successful* answer, not what a miss is.
#[tokio::test]
async fn a_dry_run_and_a_real_apply_are_one_method_that_says_which_answer_it_gave() {
    let daemon = Daemon::start().await;

    // Built rather than written out, because `/tmp/shop` is not an absolute path on Windows and the
    // root is checked before the blueprint is looked up — which would make this a test of the wrong
    // refusal on one of the three systems this has to pass on.
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "blueprint.apply",
        "id": 1,
        "params": {
            "blueprint": "nothing-here",
            "project": "shop",
            "root": std::env::temp_dir().join("shop").display().to_string(),
            "dry_run": true,
        }
    })
    .to_string();

    let answer = daemon.rpc(&body).await;

    assert_eq!(answer["error"]["data"]["code"], "not_found", "{answer}");
    assert!(
        answer["error"]["data"]["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("blueprint list")),
        "{answer}"
    );
}

/// **A fresh home holds the gallery and nothing else** — roadmap task **T79**. This said *no
/// blueprints* until the gallery existed; what makes the new claim the stronger one is that it is
/// about the set rather than about emptiness.
#[tokio::test]
async fn a_fresh_home_holds_the_gallery() {
    let daemon = Daemon::start().await;

    let answer = daemon
        .rpc(r#"{"jsonrpc":"2.0","method":"blueprint.list","id":1}"#)
        .await;

    let listed = answer["result"]["blueprints"]
        .as_array()
        .unwrap_or_else(|| panic!("a listing: {answer}"));

    let slugs: Vec<_> = listed
        .iter()
        .map(|one| one["slug"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        slugs,
        [
            "cakephp",
            "codeigniter",
            "craft",
            "django",
            "drupal",
            "express-mongodb",
            "laravel",
            "laravel-mongodb",
            "nextjs",
            "php-mysql",
            "rails",
            "statamic",
            "static",
            "strapi",
            "symfony",
            "vite",
            "wordpress",
            "yii"
        ],
        "{answer}"
    );

    for one in listed {
        assert_eq!(one["source"], "builtin", "{answer}");
        assert_eq!(one["trusted"], true, "{answer}");
    }
}

#[tokio::test]
async fn an_endpoint_that_does_not_exist_is_a_404_in_the_shape_every_client_renders() {
    let daemon = Daemon::start().await;
    // A path this daemon has no route for at all — the body is the plain error shape rather than a
    // JSON-RPC response, because there is no call to answer. `/metrics` stood here until T71 built
    // it, which is the same drift the unknown-method case above is kept past.
    let response = daemon.get("/blueprints").await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let error = json(response).await;
    assert_eq!(error["code"], "not_found");
    assert!(
        error["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("/rpc")),
        "{error}"
    );
}

/// `GET /logs/service/{id}` is a route that exists, so a name nothing declares is a `404` about the
/// *service* — not one about the endpoint. The difference is what a person reads when they mistype
/// a service id, and it is the reason this route is matched before the table of whole paths.
///
/// The path has two segments since roadmap task **T78a**, which gave a job the second kind of
/// subject: nothing has to decide whether a first segment is a package name or the word `job`.
#[tokio::test]
async fn asking_for_the_output_of_a_service_nothing_declares_names_the_service() {
    let daemon = Daemon::start().await;
    let response = daemon.get("/logs/service/mariadb").await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let error = json(response).await;
    assert_eq!(error["code"], "not_found");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("mariadb")),
        "{error}"
    );
}

/// A service id this daemon would refuse anywhere else is refused here too, and before anything is
/// looked up: the envelope is what is wrong, so it is a `400` rather than a `404`.
#[tokio::test]
async fn asking_for_the_output_of_something_that_is_not_a_service_id_is_refused() {
    let daemon = Daemon::start().await;
    let response = daemon.get("/logs/service/Not%20A%20Service").await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json(response).await["code"], "invalid_argument");
}

#[tokio::test]
async fn the_wrong_verb_on_a_real_route_says_which_one_would_have_worked() {
    let daemon = Daemon::start().await;
    let response = daemon.get("/rpc").await;

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        response.headers().get(ALLOW).map(|v| v.as_bytes()),
        Some(&b"POST"[..])
    );
}

#[tokio::test]
async fn a_body_larger_than_the_limit_is_refused_rather_than_read() {
    let daemon = Daemon::start().await;

    // Valid JSON, and two megabytes of it: the point is that the daemon stops reading rather than
    // that the content is bad.
    let oversized = format!(
        r#"{{"jsonrpc":"2.0","method":"{}","id":1}}"#,
        "x".repeat(2 * 1024 * 1024)
    );
    let (status, body) = daemon.send_unread(Bytes::from(oversized)).await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["code"], "invalid_argument");
}

#[tokio::test]
async fn the_event_stream_opens_and_is_held_open_rather_than_closed_at_once() {
    let daemon = Daemon::start().await;
    let response = daemon.get("/events").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).map(|v| v.as_bytes()),
        Some(&b"text/event-stream"[..])
    );

    // Nothing in this build publishes an event yet — the first producer arrives with service state
    // in Phase 1 — so what is provable here is the property that would otherwise be missed: an idle
    // stream stays open instead of ending, which is what makes it a stream rather than an empty
    // response. The frames themselves are pinned by the unit tests in `api::events`.
    let mut body = response.into_body();
    let idle = tokio::time::timeout(Duration::from_millis(500), body.frame()).await;

    assert!(
        idle.is_err(),
        "an idle event stream should stay open, not end: {idle:?}"
    );
}
