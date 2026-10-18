//! What a home answers for: repair, bundles, doctor, the authority and blueprints.

use super::*;

/// `daemon.doctor_repair` on a fresh home — roadmap task **T47b**.
///
/// **What is asserted is the shape and the silence, not a cure.** A home a moment old has nothing
/// this build repairs *except* whatever the machine running the suite brings with it — a reserved
/// port range, a hosts block somebody edited — so the assertion is about the document: every entry
/// names a condition and carries one of the three actions, and `granting` is always present so a
/// client can render "an administrator's permission is needed" off it without guessing.
///
/// The repairs themselves are proved where they can be: the table in `repair.rs`'s own tests, and
/// the one repair a suite can drive end to end in `mixengine-cli/tests/doctor.rs`.
#[tokio::test]
async fn a_repair_answers_a_record_of_what_it_did() {
    let daemon = Daemon::start().await;

    let answer = daemon
        .rpc(r#"{"jsonrpc":"2.0","method":"daemon.doctor_repair","id":1}"#)
        .await;

    let actions = answer["result"]["actions"]
        .as_array()
        .unwrap_or_else(|| panic!("a list of actions: {answer}"));

    for action in actions {
        assert!(
            action["id"].as_str().is_some_and(|id| !id.is_empty()),
            "every entry names the condition it is about: {action}"
        );
        assert!(
            action["name"].as_str().is_some_and(|name| !name.is_empty()),
            "every entry carries the check's own name: {action}"
        );
        assert!(
            matches!(
                action["outcome"]["action"].as_str(),
                Some("repaired" | "enqueued" | "untouched")
            ),
            "{action}"
        );
    }

    assert!(
        answer["result"].get("granting").is_some(),
        "a repair always says whether it raised a prompt: {answer}"
    );
}

/// `daemon.bundle` on a fresh home — roadmap task **T93**.
///
/// **What is asserted is the archive, never the machine.** A home a moment old on a CI runner has
/// whatever that runner brings with it, so nothing here reads the doctor report inside the bundle —
/// only that the six members this build declares are the six entries in the file, and that what
/// was left out is stated rather than left for the reader to discover.
#[tokio::test]
async fn a_bundle_holds_the_members_this_build_declares() {
    let daemon = Daemon::start().await;

    let answer = daemon
        .rpc(r#"{"jsonrpc":"2.0","method":"daemon.bundle","id":1}"#)
        .await;

    let path = answer["result"]["path"]
        .as_str()
        .unwrap_or_else(|| panic!("the archive names where it went: {answer}"));

    let file = std::fs::File::open(path).unwrap_or_else(|error| panic!("{error}: {path}"));
    let mut archive = zip::ZipArchive::new(file).unwrap_or_else(|error| panic!("{error}: {path}"));

    let mut entries: Vec<String> = archive
        .file_names()
        .map(|name| name.expect("an entry name").into_owned())
        .collect();
    entries.sort_unstable();

    assert_eq!(
        entries,
        [
            "crashes.json",
            "daemon.log",
            "doctor.json",
            "manifest.json",
            "platform.json",
            "status.json",
        ],
        "{answer}"
    );

    // **A home a moment old has never crashed**, so the one thing this can assert about the member
    // without reading the machine is that it is an array and that it is empty — which is the answer
    // "nothing has gone wrong here" rather than a member nobody wrote. Roadmap task **T91**.
    let mut crashes = String::new();
    std::io::Read::read_to_string(
        &mut archive
            .by_name("crashes.json")
            .unwrap_or_else(|error| panic!("{error}: {path}")),
        &mut crashes,
    )
    .unwrap_or_else(|error| panic!("{error}: {path}"));

    assert_eq!(
        serde_json::from_str::<Vec<mixengine_proto::CrashReport>>(&crashes)
            .unwrap_or_else(|error| panic!("{error}: {crashes}")),
        [],
        "{answer}"
    );

    let omitted = answer["result"]["omitted"]
        .as_array()
        .unwrap_or_else(|| panic!("a list of what was left out: {answer}"));

    assert!(
        omitted.iter().any(|left| left["name"] == "etc/"),
        "the rendered configuration is named rather than silently absent: {answer}"
    );
    for left in omitted {
        assert!(
            left["because"]
                .as_str()
                .is_some_and(|because| !because.is_empty()),
            "an omission with no reason is an omission nobody can act on: {left}"
        );
    }
}

/// `daemon.doctor` on a fresh home — roadmap task **T47a**.
///
/// **The assertion is that every check appears**, whatever it answered. A doctor that dropped the
/// checks it had nothing to say about would read as a clean bill of health on the system where it
/// examined the least, which is the failure the fixed-order list exists to prevent.
#[tokio::test]
async fn the_doctor_reports_every_check_and_none_of_them_is_missing() {
    let daemon = Daemon::start().await;

    let answer = daemon
        .rpc(r#"{"jsonrpc":"2.0","method":"daemon.doctor","id":1}"#)
        .await;

    let checks = answer["result"]["checks"]
        .as_array()
        .unwrap_or_else(|| panic!("a list of checks: {answer}"));

    assert_eq!(checks.len(), 26, "{answer}");

    for check in checks {
        assert!(
            check["name"].as_str().is_some_and(|name| !name.is_empty()),
            "every check names what it examined: {check}"
        );
        assert!(
            matches!(
                check["outcome"]["outcome"].as_str(),
                Some("ok" | "note" | "problem" | "skipped")
            ),
            "{check}"
        );
    }

    // The orphan guarantee is a `Note` on every system — never a problem, and never silence. That
    // is ADR 0007's whole reason for existing, held here rather than assumed.
    let descendants = checks
        .iter()
        .find(|check| {
            check["name"]
                .as_str()
                .is_some_and(|name| name.contains("descendant"))
        })
        .unwrap_or_else(|| panic!("the descendants check: {answer}"));

    assert_eq!(descendants["outcome"]["outcome"], "note", "{descendants}");
    assert!(
        descendants["outcome"]["because"]
            .as_str()
            .is_some_and(|because| !because.is_empty()),
        "{descendants}"
    );
}

/// The two checks that reach outside the daemon, and the shape each takes here — roadmap task
/// **T47a**.
#[tokio::test]
async fn the_doctor_reads_the_domains_and_says_what_it_could_not_examine() {
    let daemon = Daemon::start().await;

    let answer = daemon
        .rpc(r#"{"jsonrpc":"2.0","method":"daemon.doctor","id":1}"#)
        .await;

    let checks = answer["result"]["checks"]
        .as_array()
        .unwrap_or_else(|| panic!("a list of checks: {answer}"));

    let ports = checks
        .iter()
        .find(|check| {
            check["name"]
                .as_str()
                .is_some_and(|name| name.contains("reserved"))
        })
        .unwrap_or_else(|| panic!("the reserved-ports check: {answer}"));

    let outcome = ports["outcome"]["outcome"]
        .as_str()
        .unwrap_or_else(|| panic!("a word: {ports}"));

    if cfg!(windows) {
        assert!(matches!(outcome, "ok" | "note" | "problem"), "{ports}");
    } else {
        assert_eq!(outcome, "skipped", "{ports}");
        assert!(
            ports["outcome"]["because"]
                .as_str()
                .is_some_and(|why| !why.is_empty()),
            "a skipped check that does not say why is silence with extra steps: {ports}"
        );
    }

    // A home with no sites has no domain that could be unreachable, which is `Ok` and not a fault.
    let domains = checks
        .iter()
        .find(|check| {
            check["name"]
                .as_str()
                .is_some_and(|name| name.contains("domain"))
        })
        .unwrap_or_else(|| panic!("the domains check: {answer}"));

    assert_eq!(domains["outcome"]["outcome"], "ok", "{domains}");
}

/// A started daemon has a certificate authority, before anything has asked for one.
///
/// **The ordering claim of the T48 design's D4, proved where it is made.** A unit test calling
/// `ensure` proves the function works; only a daemon started over an empty home proves that
/// something calls it at start — which is what lets T49 put the trust-store install in the same
/// single first-run elevation batch as the resolver and the port grant. Asked over the socket and
/// then checked on disk, because the two are different claims: the second is where T49 and T50 will
/// look.
#[tokio::test]
async fn a_started_daemon_has_an_authority_before_anything_asks_for_one() {
    let daemon = Daemon::start().await;

    let answer = daemon
        .rpc(r#"{"jsonrpc":"2.0","method":"cert.ca_status","id":1}"#)
        .await;

    let result = &answer["result"];

    assert_eq!(result["state"], "present", "{answer}");

    let ca = &result["ca"];

    assert_eq!(
        ca["fingerprint"].as_str().map(str::len),
        Some(64),
        "a SHA-256 in hex is 64 characters: {answer}"
    );
    assert!(
        ca["subject"]
            .as_str()
            .is_some_and(|subject| subject.contains("MixEngine Local CA")),
        "{answer}"
    );
    assert!(
        ca["days_left"].as_i64().is_some_and(|days| days > 3_600),
        "the authority is not ten years long: {answer}"
    );

    let pem = ca["certificate_pem"]
        .as_str()
        .unwrap_or_else(|| panic!("the status carries the certificate: {answer}"));

    assert!(pem.contains("-----BEGIN CERTIFICATE-----"), "{answer}");
    assert!(
        !pem.contains("PRIVATE"),
        "the status carried a private key: {answer}"
    );

    // On disk, where T49 installs from and T50 signs with. The RPC answering does not prove this.
    let root = mixengine_platform::paths::in_full(daemon.home.path());
    let paths = Paths::new(root, &PathOverrides::default());

    assert!(
        mixengine_core::certs::ca::certificate_path(paths.certs()).exists(),
        "the certificate is not where the rest of the product will look for it"
    );
    assert!(
        mixengine_core::certs::ca::key_path(paths.certs()).exists(),
        "the private key is not on disk"
    );
}

/// A blueprint written by hand, with the one section a capture never writes.
const HAND_WRITTEN_BLUEPRINT: &str = r#"schema = 1

[blueprint]
name = "borrowed"
description = "somebody else's stack"
created_at = "2026-09-01T00:00:00Z"
created_on = { os = "linux", version = "0.1.0" }

[site]
kind = "static"
doc_root = "public"
https = false
domain_pattern = "{project}.test"

[scaffold]
command = "printf hello > made.txt"
"#;

/// **Import is the only thing that can produce a blueprint this machine did not write** — roadmap
/// task **T78a**, its design's D3. Nothing vouched for this one, so it lands untrusted and the
/// listing says so.
#[tokio::test]
async fn an_imported_blueprint_without_a_signature_is_untrusted() {
    let daemon = Daemon::start().await;
    let file = daemon.home().join("borrowed.toml");
    std::fs::write(&file, HAND_WRITTEN_BLUEPRINT).expect("a file to import");

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "blueprint.import",
        "id": 1,
        "params": { "path": file.display().to_string() }
    })
    .to_string();

    let answer = daemon.rpc(&body).await;

    assert_eq!(answer["result"]["source"], "imported", "{answer}");
    assert_eq!(answer["result"]["trusted"], false, "{answer}");
    assert_eq!(answer["result"]["slug"], "borrowed", "{answer}");

    // **Which kind of untrusted** — roadmap task **T79b**. Nothing came with this one, and that is
    // not the same event as a signature that did not verify.
    assert_eq!(answer["result"]["signature"], "missing", "{answer}");

    let listed = daemon
        .rpc(r#"{"jsonrpc":"2.0","method":"blueprint.list","id":2}"#)
        .await;

    assert_eq!(
        listed["result"]["blueprints"][0]["trusted"], false,
        "{listed}"
    );
}

/// **A signature that did not verify is not the same event as no signature at all** — roadmap task
/// **T79b**. Both land untrusted, and only this one is what the gallery key exists to catch: a
/// manifest edited after somebody signed it.
///
/// The listing is asserted as well as the answer, because that is the half that proves the reason
/// is a **column**. A test reading only what `import` returned would stay green with the migration
/// broken, and the reason would be gone by the next `mix blueprint list`.
#[tokio::test]
async fn an_imported_blueprint_whose_signature_does_not_verify_says_so() {
    let daemon = Daemon::start().await;
    let file = daemon.home().join("borrowed.toml");
    std::fs::write(&file, HAND_WRITTEN_BLUEPRINT).expect("a file to import");

    // Not a signature at all, which the verifier folds in with a stale one and a foreign one —
    // three events, one word, and one sentence that has to be true of all three.
    std::fs::write(
        daemon.home().join("borrowed.toml.minisig"),
        "untrusted comment: nothing\nthis is not a signature\n",
    )
    .expect("a signature beside it");

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "blueprint.import",
        "id": 1,
        "params": { "path": file.display().to_string() }
    })
    .to_string();

    let answer = daemon.rpc(&body).await;

    assert_eq!(answer["result"]["trusted"], false, "{answer}");
    assert_eq!(answer["result"]["signature"], "rejected", "{answer}");

    let listed = daemon
        .rpc(r#"{"jsonrpc":"2.0","method":"blueprint.list","id":2}"#)
        .await;

    let borrowed = listed["result"]["blueprints"]
        .as_array()
        .expect("a listing")
        .iter()
        .find(|row| row["slug"] == "borrowed")
        .unwrap_or_else(|| panic!("no row for the blueprint that was imported: {listed}"));

    assert_eq!(borrowed["trusted"], false, "{listed}");
    assert_eq!(borrowed["signature"], "rejected", "{listed}");
}

/// A file that is not a manifest is refused by name, rather than filed as something to find out
/// about at apply time.
#[tokio::test]
async fn a_file_that_is_not_a_manifest_is_refused() {
    let daemon = Daemon::start().await;
    let file = daemon.home().join("notes.toml");
    std::fs::write(&file, "this is not a blueprint").expect("a file");

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "blueprint.import",
        "id": 1,
        "params": { "path": file.display().to_string() }
    })
    .to_string();

    let answer = daemon.rpc(&body).await;

    assert_eq!(
        answer["error"]["data"]["code"], "invalid_argument",
        "{answer}"
    );
}

/// **A gallery file imported by hand is filed under its filename** — roadmap task **T79a**, its
/// design's D10. `[blueprint] name` is display text: the six say `Laravel`, `Next.js`, `Static
/// site`, and `validated_slug` takes none of them. Before D10 this import failed with
/// `InvalidBlueprintName`, so the signed publication had nothing to land on.
///
/// Every entry rather than one, because the two names that break every rule — a dot and a space —
/// are only reached by walking the whole gallery. `overwrite` because a seeded home already holds
/// all six.
#[tokio::test]
async fn every_gallery_file_imported_by_hand_is_filed_under_its_filename() {
    let daemon = Daemon::start().await;

    for (id, entry) in mixengine_core::blueprints::gallery::ENTRIES
        .iter()
        .enumerate()
    {
        let file = daemon.home().join(format!("{}.toml", entry.slug));
        std::fs::write(&file, entry.manifest).expect("a file to import");

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "blueprint.import",
            "id": id + 1,
            "params": { "path": file.display().to_string(), "overwrite": true }
        })
        .to_string();

        let answer = daemon.rpc(&body).await;

        assert_eq!(answer["result"]["slug"], entry.slug, "{answer}");
        assert_eq!(answer["result"]["source"], "imported", "{answer}");
        assert_eq!(answer["result"]["trusted"], false, "{answer}");
    }
}
