//! SeaweedFS, a local S3, as a `service` extension — roadmap task **T201**.
//!
//! **The format is not changed for it** (the design's D2), so what these hold to is that the
//! fixture says what `weed server -s3` needs in the vocabulary that already exists: every port it
//! opens is one MixEngine allocated (D1), it reports nothing home (D1a), it is ready when S3 answers
//! (D1b), it is given time to stop (D1c), and its data lives under `{data_dir}`. What the binary
//! itself does with these flags was measured on three systems and is written in the fixture's
//! comments; nothing here downloads it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mixengine_core::extensions::install;
use mixengine_core::extensions::manifest::{self, Body, ExtensionManifest};
use mixengine_core::extensions::render::{self, Context};
use mixengine_core::{Error, Paths, Store};
use mixengine_proto::{Millis, ReadyCheck, ServiceSpec, StopBehaviour};
use tempfile::TempDir;

/// Every key the fixture's `[ports]` must hold — four HTTP ports and the gRPC port beside each.
const PORTS: [&str; 8] = [
    "filer_grpc_port",
    "filer_port",
    "master_grpc_port",
    "master_port",
    "s3_grpc_port",
    "s3_port",
    "volume_grpc_port",
    "volume_port",
];

fn seaweedfs() -> ExtensionManifest {
    manifest::read(
        Path::new("extension.toml"),
        mixengine_testkit::extension::SEAWEEDFS,
    )
    .expect("the fixture parses")
}

async fn home() -> (TempDir, Paths, Store) {
    let directory = TempDir::new().expect("a temporary directory");
    let paths = Paths::new(
        directory.path().to_path_buf(),
        &mixengine_core::config::PathOverrides::default(),
    );
    let store = Store::open(paths.database_file())
        .await
        .expect("a database");

    (directory, paths, store)
}

/// An absolute path on whichever system this runs on — a spec refuses a relative `program`, and
/// `/mixengine` is relative on Windows.
fn absolute(relative: &str) -> PathBuf {
    std::env::temp_dir().join(relative)
}

/// The context an installed SeaweedFS renders under, with `ports` as the install handed them out.
fn installed_context(data_dir: PathBuf, ports: BTreeMap<String, u16>) -> Context {
    let manifest = seaweedfs();
    let mut context = Context::planned(
        &Paths::new(
            absolute("mixengine"),
            &mixengine_core::config::PathOverrides::default(),
        ),
        &manifest,
    );
    context.data_dir = data_dir;
    context.ports = ports;
    context
}

fn spec_of(context: &Context) -> ServiceSpec {
    let manifest = seaweedfs();
    let Body::Service(template) = &manifest.body else {
        panic!("SeaweedFS is a service");
    };

    render::service_spec(&manifest, template, context).expect("the spec renders")
}

fn args_of(context: &Context) -> Vec<String> {
    spec_of(context).args().to_vec()
}

fn default_args() -> Vec<String> {
    args_of(&installed_context(
        absolute("seaweedfs-data"),
        seaweedfs().ports,
    ))
}

/// The value that follows `flag` in a rendered argument list.
fn after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|argument| argument == flag)
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
}

/// **D1.** A port `weed` opens and `[ports]` does not name is a port nobody allocated, and the
/// first person with something on 19333 meets an error about a number they never saw.
#[test]
fn every_port_weed_opens_is_one_mixengine_allocates() {
    let manifest = seaweedfs();

    assert_eq!(
        manifest
            .ports
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        PORTS,
    );
}

/// **D1.** Each port reaches `weed` through its own flag, so it derives none of them.
#[test]
fn each_port_is_passed_with_its_own_flag() {
    let manifest = seaweedfs();
    let args = default_args();

    for (flag, key) in [
        ("-master.port", "master_port"),
        ("-master.port.grpc", "master_grpc_port"),
        ("-volume.port", "volume_port"),
        ("-volume.port.grpc", "volume_grpc_port"),
        ("-filer.port", "filer_port"),
        ("-filer.port.grpc", "filer_grpc_port"),
        ("-s3.port", "s3_port"),
        ("-s3.port.grpc", "s3_grpc_port"),
    ] {
        assert_eq!(
            after(&args, flag),
            Some(manifest.ports[key].to_string().as_str()),
            "{flag} in {args:?}"
        );
    }
}

/// **D1.** 4.48 also opens an Iceberg catalog on 8181 and a Lance server on 9101 unless told `0`.
/// Two fixed ports nobody allocated are two collisions waiting for a second home.
#[test]
fn the_two_listeners_nothing_needs_are_switched_off() {
    let args = default_args();

    assert_eq!(after(&args, "-s3.port.iceberg"), Some("0"), "{args:?}");
    assert_eq!(after(&args, "-s3.port.lance"), Some("0"), "{args:?}");
}

