//! The engine against a real `/v1`: the one test where the client meets a server it did not write.
//!
//! **`#[ignore]`**, like every test here that needs something CI does not have. Run it against
//! `server/native/` in test-outbox mode:
//!
//! ```text
//! cd server/native && MIXLAB_SYNC_TEST_OUTBOX=1 MIXLAB_SYNC_BIND=127.0.0.1:8766 \
//!   MIXLAB_SYNC_DATABASE=/tmp/live.db MIXLAB_SYNC_REGISTRATIONS_PER_HOUR=100000 \
//!   MIXLAB_SYNC_AUTH_PER_HOUR=100000 cargo run
//! cd apps/desktop/src-tauri && MIXLAB_SYNC_TEST_SERVER=http://127.0.0.1:8766 \
//!   cargo test --locked --test sync_live -- --ignored
//! ```
//!
//! A move needs a second server with a database of its own: the same command with
//! `MIXLAB_SYNC_BIND=127.0.0.1:8767` and another `MIXLAB_SYNC_DATABASE`, and
//! `MIXLAB_SYNC_TEST_SERVER_2=http://127.0.0.1:8767` beside `MIXLAB_SYNC_TEST_SERVER` on the test
//! line.

use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde_json::{json, Value};
use tauri_app_lib::sync::account::{Account, Argon, Registration};
use tauri_app_lib::sync::crypto::{self, RecordAddress, Sealed};
use tauri_app_lib::sync::engine::{self, Change, Outgoing};
use tauri_app_lib::sync::lend::Item;
use tauri_app_lib::sync::saved::InMemory;
use tauri_app_lib::sync::session::SyncState;
use tauri_app_lib::sync::store::Store;
use tauri_app_lib::sync::transport::Transport;
use tauri_app_lib::sync::wire::WireRecord;

fn server() -> String {
    std::env::var("MIXLAB_SYNC_TEST_SERVER")
        .expect("MIXLAB_SYNC_TEST_SERVER names a server running in test-outbox mode")
}

fn random(bytes: usize) -> String {
    STANDARD.encode((0..bytes).map(|_| rand::random::<u8>()).collect::<Vec<_>>())
}

fn second_server() -> String {
    std::env::var("MIXLAB_SYNC_TEST_SERVER_2")
        .expect("MIXLAB_SYNC_TEST_SERVER_2 names a second server, with its own database")
}

/// The code in the newest letter of `kind` to `email`, from the outbox of the server at `base`.
async fn letter_on(base: &str, email: &str, kind: &str) -> String {
    let body = reqwest::get(format!(
        "{base}/__test__/outbox?email={}",
        email.replace('@', "%40")
    ))
    .await
    .expect("the server answers")
    .text()
    .await
    .expect("a body");
    let outbox: Value = serde_json::from_str(&body).expect("json");
    outbox["messages"]
        .as_array()
        .and_then(|messages| {
            messages
                .iter()
                .rev()
                .find(|message| message["kind"] == kind)
        })
        .and_then(|message| message["token"].as_str())
        .expect("a letter")
        .to_owned()
}

/// The same, from the first server.
async fn letter(email: &str, kind: &str) -> String {
    letter_on(&server(), email, kind).await
}

fn registration(email: &str, a: String) -> Registration {
    Registration {
        email: email.to_owned(),
        a,
        salt_account: random(16),
        argon: Argon::ours(),
        wrapped_mk_password: random(72),
        wrapped_mk_recovery: random(72),
    }
}

/// One account, verified, and the `A` that signs in to it.
async fn account() -> (String, [u8; 32]) {
    let email = format!("desktop-{}@example.invalid", uuid::Uuid::new_v4().simple());
    let a: [u8; 32] = rand::random();
    let server = Account::new(&server(), None).unwrap();
    server
        .register(&registration(&email, STANDARD.encode(a)))
        .await
        .unwrap();
    server
        .verify(&email, &letter(&email, "verification").await)
        .await
        .unwrap();
    (email, a)
}