/// **D1a.** Nothing leaves a person's machine unasked. A Go boolean flag takes its value after `=`
/// and not as the next argument, so this is one argument.
#[test]
fn it_reports_nothing_home() {
    let args = default_args();

    assert!(
        args.iter()
            .any(|argument| argument == "-master.telemetry=false"),
        "{args:?}"
    );
}

/// **Review focus 1.** 8080 is taken on half the machines this runs on. The install moves it, and
/// the flag has to carry the number the install gave, not the one the manifest asked for.
#[test]
fn a_moved_port_renders_the_number_the_install_was_given() {
    let mut ports = seaweedfs().ports;
    ports.insert("volume_port".to_owned(), 18_080);

    let args = args_of(&installed_context(absolute("seaweedfs-data"), ports));

    assert_eq!(after(&args, "-volume.port"), Some("18080"), "{args:?}");
}

/// **Review focus 3.** A home under `C:\Users\Nguyen Hai Quang` is one argument, because `-dir`
/// and its value are two entries of `args` rather than one string somebody splits.
#[test]
fn the_data_directory_is_one_argument_even_with_a_space() {
    let data_dir = absolute("Nguyen Hai Quang").join("seaweedfs");
    let args = args_of(&installed_context(data_dir.clone(), seaweedfs().ports));

    assert_eq!(
        after(&args, "-dir").map(PathBuf::from),
        Some(data_dir),
        "{args:?}"
    );
}

/// **D1b.** Ready when S3 answers HTTP: the port opens before the master has a leader, and a TCP
/// check would call it up while every request still fails. The S3 port is also the one
/// `services.port` holds (T81, D8).
#[test]
fn it_is_ready_when_s3_answers() {
    let manifest = seaweedfs();
    let spec = spec_of(&installed_context(
        absolute("seaweedfs-data"),
        manifest.ports.clone(),
    ));

    let ReadyCheck::Http {
        url,
        expect_status,
        timeout,
    } = spec.ready()
    else {
        panic!("ready is an HTTP check: {:?}", spec.ready());
    };
    assert_eq!(
        url,
        &format!("http://127.0.0.1:{}/healthz", manifest.ports["s3_port"])
    );
    assert_eq!(*expect_status, 200);
    assert_eq!(*timeout, Millis::from_secs(60), "a restart took up to 20 s");
    assert_eq!(
        mixengine_core::extensions::recipe::served_port_name(&manifest),
        Some("s3_port")
    );
}

/// **D1c.** `weed` takes 16 s to leave after a signal even with the volume server's pre-stop pause
/// off, so the default 10 s grace would end every stop in a kill.
#[test]
fn a_stop_is_given_time() {
    let args = default_args();
    let spec = spec_of(&installed_context(
        absolute("seaweedfs-data"),
        seaweedfs().ports,
    ));

    assert_eq!(
        after(&args, "-volume.preStopSeconds"),
        Some("0"),
        "{args:?}"
    );
    assert_eq!(
        spec.stop(),
        &StopBehaviour::Signal {
            grace: Millis::from_secs(30)
        }
    );
}

/// **D3.** Open goes to the filer's file browser.
#[test]
fn open_goes_to_the_filer() {
    let ui = seaweedfs().ui.expect("SeaweedFS has a page");

    assert_eq!(ui.port, "filer_port");
}

/// **D1.** The plan names all eight ports before anything is fetched.
#[tokio::test]
async fn the_plan_asks_for_eight_ports() {
    let (_home, paths, store) = home().await;

    // Every machine CI runs on has a build; Windows on ARM, which has none, is the next test's.
    let plan = install::plan(&store, &paths, &seaweedfs(), false)
        .await
        .expect("a plan on a machine upstream publishes for");

    assert_eq!(plan.ports.len(), PORTS.len());
}

/// **Review focus 4.** Upstream publishes nothing for Windows on ARM, and a machine without a build
/// is told which ones exist before a byte is fetched.
#[tokio::test]
async fn a_machine_with_no_build_is_told_which_ones_exist() {
    let (_home, paths, store) = home().await;
    let mut manifest = seaweedfs();
    let published = manifest.artifacts["linux-x86_64"].clone();
    manifest.artifacts.clear();
    manifest
        .artifacts
        .insert("no-such-target".to_owned(), published);

    let refusal = install::plan(&store, &paths, &manifest, false)
        .await
        .expect_err("nothing is published for this machine");

    assert!(
        matches!(refusal, Error::ExtensionNoArtifact { ref targets, .. } if targets == &["no-such-target"]),
        "{refusal}"
    );
    assert_eq!(
        seaweedfs()
            .artifacts
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "linux-aarch64",
            "linux-x86_64",
            "macos-aarch64",
            "macos-x86_64",
            "windows-x86_64"
        ],
        "the five targets upstream publishes, and not Windows on ARM"
    );
}