/// A machine signed in to that account: its transport, its own store, and its device id.
async fn machine(email: &str, a: &[u8; 32], name: &str) -> (Transport, Store, String) {
    let signed_in = Account::new(&server(), None)
        .unwrap()
        .login(email, a, name)
        .await
        .unwrap();
    (
        Transport::new(&server(), &signed_in.access_token, None).unwrap(),
        Store::in_memory(&server()).await.unwrap(),
        signed_in.device_id,
    )
}

fn seal(
    data_key: &[u8; 32],
    collection: &str,
    id: &str,
    plaintext: &[u8],
    updated_at: i64,
) -> Outgoing {
    let sealed = crypto::seal_record(
        data_key,
        &RecordAddress {
            collection,
            id,
            deleted: false,
        },
        plaintext,
    )
    .unwrap();
    Outgoing {
        collection: collection.into(),
        id: id.into(),
        updated_at,
        change: Change::Write {
            nonce: STANDARD.encode(sealed.nonce),
            ciphertext: STANDARD.encode(sealed.ciphertext),
        },
    }
}

fn open(data_key: &[u8; 32], record: &WireRecord) -> Vec<u8> {
    let nonce: [u8; 24] = STANDARD
        .decode(record.nonce.as_ref().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let ciphertext = STANDARD
        .decode(record.ciphertext.as_ref().unwrap())
        .unwrap();
    crypto::open_record(
        data_key,
        &RecordAddress {
            collection: &record.collection,
            id: &record.id,
            deleted: false,
        },
        &Sealed { nonce, ciphertext },
    )
    .unwrap()
}

/// Every page of `collection`, each recorded before the next is read — what the shell does, with
/// the module's write left out.
async fn pull_all(transport: &Transport, store: &Store, collection: &str) -> Vec<WireRecord> {
    let mut all = Vec::new();
    loop {
        let fetched = engine::fetch(transport, store, collection, env!("CARGO_PKG_VERSION"))
            .await
            .unwrap();
        for record in &fetched.records {
            store.remember(record).await.unwrap();
        }
        engine::commit(store, collection, &fetched, &[])
            .await
            .unwrap();
        all.extend(fetched.records.iter().cloned());
        if !fetched.more {
            return all;
        }
    }
}

#[tokio::test]
#[ignore = "needs a sync server in test-outbox mode; see the module comment"]
async fn a_second_machine_reads_what_the_first_wrote() {
    let master = crypto::new_master_key();
    let (id_key, data_key) = (crypto::id_key(&master), crypto::data_key(&master));
    let collection = crypto::opaque_id(&id_key, "saved-queries");
    let id = crypto::opaque_id(&id_key, "a-local-uuid");

    let (email, a) = account().await;
    let (desktop, desktop_store, desktop_device) = machine(&email, &a, "desktop").await;
    let (laptop, laptop_store, _) = machine(&email, &a, "laptop").await;
    let limits = desktop.capabilities().await.unwrap();

    let pushed = engine::push(
        &desktop,
        &desktop_store,
        &limits,
        &desktop_device,
        vec![seal(&data_key, &collection, &id, b"select 1", 100)],
    )
    .await
    .unwrap();
    assert_eq!(pushed.accepted, 1);

    let arrived = pull_all(&laptop, &laptop_store, &collection).await;

    assert_eq!(arrived.len(), 1);
    assert_eq!(arrived[0].device, desktop_device);
    assert_eq!(open(&data_key, &arrived[0]), b"select 1");
}

#[tokio::test]
#[ignore = "needs a sync server in test-outbox mode; see the module comment"]
async fn two_machines_settle_a_conflict_the_same_way() {
    let master = crypto::new_master_key();
    let (id_key, data_key) = (crypto::id_key(&master), crypto::data_key(&master));
    let collection = crypto::opaque_id(&id_key, "saved-queries");
    let id = crypto::opaque_id(&id_key, "a-local-uuid");

    let (email, a) = account().await;
    let (desktop, desktop_store, desktop_device) = machine(&email, &a, "desktop").await;
    let (laptop, laptop_store, laptop_device) = machine(&email, &a, "laptop").await;
    let limits = desktop.capabilities().await.unwrap();

    engine::push(
        &desktop,
        &desktop_store,
        &limits,
        &desktop_device,
        vec![seal(&data_key, &collection, &id, b"first", 100)],
    )
    .await
    .unwrap();
    pull_all(&laptop, &laptop_store, &collection).await;

    // Both edit the version they both saw; the laptop's edit is later.
    engine::push(
        &desktop,
        &desktop_store,
        &limits,
        &desktop_device,
        vec![seal(&data_key, &collection, &id, b"desktop's", 200)],
    )
    .await
    .unwrap();
    let pushed = engine::push(
        &laptop,
        &laptop_store,
        &limits,
        &laptop_device,
        vec![seal(&data_key, &collection, &id, b"laptop's", 300)],
    )
    .await
    .unwrap();
    assert_eq!(
        pushed.accepted, 1,
        "the later write wins and goes round again"
    );

    let last = pull_all(&desktop, &desktop_store, &collection)
        .await
        .pop()
        .expect("the desktop is told");
    assert_eq!(last.device, laptop_device);
    assert_eq!(open(&data_key, &last), b"laptop's");
}

/// T189: the machine that wrote is told nothing is new and its cursor steps over its own write;
/// the other machine is told to pull.
#[tokio::test]
#[ignore = "needs a sync server in test-outbox mode; see the module comment"]
async fn heads_name_what_another_machine_wrote_and_step_over_an_echo() {
    let master = crypto::new_master_key();
    let (id_key, data_key) = (crypto::id_key(&master), crypto::data_key(&master));
    let collection = crypto::opaque_id(&id_key, "saved-queries");
    let version = env!("CARGO_PKG_VERSION");

    let (email, a) = account().await;
    let (desktop, desktop_store, desktop_device) = machine(&email, &a, "desktop").await;
    let (laptop, laptop_store, _) = machine(&email, &a, "laptop").await;
    let limits = desktop.capabilities().await.unwrap();

    // Both machines have read the account once, so neither cursor is 0.
    let first = crypto::opaque_id(&id_key, "first");
    engine::push(
        &desktop,
        &desktop_store,
        &limits,
        &desktop_device,
        vec![seal(&data_key, &collection, &first, b"select 1", 100)],
    )
    .await
    .unwrap();
    pull_all(&desktop, &desktop_store, &collection).await;
    pull_all(&laptop, &laptop_store, &collection).await;

    let second = crypto::opaque_id(&id_key, "second");
    engine::push(
        &desktop,
        &desktop_store,
        &limits,
        &desktop_device,
        vec![seal(&data_key, &collection, &second, b"select 2", 200)],
    )
    .await
    .unwrap();

    let told_desktop = engine::stale(
        &desktop,
        &desktop_store,
        &limits,
        std::slice::from_ref(&collection),
        version,
    )
    .await
    .unwrap();
    let told_laptop = engine::stale(
        &laptop,
        &laptop_store,
        &limits,
        std::slice::from_ref(&collection),
        version,
    )
    .await
    .unwrap();
    assert!(told_desktop.is_empty());
    assert_eq!(told_laptop, vec![collection.clone()]);

    let echo = engine::fetch(&desktop, &desktop_store, &collection, version)
        .await
        .unwrap();
    assert!(
        echo.records.is_empty(),
        "the desktop's cursor stepped over its own write"
    );
}

/// T189, the shell's call: plain names in, plain names out, one request for the lot.
#[tokio::test]
#[ignore = "needs a sync server in test-outbox mode; see the module comment"]
async fn the_client_asks_which_collections_to_pull_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let (desktop, email) = signed_up(dir.path(), "desktop", "correct horse").await;
    let laptop = machine_state(dir.path(), "laptop");
    laptop
        .login(&server(), None, &email, "correct horse".into(), "laptop")
        .await
        .unwrap();
    let names = vec!["query-snippets".to_string(), "rest-requests".to_string()];

    // Never pulled: stale without asking.
    assert_eq!(desktop.heads(names.clone()).await.unwrap(), names);

    // An account nobody has written to ends every pull at 0, which is a cursor that has seen
    // nothing: something is written first, so both machines' cursors are past it.
    let first = Item {
        id: "a-snippet".into(),
        data: json!({ "sql": "select 1" }),
    };
    pull_everything(&desktop, "query-snippets").await;
    desktop
        .push("query-snippets", vec![first.clone()])
        .await
        .unwrap();
    for name in &names {
        pull_everything(&desktop, name).await;
        pull_everything(&laptop, name).await;
    }
    let second = Item {
        id: "another-snippet".into(),
        data: json!({ "sql": "select 2" }),
    };
    desktop
        .push("query-snippets", vec![first, second])
        .await
        .unwrap();

    assert_eq!(
        desktop.heads(names.clone()).await.unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(
        laptop.heads(names).await.unwrap(),
        vec!["query-snippets".to_string()]
    );
}

#[tokio::test]
#[ignore = "needs a sync server in test-outbox mode; see the module comment"]
async fn a_wrong_code_is_a_wrong_code() {
    let email = format!("desktop-{}@example.invalid", uuid::Uuid::new_v4().simple());
    let server = Account::new(&server(), None).unwrap();
    server
        .register(&registration(&email, random(32)))
        .await
        .unwrap();
    let error = server.verify(&email, "AAAA-AAAA").await.unwrap_err();
    assert_eq!(error.code, "error.syncWrongCode");
}

#[tokio::test]
#[ignore = "needs a sync server in test-outbox mode; see the module comment"]
async fn a_refresh_rotates_and_spends_the_token_it_used() {
    let (email, a) = account().await;
    let server = Account::new(&server(), None).unwrap();
    let signed_in = server.login(&email, &a, "desktop").await.unwrap();
    let refreshed = server.refresh(&signed_in.refresh_token).await.unwrap();
    assert_ne!(refreshed.refresh_token, signed_in.refresh_token);
    let again = server.refresh(&signed_in.refresh_token).await;
    assert_eq!(
        again.err().map(|error| error.code),
        Some("error.syncSignedOut")
    );
}

#[tokio::test]
#[ignore = "needs a sync server in test-outbox mode; see the module comment"]
async fn a_device_list_names_this_machine_and_a_revoke_ends_it() {
    let (email, a) = account().await;
    let server = Account::new(&server(), None).unwrap();
    let desktop = server.login(&email, &a, "desktop").await.unwrap();
    let laptop = server.login(&email, &a, "laptop").await.unwrap();

    let devices = server.devices(&desktop.access_token).await.unwrap();
    assert_eq!(devices.len(), 2);
    assert!(devices
        .iter()
        .any(|device| device.current && device.name == "desktop"));

    server
        .revoke(&desktop.access_token, &laptop.device_id)
        .await
        .unwrap();
    let cut = server.devices(&laptop.access_token).await;
    assert_eq!(
        cut.err().map(|error| error.code),
        Some("error.syncSignedOut")
    );
}

fn machine_state(dir: &std::path::Path, name: &str) -> SyncState {
    SyncState::new(
        Arc::new(InMemory::default()),
        Ok(dir.join(format!("{name}.db"))),
    )
}

/// A machine that registered and confirmed, the address it used, and its recovery key.
async fn signed_up_with_key(
    dir: &std::path::Path,
    name: &str,
    password: &str,
) -> (SyncState, String, String) {
    let email = format!("desktop-{}@example.invalid", uuid::Uuid::new_v4().simple());
    let state = machine_state(dir, name);
    let recovery = state
        .register(&server(), None, &email, password.into())
        .await
        .unwrap();
    state
        .verify(&letter(&email, "verification").await, name)
        .await
        .unwrap();
    (state, email, recovery)
}

async fn signed_up(dir: &std::path::Path, name: &str, password: &str) -> (SyncState, String) {
    let (state, email, _) = signed_up_with_key(dir, name, password).await;
    (state, email)
}

/// Every page of `collection`, each committed as the shell would once its module has written it:
/// what a machine does before its first push in an account (`SyncState::push`).
async fn pull_everything(state: &SyncState, collection: &str) {
    loop {
        let page = state.pull_page(collection).await.unwrap();
        state
            .commit_pull(collection, &page.token, Vec::new())
            .await
            .unwrap();
        if !page.more {
            return;
        }
    }
}

/// Sign up, confirm, sign in on a second machine, and carry an item across — the shell's calls,
/// without the shell. The last push says nothing changed, which is the proof that the laptop
/// recorded what it wrote rather than what it merely received.
#[tokio::test]
#[ignore = "needs a sync server in test-outbox mode; see the module comment"]
async fn the_client_signs_up_signs_in_and_carries_an_item() {
    let dir = tempfile::tempdir().unwrap();
    let email = format!("desktop-{}@example.invalid", uuid::Uuid::new_v4().simple());
    let desktop = SyncState::new(
        Arc::new(InMemory::default()),
        Ok(dir.path().join("desktop.db")),
    );
    let laptop = SyncState::new(
        Arc::new(InMemory::default()),
        Ok(dir.path().join("laptop.db")),
    );

    let recovery = desktop
        .register(&server(), None, &email, "correct horse".into())
        .await
        .unwrap();
    assert_eq!(recovery.split('-').count(), 13);
    let status = desktop
        .verify(&letter(&email, "verification").await, "desktop")
        .await
        .unwrap();
    assert!(status.signed_in);
    laptop
        .login(&server(), None, &email, "correct horse".into(), "laptop")
        .await
        .unwrap();

    let snippet = Item {
        id: "a-snippet".into(),
        data: json!({ "sql": "select 1" }),
    };
    pull_everything(&desktop, "query-snippets").await;
    let pushed = desktop
        .push("query-snippets", vec![snippet.clone()])
        .await
        .unwrap();
    assert_eq!(pushed.accepted, 1);

    let page = laptop.pull_page("query-snippets").await.unwrap();
    assert_eq!(page.changes.upserts, vec![snippet.clone()]);
    laptop
        .commit_pull("query-snippets", &page.token, Vec::new())
        .await
        .unwrap();
    let after = laptop.push("query-snippets", vec![snippet]).await.unwrap();
    assert_eq!(after.accepted, 0);
    assert_eq!(after.token, None);

    assert_eq!(laptop.devices().await.unwrap().len(), 2);
}

/// D6 case 1: the other machine is signed out, nothing is re-encrypted, and the new password is
/// the one that signs in.
#[tokio::test]
#[ignore = "needs a sync server in test-outbox mode; see the module comment"]
async fn changing_the_password_signs_the_others_out_and_keeps_the_records() {
    let dir = tempfile::tempdir().unwrap();
    let (desktop, email) = signed_up(dir.path(), "desktop", "old password").await;
    let laptop = machine_state(dir.path(), "laptop");
    laptop
        .login(&server(), None, &email, "old password".into(), "laptop")
        .await
        .unwrap();
    let snippet = Item {
        id: "kept".into(),
        data: json!({ "sql": "select 1" }),
    };
    pull_everything(&desktop, "query-snippets").await;
    desktop
        .push("query-snippets", vec![snippet.clone()])
        .await
        .unwrap();

    let wrong = desktop
        .change_password("not it".into(), "new password".into())
        .await
        .unwrap_err();
    assert_eq!(wrong.code, "error.syncWrongPassword");
    desktop
        .change_password("old password".into(), "new password".into())
        .await
        .unwrap();

    assert_eq!(
        laptop.devices().await.err().map(|error| error.code),
        Some("error.syncSignedOut")
    );
    let again = machine_state(dir.path(), "again");
    again
        .login(&server(), None, &email, "new password".into(), "again")
        .await
        .unwrap();
    let page = again.pull_page("query-snippets").await.unwrap();
    assert_eq!(page.changes.upserts, vec![snippet]);
}

/// D6 case 2: the code proves who, the recovery key keeps what — and a mistyped key costs a retry,
/// not a second letter, because the ticket is held.
#[tokio::test]
#[ignore = "needs a sync server in test-outbox mode; see the module comment"]
async fn a_forgotten_password_with_the_recovery_key_keeps_the_records() {
    let dir = tempfile::tempdir().unwrap();
    let (desktop, email, recovery) = signed_up_with_key(dir.path(), "desktop", "forgotten").await;
    let snippet = Item {
        id: "kept".into(),
        data: json!({ "sql": "select 2" }),
    };
    pull_everything(&desktop, "query-snippets").await;
    desktop
        .push("query-snippets", vec![snippet.clone()])
        .await
        .unwrap();

    let fresh = machine_state(dir.path(), "fresh");
    fresh.ask_reset(&server(), None, &email).await.unwrap();
    fresh
        .open_reset(&server(), None, &email, &letter(&email, "reset").await)
        .await
        .unwrap();
    let wrong = crypto::format_recovery_key(&crypto::new_recovery_key());
    let refused = fresh
        .reset_keeping(&wrong, "remembered".into(), "fresh")
        .await
        .unwrap_err();
    assert_eq!(refused.code, "error.syncRecoveryKeyWrong");
    fresh
        .reset_keeping(&recovery, "remembered".into(), "fresh")
        .await
        .unwrap();

    let page = fresh.pull_page("query-snippets").await.unwrap();
    assert_eq!(page.changes.upserts, vec![snippet]);
    assert_eq!(
        desktop.devices().await.err().map(|error| error.code),
        Some("error.syncSignedOut")
    );
}

/// D6 case 3: nothing survives on the server, and the machine starts over under a new `MK`, with a
/// new recovery key of the same shape.
#[tokio::test]
#[ignore = "needs a sync server in test-outbox mode; see the module comment"]
async fn a_forgotten_password_without_the_key_starts_over() {
    let dir = tempfile::tempdir().unwrap();
    let (desktop, email) = signed_up(dir.path(), "desktop", "forgotten").await;
    pull_everything(&desktop, "query-snippets").await;
    let pushed = desktop
        .push(
            "query-snippets",
            vec![Item {
                id: "lost".into(),
                data: json!({ "sql": "select 3" }),
            }],
        )
        .await
        .unwrap();
    // On the server before the reset, so that "nothing survives" is about something.
    assert_eq!(pushed.accepted, 1);

    let fresh = machine_state(dir.path(), "fresh");
    fresh.ask_reset(&server(), None, &email).await.unwrap();
    let code = letter(&email, "reset").await;
    let recovery = fresh
        .prepare_start_over(&server(), None, &email, "remembered".into())
        .await
        .unwrap();
    assert_eq!(recovery.split('-').count(), 13);
    fresh.start_over(&code, "fresh").await.unwrap();

    let page = fresh.pull_page("query-snippets").await.unwrap();
    assert!(page.changes.upserts.is_empty());
}

/// D4b: deleting re-proves the password, takes every record, and signs every machine out.
#[tokio::test]
#[ignore = "needs a sync server in test-outbox mode; see the module comment"]
async fn deleting_the_account_takes_everything_and_signs_everyone_out() {
    let dir = tempfile::tempdir().unwrap();
    let (desktop, email) = signed_up(dir.path(), "desktop", "the password").await;
    let laptop = machine_state(dir.path(), "laptop");
    laptop
        .login(&server(), None, &email, "the password".into(), "laptop")
        .await
        .unwrap();
    pull_everything(&desktop, "query-snippets").await;
    desktop
        .push(
            "query-snippets",
            vec![Item {
                id: "a".into(),
                data: json!(1),
            }],
        )
        .await
        .unwrap();

    let wrong = desktop.delete_account("not it".into()).await.unwrap_err();
    assert_eq!(wrong.code, "error.syncWrongPassword");
    assert_eq!(
        desktop.delete_account("the password".into()).await.unwrap(),
        1
    );

    assert!(!desktop.status().await.unwrap().signed_in);
    assert_eq!(
        laptop.devices().await.err().map(|error| error.code),
        Some("error.syncSignedOut")
    );
}

/// A freeze left behind is one request away from over, from any signed-in machine (D4b).
#[tokio::test]
#[ignore = "needs a sync server in test-outbox mode; see the module comment"]
async fn a_frozen_account_is_seen_and_thawed_from_another_machine() {
    let dir = tempfile::tempdir().unwrap();
    let (desktop, email) = signed_up(dir.path(), "desktop", "pw").await;
    let laptop = machine_state(dir.path(), "laptop");
    laptop
        .login(&server(), None, &email, "pw".into(), "laptop")
        .await
        .unwrap();

    // Pulled first, as every machine is before its first push (`SyncState::push`); frozen, the
    // push that follows is then refused by the server rather than held back here.
    pull_everything(&laptop, "query-snippets").await;
    desktop.freeze_for_test().await.unwrap();
    assert_eq!(laptop.freeze_state().await.unwrap().state, "frozen");
    // A refusal comes back beside what the push did, not in place of it (T178c, C2); the loop
    // throws it once it has committed the rest.
    let pushed = laptop
        .push(
            "query-snippets",
            vec![Item {
                id: "a".into(),
                data: json!(1),
            }],
        )
        .await
        .unwrap();
    assert_eq!(
        pushed.error.map(|error| error.code),
        Some("error.syncAccountFrozen")
    );

    assert_eq!(laptop.thaw().await.unwrap().state, "active");
}

/// D4b end to end: the records cross unchanged, the old account is deleted, and the machine goes
/// on syncing against the new server with the same password.
#[tokio::test]
#[ignore = "needs two sync servers in test-outbox mode; see the module comment"]
async fn an_account_moves_to_another_server() {
    let dir = tempfile::tempdir().unwrap();
    let (desktop, email) = signed_up(dir.path(), "desktop", "the password").await;
    let snippets = vec![
        Item {
            id: "a".into(),
            data: json!({ "sql": "select 1" }),
        },
        Item {
            id: "b".into(),
            data: json!({ "sql": "select 2" }),
        },
    ];
    pull_everything(&desktop, "query-snippets").await;
    desktop
        .push("query-snippets", snippets.clone())
        .await
        .unwrap();

    desktop
        .move_begin(&second_server(), None, "the password".into())
        .await
        .unwrap();
    let moved = desktop
        .move_confirm(
            &letter_on(&second_server(), &email, "verification").await,
            "desktop",
        )
        .await
        .unwrap();
    assert_eq!(moved.copied, 2);
    let status = desktop.move_finish(true).await.unwrap();
    assert_eq!(status.server.as_deref(), Some(second_server().as_str()));

    let there = machine_state(dir.path(), "there");
    there
        .login(
            &second_server(),
            None,
            &email,
            "the password".into(),
            "there",
        )
        .await
        .unwrap();
    let page = there.pull_page("query-snippets").await.unwrap();
    assert_eq!(page.changes.upserts.len(), 2);

    let gone = machine_state(dir.path(), "gone")
        .login(&server(), None, &email, "the password".into(), "gone")
        .await
        .unwrap_err();
    assert_eq!(gone.code, "error.syncWrongPassword");
}

/// C3: a mistyped password stops a move at its first step, and the new server never learns it.
#[tokio::test]
#[ignore = "needs two sync servers in test-outbox mode; see the module comment"]
async fn a_move_with_the_wrong_password_is_refused_before_anything_is_registered() {
    let dir = tempfile::tempdir().unwrap();
    let (desktop, email) = signed_up(dir.path(), "desktop", "the password").await;
    let refused = desktop
        .move_begin(&second_server(), None, "the passwort".into())
        .await
        .unwrap_err();
    assert_eq!(refused.code, "error.syncWrongPassword");

    // Nothing was registered there: the address is still free, and the right password registers it.
    desktop
        .move_begin(&second_server(), None, "the password".into())
        .await
        .unwrap();
    letter_on(&second_server(), &email, "verification").await;
}

/// C4: an account moved away and back is a new account on its first server. This machine's store
/// for the old one held a cursor past both records; kept under the address alone, the return
/// pulled nothing until `seq` caught up. Under the account, it starts from nothing and meets both.
#[tokio::test]
#[ignore = "needs two sync servers in test-outbox mode; see the module comment"]
async fn an_account_moved_away_and_back_pulls_everything_again() {
    let dir = tempfile::tempdir().unwrap();
    let (desktop, email) = signed_up(dir.path(), "desktop", "the password").await;
    let snippets = vec![
        Item {
            id: "a".into(),
            data: json!({ "sql": "select 1" }),
        },
        Item {
            id: "b".into(),
            data: json!({ "sql": "select 2" }),
        },
    ];
    pull_everything(&desktop, "query-snippets").await;
    desktop.push("query-snippets", snippets).await.unwrap();
    // Read back once, so this machine's cursor on the first server sits past both records.
    let page = desktop.pull_page("query-snippets").await.unwrap();
    desktop
        .commit_pull("query-snippets", &page.token, Vec::new())
        .await
        .unwrap();

    for to in [second_server(), server()] {
        desktop
            .move_begin(&to, None, "the password".into())
            .await
            .unwrap();
        desktop
            .move_confirm(&letter_on(&to, &email, "verification").await, "desktop")
            .await
            .unwrap();
        let status = desktop.move_finish(true).await.unwrap();
        assert_eq!(status.server.as_deref(), Some(to.as_str()));
    }

    let page = desktop.pull_page("query-snippets").await.unwrap();
    assert_eq!(
        page.changes.upserts.len(),
        2,
        "back on the first server, the new account's records were not pulled"
    );
}

/// A push trusts that a record this machine never saw is one the server does not have, which holds
/// only after this account's collection has been pulled once. After a move the store started
/// empty, and a push before any pull rewrote every record the move had just copied.
#[tokio::test]
#[ignore = "needs a sync server in test-outbox mode; see the module comment"]
async fn a_first_push_waits_for_a_first_pull() {
    let dir = tempfile::tempdir().unwrap();
    let (desktop, _) = signed_up(dir.path(), "desktop", "pw").await;
    let snippet = Item {
        id: "a-snippet".into(),
        data: json!({ "sql": "select 1" }),
    };

    let early = desktop
        .push("query-snippets", vec![snippet.clone()])
        .await
        .unwrap();
    assert!(
        early.needs_pull,
        "pushed before this account was ever pulled"
    );
    assert_eq!(early.accepted, 0);

    pull_everything(&desktop, "query-snippets").await;
    let pushed = desktop.push("query-snippets", vec![snippet]).await.unwrap();
    assert!(!pushed.needs_pull);
    assert_eq!(pushed.accepted, 1);
}

/// The live servers announce no end, and both reads say so: the one for the account this machine
/// is in, and the one the sign-in form makes of any server before signing in.
#[tokio::test]
#[ignore = "needs a sync server in test-outbox mode; see the module comment"]
async fn a_server_with_no_closing_date_reports_none() {
    let dir = tempfile::tempdir().unwrap();
    let (desktop, _) = signed_up(dir.path(), "desktop", "pw").await;
    assert_eq!(desktop.closing_on().await.unwrap(), None);
    let asked = Account::new(&server(), None)
        .unwrap()
        .capabilities()
        .await
        .unwrap();
    assert_eq!(asked.closing_on, None);
    assert!(asked.protocol_versions.contains(&"v1".to_string()));
}
