//! Turning a request body into an answer: batches, notifications, dispatch, and panic containment.
//!
//! The whole of `POST /rpc` is here, and it is deliberately separate from [`super::http`]: this
//! module never sees a header, a status code or a socket, so everything it does can be tested by
//! handing it a slice of bytes.

use std::sync::Arc;

use mixengine_proto::rpc::{self, Id, Request, Response, RpcCode, RpcError};
use mixengine_proto::{
    BlueprintApply, BlueprintCapture, BlueprintImport, CaRotateQuery, CaStatusQuery,
    CaUninstallQuery, CertIssue, CertStatusQuery, CleanupQuery, DatabaseClientQuery,
    DatabaseCreate, DatabaseCredentialsQuery, DatabaseOpen, DiagnosticsBundle, DiskUsageQuery,
    DoctorRepair, DomainAdd, DomainRemove, DomainStatusQuery, ElevationDrop, Error, ErrorCode,
    ExtensionAvailable, ExtensionChoice, ExtensionInspect, ExtensionInstall, ExtensionPlanRequest,
    ExtensionTarget, ExtensionUninstall, FrontEndSwitch, JobFilter, JobList, JobQuery, JobWait,
    MetricsFrame, MetricsHistory, MetricsHistoryQuery, PackageFilter, PackageInstall,
    PackageTarget, PackageUpgrade, PackageUpgradeQuery, ProjectCreate, ProjectQuery, ProjectUpdate,
    ResetCredential, RuntimeFilter, RuntimeInstall, RuntimeQuestion, RuntimeTarget,
    RuntimeUninstall, RuntimeUpgrade, RuntimeUpgradeQuery, SaveResourcesSet, ServiceAutostartSet,
    ServiceCreate, ServiceDelete, ServiceIdleSet, ServiceLimitsSet, ServiceQuery, ServiceTarget,
    SiteCreate, SiteListQuery, SiteQuery, SiteShare, SiteUpdate, UninstallQuery, UpdateApply,
    UpdateCheck, UpdateDecide, UpdateFinish, UpdateHandOver,
};
use serde_json::Value;
use tracing::Instrument as _;

use super::Api;
use crate::error::ToWire as _;

/// Answer a `POST /rpc` body.
///
/// `None` means *write nothing back*: the body held only notifications, and the spec is explicit
/// that a server returns nothing for those. The caller turns that into `204 No Content` rather than
/// into an empty `200`, because an empty body is not valid JSON and a client that parses every
/// response would choke on one.
pub(super) async fn answer(api: &Arc<Api>, body: &[u8]) -> Option<Vec<u8>> {
    let payload: Value = match serde_json::from_slice(body) {
        Ok(payload) => payload,

        // The one failure where the id is genuinely unknowable, and the only place a `null` id is
        // correct. Note it is still an HTTP 200: the transport worked, the call did not.
        Err(error) => {
            let response = Response::failure(
                None,
                RpcError::at(
                    RpcCode::PARSE_ERROR,
                    Error::new(
                        ErrorCode::InvalidArgument,
                        format!("the request body is not JSON: {error}"),
                    )
                    .with_hint(
                        "`POST /rpc` takes one JSON-RPC 2.0 request object, or an array of them",
                    ),
                ),
            );

            return Some(encode(&response));
        }
    };

    match payload {
        Value::Array(calls) => {
            // An empty batch is the one array the spec singles out: there is nothing to answer, so
            // a server that returned `[]` would be answering a request it never received.
            if calls.is_empty() {
                let response = Response::failure(
                    None,
                    RpcError::at(
                        RpcCode::INVALID_REQUEST,
                        Error::new(ErrorCode::InvalidArgument, "the batch is empty"),
                    ),
                );

                return Some(encode(&response));
            }

            let mut answers = Vec::with_capacity(calls.len());

            // Sequential rather than concurrent. Running a batch in parallel would be free today —
            // no handler touches shared state — and would stop being free the moment one does,
            // silently, in whichever phase adds it. The spec promises nothing about order, so the
            // conservative reading costs a client nothing.
            for call in calls {
                if let Some(answer) = dispatch(api, call).await {
                    answers.push(answer);
                }
            }

            (!answers.is_empty()).then(|| encode(&answers))
        }

        single => dispatch(api, single).await.as_ref().map(encode),
    }
}

/// One call out of a body, answered — or not, if it was a notification.
async fn dispatch(api: &Arc<Api>, call: Value) -> Option<Response> {
    // Read out before the call is decoded, because a request that fails to decode still has to be
    // answered *to the id it claimed*: a client matching answers to calls has no other way to know
    // which of its five requests was the malformed one.
    let claimed = call.get("id");

    // A notification is a request with no `id` **member**, which is not the same thing as one whose
    // id is `null`: the spec discourages the latter but nowhere lets it mean "answer nothing", so a
    // client that sends one and waits would wait forever. The two are indistinguishable after
    // decoding — `Option<Id>` reads an absent member and a null one alike — so the distinction is
    // taken here, from the JSON, and never from `Request::is_notification`.
    let wants_an_answer = claimed.is_some();
    let id = claimed
        .cloned()
        .and_then(|id| serde_json::from_value::<Id>(id).ok());

    let request: Request = match serde_json::from_value(call) {
        Ok(request) => request,

        Err(error) => {
            return Some(Response::failure(
                id,
                RpcError::at(
                    RpcCode::INVALID_REQUEST,
                    Error::new(
                        ErrorCode::InvalidArgument,
                        format!("not a JSON-RPC request: {error}"),
                    )
                    .with_hint(
                        "a request is `{\"jsonrpc\":\"2.0\",\"method\":\"…\",\"id\":1}`, with \
                         `params` where the method takes them",
                    ),
                ),
            ));
        }
    };

    // The id is rendered into the span up front rather than recorded onto an `Empty` field
    // afterwards: `tracing-subscriber` formats a span's fields once and *appends* what is recorded
    // later, so the second way prints `id=3 id=3`. Each of the three cases is a value and not a
    // missing field — "this call wanted no answer" is worth being able to see in the log, and so is
    // the odd client that asked to be answered to `null`.
    let called = match (&id, wants_an_answer) {
        (Some(id), _) => id.to_string(),
        (None, true) => "null".to_owned(),
        (None, false) => "-".to_owned(),
    };
    let span = tracing::info_span!("rpc", method = %request.method, id = %called);

    let outcome = call_method(Arc::clone(api), request.method, request.params)
        .instrument(span)
        .await;

    // A notification is answered by silence even when it failed. The client asked not to be told,
    // and the daemon's own log is where that failure goes — which is why `call_method` logs it
    // rather than leaving it to whoever builds the response.
    if !wants_an_answer {
        return None;
    }

    // `id` is `None` only for the client that spelled its own id `null`, and that is what it gets
    // back: an answer echoes the id it was given rather than improving on it.
    Some(match outcome {
        Ok(result) => Response::success(id, result),
        Err(failure) => Response::failure(id, failure.into_rpc()),
    })
}

/// Run one method, containing anything it does to itself.
///
/// The call happens inside a spawned task purely so that a panic becomes a value. A handler that
/// panics must not take the daemon with it — `docs/standards/rust.md` is explicit that a panic
/// here kills every managed service — and the connection alone is not enough to sacrifice either:
/// the client would see a dropped socket and have no idea whether its request had been carried out.
/// `panic = "abort"` in the release profile would defeat this, which is why the workspace manifest
/// says so out loud.
async fn call_method(
    api: Arc<Api>,
    method: String,
    params: Option<Value>,
) -> Result<Value, Failure> {
    let named = method.clone();

    let outcome = tokio::spawn(
        async move {
            match method.as_str() {
                rpc::method::DAEMON_STATUS => {
                    no_params(params.as_ref())?;
                    encode_result(&api.status().await.map_err(refused)?)
                }

                rpc::method::DAEMON_VERSION => {
                    no_params(params.as_ref())?;
                    encode_result(&api.version())
                }

                rpc::method::DAEMON_SHUTDOWN => {
                    no_params(params.as_ref())?;
                    encode_result(&api.daemon_shutdown().await)
                }
                rpc::method::DAEMON_SET_CREDENTIAL_STORE => {
                    let asked: mixengine_proto::CredentialStoreSet = arguments(params)?;
                    encode_result(&api.set_credential_store(asked).await.map_err(refused)?)
                }

                rpc::method::UPDATE_STATUS => {
                    no_params(params.as_ref())?;
                    encode_result(&api.update_status().await)
                }

                rpc::method::UPDATE_CHECK => {
                    let check: UpdateCheck = arguments(params)?;
                    encode_result(&api.update_check(check).await.map_err(refused)?)
                }

                rpc::method::UPDATE_DECIDE => {
                    let decide: UpdateDecide = arguments(params)?;
                    encode_result(&api.update_decide(decide).await.map_err(refused)?)
                }

                rpc::method::UPDATE_APPLY => {
                    let apply: UpdateApply = arguments(params)?;
                    encode_result(&api.update_apply(apply).await.map_err(refused)?)
                }

                rpc::method::UPDATE_HAND_OVER => {
                    let hand_over: UpdateHandOver = arguments(params)?;
                    encode_result(&api.update_hand_over(hand_over).await.map_err(refused)?)
                }

                rpc::method::UPDATE_FINISH => {
                    let finish: UpdateFinish = arguments(params)?;
                    encode_result(&api.update_finish(finish).await.map_err(refused)?)
                }

                rpc::method::DAEMON_UNINSTALL_PLAN => {
                    let query: UninstallQuery = arguments(params)?;
                    encode_result(&api.uninstall.plan(&query).await.map_err(refused)?)
                }

                rpc::method::DAEMON_UNINSTALL => {
                    let query: UninstallQuery = arguments(params)?;
                    encode_result(&api.uninstall_now(query).await.map_err(refused)?)
                }

                rpc::method::DAEMON_DISK_USAGE => {
                    let query: DiskUsageQuery = arguments(params)?;
                    encode_result(&api.disk.usage(&query).await.map_err(refused)?)
                }

                rpc::method::DAEMON_CLEANUP => {
                    let query: CleanupQuery = arguments(params)?;
                    encode_result(&api.cleanup_now(query).await.map_err(refused)?)
                }

                rpc::method::RUNTIME_LIST_AVAILABLE => {
                    let filter: RuntimeFilter = arguments(params)?;
                    encode_result(
                        &api.runtimes
                            .list_available(&filter)
                            .await
                            .map_err(refused)?,
                    )
                }

                rpc::method::RUNTIME_LIST_INSTALLED => {
                    let filter: RuntimeFilter = arguments(params)?;
                    encode_result(
                        &api.runtimes
                            .list_installed(&filter)
                            .await
                            .map_err(refused)?,
                    )
                }

                rpc::method::RUNTIME_INSTALL => {
                    let asked: RuntimeInstall = arguments(params)?;
                    encode_result(&api.runtimes.install(&asked).await.map_err(refused)?)
                }

                rpc::method::RUNTIME_REQUIREMENTS => {
                    let target: RuntimeTarget = arguments(params)?;
                    encode_result(&api.runtimes.requirements(&target).await.map_err(refused)?)
                }

                rpc::method::RUNTIME_UNINSTALL => {
                    let asked: RuntimeUninstall = arguments(params)?;
                    api.not_being_upgraded(asked.target.kind.as_str(), &asked.target.version)
                        .await
                        .map_err(refused)?;
                    encode_result(&api.runtimes.uninstall(&asked).await.map_err(refused)?)
                }

                rpc::method::RUNTIME_FOUND => {
                    encode_result(&api.runtimes.found().await.map_err(refused)?)
                }

                rpc::method::PACKAGE_FOUND => {
                    encode_result(&api.packages.found().await.map_err(refused)?)
                }

                rpc::method::RUNTIME_ADOPT => {
                    let target: RuntimeTarget = arguments(params)?;
                    encode_result(&api.runtimes.adopt(&target).await.map_err(refused)?)
                }

                rpc::method::RUNTIME_SET_DEFAULT => {
                    let target: RuntimeTarget = arguments(params)?;
                    encode_result(&api.runtimes.set_default(&target).await.map_err(refused)?)
                }

                rpc::method::RUNTIME_LIST_EXTENSIONS => {
                    let target: RuntimeTarget = arguments(params)?;
                    encode_result(&api.php_extensions.list(&target).await.map_err(refused)?)
                }

                rpc::method::RUNTIME_SET_EXTENSION => {
                    let choice: ExtensionChoice = arguments(params)?;
                    encode_result(&api.php_extensions.set(&choice).await.map_err(refused)?)
                }

                rpc::method::PACKAGE_LIST => {
                    let filter: PackageFilter = arguments(params)?;
                    encode_result(&api.packages.list(&filter).await.map_err(refused)?)
                }

                rpc::method::PACKAGE_LIST_AVAILABLE => {
                    let filter: PackageFilter = arguments(params)?;
                    encode_result(
                        &api.packages
                            .list_available(&filter)
                            .await
                            .map_err(refused)?,
                    )
                }

                rpc::method::PACKAGE_INSTALL => {
                    let asked: PackageInstall = arguments(params)?;
                    encode_result(&api.packages.install(&asked).await.map_err(refused)?)
                }

                rpc::method::PACKAGE_REQUIREMENTS => {
                    let target: PackageTarget = arguments(params)?;
                    encode_result(&api.packages.requirements(&target).await.map_err(refused)?)
                }

                rpc::method::PACKAGE_ADOPT => {
                    let target: PackageTarget = arguments(params)?;
                    encode_result(&api.packages.adopt(&target).await.map_err(refused)?)
                }

                rpc::method::PACKAGE_UNINSTALL => {
                    let target: PackageTarget = arguments(params)?;
                    api.not_being_upgraded(&target.package, &target.version)
                        .await
                        .map_err(refused)?;
                    encode_result(&api.packages.uninstall(&target).await.map_err(refused)?)
                }

                rpc::method::PACKAGE_UPGRADE_PLAN => {
                    let query: PackageUpgradeQuery = arguments(params)?;
                    encode_result(&api.package_upgrade_plan(&query).await.map_err(refused)?)
                }

                rpc::method::PACKAGE_UPGRADE => {
                    let asked: PackageUpgrade = arguments(params)?;
                    encode_result(&api.package_upgrade(asked).await.map_err(refused)?)
                }

                rpc::method::PROJECT_CREATE => {
                    let create: ProjectCreate = arguments(params)?;
                    encode_result(&api.projects.create(&create).await.map_err(refused)?)
                }

                rpc::method::PROJECT_LIST => {
                    no_params(params.as_ref())?;
                    encode_result(&api.projects.list().await.map_err(refused)?)
                }

                rpc::method::PROJECT_SHOW => {
                    let query: ProjectQuery = arguments(params)?;
                    encode_result(&api.projects.show(&query).await.map_err(refused)?)
                }

                rpc::method::PROJECT_UPDATE => {
                    let update: ProjectUpdate = arguments(params)?;
                    encode_result(&api.projects.update(&update).await.map_err(refused)?)
                }

                rpc::method::PROJECT_DELETE => {
                    let query: ProjectQuery = arguments(params)?;
                    encode_result(&api.projects.delete(&query).await.map_err(refused)?)
                }

                rpc::method::PROJECT_EXPORT => {
                    let query: ProjectQuery = arguments(params)?;
                    encode_result(&api.projects.export(&query).await.map_err(refused)?)
                }

                rpc::method::SITE_CREATE => {
                    let create: SiteCreate = arguments(params)?;
                    encode_result(&api.sites.create(&create).await.map_err(refused)?)
                }

                // `arguments` already answers "nothing" with an empty object, and every field of
                // `SiteListQuery` has a default — so `mix site list` with no project named is a
                // whole request without a second helper beside it.
                rpc::method::SITE_LIST => {
                    let query: SiteListQuery = arguments(params)?;
                    encode_result(&api.sites.list(&query).await.map_err(refused)?)
                }

                rpc::method::SITE_SHOW => {
                    let query: SiteQuery = arguments(params)?;
                    encode_result(&api.sites.show(&query).await.map_err(refused)?)
                }

                rpc::method::SITE_UPDATE => {
                    let update: SiteUpdate = arguments(params)?;
                    encode_result(&api.sites.update(&update).await.map_err(refused)?)
                }

                rpc::method::SITE_SHARE => {
                    let request: SiteShare = arguments(params)?;
                    encode_result(
                        &api.sites
                            .share(
                                &request.site,
                                request.interface.as_deref(),
                                request.for_seconds,
                                mixengine_proto::Timestamp::from_system_time(
                                    std::time::SystemTime::now(),
                                ),
                            )
                            .await
                            .map_err(refused)?,
                    )
                }

                rpc::method::SITE_UNSHARE => {
                    let query: SiteQuery = arguments(params)?;
                    api.sites.unshare(&query.site).await.map_err(refused)?;
                    encode_result(&())
                }

                rpc::method::SITE_START => {
                    let query: SiteQuery = arguments(params)?;
                    encode_result(&api.sites.start(&query).await.map_err(refused)?)
                }

                rpc::method::SITE_STOP => {
                    let query: SiteQuery = arguments(params)?;
                    encode_result(&api.sites.stop(&query).await.map_err(refused)?)
                }

                rpc::method::SITE_DELETE => {
                    let query: SiteQuery = arguments(params)?;
                    encode_result(&api.sites.delete(&query).await.map_err(refused)?)
                }

                rpc::method::DAEMON_DOCTOR => encode_result(&api.doctor_report().await),

                rpc::method::BLUEPRINT_CAPTURE => {
                    let capture: BlueprintCapture = arguments(params)?;
                    encode_result(&api.blueprints.capture(&capture).await.map_err(refused)?)
                }

                rpc::method::BLUEPRINT_IMPORT => {
                    let asked: BlueprintImport = arguments(params)?;
                    encode_result(&api.blueprints.import(&asked).await.map_err(refused)?)
                }

                rpc::method::BLUEPRINT_LIST => {
                    no_params(params.as_ref())?;
                    encode_result(&api.blueprints.list().await.map_err(refused)?)
                }

                rpc::method::BLUEPRINT_APPLY => {
                    let apply: BlueprintApply = arguments(params)?;
                    encode_result(&api.blueprint_apply(&apply).await.map_err(refused)?)
                }

                rpc::method::EXTENSION_INSPECT => {
                    let asked: ExtensionInspect = arguments(params)?;
                    encode_result(&api.extensions.inspect(&asked).map_err(refused)?)
                }

                rpc::method::EXTENSION_LIST => {
                    no_params(params.as_ref())?;
                    encode_result(&api.extensions.list().await.map_err(refused)?)
                }

                rpc::method::EXTENSION_AVAILABLE => {
                    let asked: ExtensionAvailable = arguments(params)?;
                    encode_result(&api.extensions.available(&asked).await.map_err(refused)?)
                }

                rpc::method::EXTENSION_PLAN => {
                    let asked: ExtensionPlanRequest = arguments(params)?;
                    encode_result(&api.extensions.plan(&asked).await.map_err(refused)?)
                }

                rpc::method::EXTENSION_INSTALL => {
                    let asked: ExtensionInstall = arguments(params)?;
                    encode_result(&api.extensions.install(&asked).await.map_err(refused)?)
                }

                rpc::method::EXTENSION_UNINSTALL => {
                    let asked: ExtensionUninstall = arguments(params)?;
                    encode_result(&api.extensions.uninstall(&asked).await.map_err(refused)?)
                }

                // **The same walk `service.start` takes** — the T81 design's D11. An extension is
                // what somebody installed and its `ServiceId` is an implementation detail of that,
                // so this resolves the one to the other and adds no supervision of its own.
                rpc::method::EXTENSION_START => {
                    let asked: ExtensionTarget = arguments(params)?;
                    let service = api
                        .extensions
                        .service_of(&asked.id)
                        .await
                        .map_err(refused)?;

                    encode_result(
                        &api.service_start(&ServiceTarget {
                            service: Some(service),
                            project: None,
                            wait: true,
                        })
                        .await
                        .map_err(refused)?,
                    )
                }

                rpc::method::EXTENSION_STOP => {
                    let asked: ExtensionTarget = arguments(params)?;
                    let service = api
                        .extensions
                        .service_of(&asked.id)
                        .await
                        .map_err(refused)?;

                    encode_result(
                        &api.service_stop(&ServiceTarget {
                            service: Some(service),
                            project: None,
                            wait: true,
                        })
                        .await
                        .map_err(refused)?,
                    )
                }

                rpc::method::DAEMON_DOCTOR_REPAIR => {
                    let asked: DoctorRepair = arguments(params)?;
                    encode_result(&api.repairs.run(&asked).await)
                }

                // Through `arguments` rather than `no_params`: `deny_unknown_fields` is what refuses
                // a misspelled option, and an option that bounds what goes into an archive is one a
                // caller must not believe they set.
                rpc::method::DAEMON_BUNDLE => {
                    let _: DiagnosticsBundle = arguments(params)?;
                    encode_result(&api.bundle().await.map_err(refused)?)
                }

                // **The reference is resolved here rather than inside `Certificates`** — T50. That
                // is where `expect` already lives, and a `Certificates` able to resolve one would
                // have to hold the `Sites` that in turn holds it.
                rpc::method::CERT_ISSUE => {
                    let issue: CertIssue = arguments(params)?;

                    let site = match issue.site.as_ref() {
                        Some(reference) => {
                            Some(api.sites.expect(reference).await.map_err(refused)?.0)
                        }
                        None => None,
                    };

                    encode_result(&api.certificates.issue(site).await.map_err(refused)?)
                }

                // **The one route in this file that opens a socket** — roadmap task T53. It
                // asks this home's own front end what it is serving, which is the only question
                // here whose answer is not read out of a file or a row.
                rpc::method::CERT_STATUS => {
                    let query: CertStatusQuery = arguments(params)?;

                    let site = match query.site.as_ref() {
                        Some(reference) => {
                            Some(api.sites.expect(reference).await.map_err(refused)?.0)
                        }
                        None => None,
                    };

                    let port = api.services.front_end_tls_port().await;

                    encode_result(
                        &api.certificates
                            .site_status(site, port)
                            .await
                            .map_err(refused)?,
                    )
                }

                // Through `arguments` rather than `no_params`, for the reason above it:
                // `deny_unknown_fields` is what refuses a misspelled option instead of quietly
                // handing back the default.
                rpc::method::CERT_CA_STATUS => {
                    let _: CaStatusQuery = arguments(params)?;
                    encode_result(&api.ca_status().await.map_err(refused)?)
                }

                // **The two routes in this file that take something away** — roadmap task T54.
                // Jobs, because they wait for an elevation prompt, and a prompt is not something an
                // RPC can block on: the person may take a minute, or walk away.
                rpc::method::CERT_CA_ROTATE => {
                    let _: CaRotateQuery = arguments(params)?;

                    encode_result(
                        &crate::certs::authority::rotate(
                            api.certificates.clone(),
                            Arc::clone(&api.elevation),
                            Arc::clone(&api.services),
                            &api.jobs,
                        )
                        .await
                        .map_err(refused)?,
                    )
                }

                rpc::method::CERT_CA_UNINSTALL => {
                    let _: CaUninstallQuery = arguments(params)?;

                    encode_result(
                        &crate::certs::authority::uninstall(
                            api.certificates.clone(),
                            Arc::clone(&api.elevation),
                            &api.jobs,
                        )
                        .await
                        .map_err(refused)?,
                    )
                }

                rpc::method::DOMAIN_ADD => {
                    let add: DomainAdd = arguments(params)?;
                    encode_result(&api.domains.add(&add).await.map_err(refused)?)
                }

                rpc::method::DOMAIN_REMOVE => {
                    let remove: DomainRemove = arguments(params)?;
                    encode_result(&api.domains.remove(&remove).await.map_err(refused)?)
                }

                rpc::method::DOMAIN_DNS_STATUS => {
                    let query: DomainStatusQuery = arguments(params)?;
                    encode_result(&api.domains.status(&query).await.map_err(refused)?)
                }

                rpc::method::RUNTIME_UPGRADE => {
                    let asked: RuntimeUpgrade = arguments(params)?;
                    encode_result(&api.runtime_upgrade(asked).await.map_err(refused)?)
                }

                rpc::method::RUNTIME_UPGRADE_PLAN => {
                    let query: RuntimeUpgradeQuery = arguments(params)?;
                    encode_result(&api.runtime_upgrade_plan(&query).await.map_err(refused)?)
                }

                rpc::method::RUNTIME_RESOLVE => {
                    let question: RuntimeQuestion = arguments(params)?;
                    encode_result(&api.runtimes.resolve(&question).await.map_err(refused)?)
                }

                // The three that write a file in the user's home rather than one in ours. Each is a
                // blocking pile of filesystem and registry work, so each runs where blocking work
                // belongs — see [`on_a_blocking_thread`].
                rpc::method::PATH_STATUS => {
                    no_params(params.as_ref())?;
                    encode_result(&api.shims.status().await.map_err(refused)?)
                }

                rpc::method::PATH_INSTALL => {
                    no_params(params.as_ref())?;
                    // Not `on_a_blocking_thread` any more: filling `bin/` now reads the database
                    // first (T130), so the call is a future rather than a pile of file copies, and
                    // the copies themselves are what they always were — nineteen stats and, on the
                    // pass that changes anything, a hard link apiece.
                    encode_result(&api.shims.install().await.map_err(refused)?)
                }

                rpc::method::PATH_UNINSTALL => {
                    no_params(params.as_ref())?;
                    encode_result(&api.shims.uninstall().await.map_err(refused)?)
                }

                // T131: the pass the daemon runs when a runtime's bindir changes, run now.
                rpc::method::PATH_RESCAN => {
                    no_params(params.as_ref())?;
                    encode_result(&api.shims.rescanned().await.map_err(refused)?)
                }

                // The second capability that writes outside the home, beside `path.*` above and on
                // its rule: only ever when somebody asks. Blocking for those three's reason too — a
                // tool to run and a file to write.
                rpc::method::AUTOSTART_STATUS => {
                    no_params(params.as_ref())?;
                    let autostart = Arc::clone(&api.autostart);
                    encode_result(
                        &on_a_blocking_thread(move || autostart.status())
                            .await
                            .map_err(refused)?,
                    )
                }

                rpc::method::AUTOSTART_ENABLE => {
                    no_params(params.as_ref())?;
                    let autostart = Arc::clone(&api.autostart);
                    encode_result(
                        &on_a_blocking_thread(move || autostart.enable())
                            .await
                            .map_err(refused)?,
                    )
                }

                rpc::method::AUTOSTART_DISABLE => {
                    no_params(params.as_ref())?;
                    let autostart = Arc::clone(&api.autostart);
                    encode_result(
                        &on_a_blocking_thread(move || autostart.disable())
                            .await
                            .map_err(refused)?,
                    )
                }

                rpc::method::SERVICE_LIST => {
                    no_params(params.as_ref())?;
                    encode_result(&api.service_list().await.map_err(refused)?)
                }

                rpc::method::SERVICE_STATUS => {
                    let query: ServiceQuery = arguments(params)?;
                    encode_result(&api.service_status(&query.service).await.map_err(refused)?)
                }

                rpc::method::SERVICE_START => {
                    let target: ServiceTarget = arguments(params)?;
                    encode_result(&api.service_start(&target).await.map_err(refused)?)
                }

                rpc::method::SERVICE_STOP => {
                    let target: ServiceTarget = arguments(params)?;
                    encode_result(&api.service_stop(&target).await.map_err(refused)?)
                }

                rpc::method::SERVICE_RESTART => {
                    let target: ServiceTarget = arguments(params)?;
                    encode_result(&api.service_restart(&target).await.map_err(refused)?)
                }

                rpc::method::HOME_PREVIOUS => {
                    encode_result(&api.home_previous().await.map_err(refused)?)
                }

                rpc::method::HOME_RESTORE => {
                    encode_result(&api.home_restore().await.map_err(refused)?)
                }

                rpc::method::SERVICE_FOUND => {
                    encode_result(&api.service_found().await.map_err(refused)?)
                }

                rpc::method::SERVICE_ADOPT => {
                    let asked: mixengine_proto::ServiceAdopt = arguments(params)?;
                    encode_result(&api.service_adopt(&asked).await.map_err(refused)?)
                }

                rpc::method::SERVICE_RESET_CREDENTIAL => {
                    let reset: ResetCredential = arguments(params)?;
                    encode_result(
                        &api.service_reset_credential(&reset)
                            .await
                            .map_err(refused)?,
                    )
                }

                rpc::method::DATABASE_CREATE => {
                    let create: DatabaseCreate = arguments(params)?;
                    encode_result(&api.databases.create(&create).await.map_err(refused)?)
                }

                rpc::method::DATABASE_CLIENT => {
                    let asked: DatabaseClientQuery = arguments(params)?;
                    encode_result(&api.databases.client(&asked).await.map_err(refused)?)
                }

                rpc::method::DATABASE_OPEN => {
                    let asked: DatabaseOpen = arguments(params)?;
                    encode_result(&api.databases.open(&asked).await.map_err(refused)?)
                }

                rpc::method::DATABASE_CREDENTIALS => {
                    let asked: DatabaseCredentialsQuery = arguments(params)?;
                    encode_result(&api.databases.credentials(&asked).await.map_err(refused)?)
                }

                rpc::method::SERVICE_CREATE => {
                    let create: ServiceCreate = arguments(params)?;
                    encode_result(&api.service_create(&create).await.map_err(refused)?)
                }

                rpc::method::SERVICE_LIMITS => {
                    let target: ServiceTarget = arguments(params)?;
                    encode_result(&api.service_limits(&target).await.map_err(refused)?)
                }

                rpc::method::SERVICE_SET_LIMITS => {
                    let asked: ServiceLimitsSet = arguments(params)?;
                    encode_result(&api.service_set_limits(&asked).await.map_err(refused)?)
                }

                rpc::method::SERVICE_IDLE => {
                    let target: ServiceTarget = arguments(params)?;
                    encode_result(&api.service_idle(&target).await.map_err(refused)?)
                }

                rpc::method::SERVICE_SET_IDLE => {
                    let asked: ServiceIdleSet = arguments(params)?;
                    encode_result(&api.service_set_idle(&asked).await.map_err(refused)?)
                }

                rpc::method::SERVICE_SET_AUTOSTART => {
                    let asked: ServiceAutostartSet = arguments(params)?;
                    encode_result(&api.service_set_autostart(&asked).await.map_err(refused)?)
                }

                rpc::method::SERVICE_SAVE_RESOURCES => {
                    no_params(params.as_ref())?;
                    encode_result(&api.service_save_resources().await.map_err(refused)?)
                }

                rpc::method::SERVICE_SET_SAVE_RESOURCES => {
                    let asked: SaveResourcesSet = arguments(params)?;
                    encode_result(
                        &api.service_set_save_resources(asked)
                            .await
                            .map_err(refused)?,
                    )
                }

                rpc::method::SERVICE_SET_FRONT_END => {
                    let switch: FrontEndSwitch = arguments(params)?;
                    encode_result(&api.service_set_front_end(switch).await.map_err(refused)?)
                }

                rpc::method::SERVICE_DELETE => {
                    let asked: ServiceDelete = arguments(params)?;
                    encode_result(
                        &api.service_delete(&asked.target.service, asked.force)
                            .await
                            .map_err(refused)?,
                    )
                }

                rpc::method::JOB_LIST => {
                    let filter: JobFilter = arguments(params)?;
                    encode_result(&JobList {
                        jobs: api.jobs.list(&filter).await.map_err(refused)?,
                    })
                }

                rpc::method::JOB_STATUS => {
                    let query: JobQuery = arguments(params)?;
                    encode_result(&api.jobs.status(query.job).await.map_err(refused)?)
                }

                rpc::method::JOB_WAIT => {
                    let wait: JobWait = arguments(params)?;
                    encode_result(
                        &api.jobs
                            .wait(wait.job, wait.timeout)
                            .await
                            .map_err(refused)?,
                    )
                }

                rpc::method::METRICS_SNAPSHOT => {
                    no_params(params.as_ref())?;
                    encode_result(&api.metrics_snapshot().await.map_err(refused)?)
                }

                rpc::method::METRICS_HISTORY => {
                    let query: MetricsHistoryQuery = arguments(params)?;
                    encode_result(&api.metrics_history(&query).await.map_err(refused)?)
                }

                rpc::method::JOB_CANCEL => {
                    let query: JobQuery = arguments(params)?;
                    encode_result(&api.jobs.cancel(query.job).await.map_err(refused)?)
                }

                rpc::method::ELEVATION_STATUS => {
                    no_params(params.as_ref())?;
                    encode_result(&api.elevation.status().await.map_err(refused)?)
                }

                rpc::method::ELEVATION_GRANT => {
                    no_params(params.as_ref())?;
                    encode_result(&api.elevation.grant().await.map_err(refused)?)
                }

                rpc::method::ELEVATION_DROP => {
                    let asked: ElevationDrop = arguments(params)?;
                    encode_result(&api.elevation.drop_pending(&asked).await.map_err(refused)?)
                }

                // Not shipped, and the only way to prove the containment above does anything: a
                // handler that panics has to be a real handler, because catching a panic raised
                // anywhere else would prove something about the test and not about the dispatcher.
                #[cfg(test)]
                "daemon.__panic" => panic!("a handler that panics, on purpose"),

                unknown => Err(Failure {
                    code: RpcCode::METHOD_NOT_FOUND,
                    error: Error::new(
                        ErrorCode::NotFound,
                        format!("this daemon has no method `{unknown}`"),
                    )
                    .with_hint(
                        "the client and the daemon are probably from different releases; \
                         `daemon.version` says which one this is",
                    ),
                }),
            }
        }
        // The span belongs to the request, and the task the work runs in is not the task the span
        // was opened on, so it is attached explicitly rather than inherited.
        .in_current_span(),
    )
    .await;

    match outcome {
        Ok(Ok(result)) => {
            tracing::debug!("answered");
            Ok(result)
        }

        Ok(Err(failure)) => {
            // `warn` and not `error`: the caller has been told, in a message written for them, and
            // most of what lands here is a request that was simply wrong. The line exists so the
            // daemon's log can answer "what was this client doing" later.
            tracing::warn!(code = %failure.error.code, error = %failure.error.message, "refused");
            Err(failure)
        }

        Err(join) => {
            // The panic message itself has already gone to the log through the panic hook — which
            // is `crate::crash`'s, installed at `main`, and which also left a report in
            // `logs/crashes/`. Until T91 built it there was no hook at all and this sentence was
            // true of nothing: the default one writes to stderr, and a `--detach`ed daemon's stderr
            // is the null device. It is still not repeated to the client — a backtrace is not
            // something a user can act on, and the daemon's log is where it belongs.
            tracing::error!(
                panicked = join.is_panic(),
                "a request handler did not finish; answering `internal` and staying up"
            );

            Err(Failure {
                code: RpcCode::INTERNAL_ERROR,
                error: Error::new(
                    ErrorCode::Internal,
                    format!("`{named}` failed in a way it does not account for"),
                )
                .with_hint(
                    "this is a bug in MixEngine; `logs/daemon.log` has the detail a report needs",
                ),
            })
        }
    }
}

/// Run a handler's blocking half where blocking work belongs.
///
/// `docs/standards/rust.md` keeps anything that waits on a disk off the runtime threads that
/// have connections to serve, and `path.*` is the first *method* here with such a half: nineteen
/// file copies, a directory walk, and on Windows a registry write that broadcasts to every window
/// on the desktop. Every other handler answers from memory or through `sqlx`, which does its own.
///
/// **A panic inside is re-raised rather than turned into an error**, so that the containment in
/// [`call_method`] is the one place a panicking handler is described — a second rendering of the
/// same accident here would be a second sentence for the same bug, differing only in which thread
/// it happened on.
pub(crate) async fn on_a_blocking_thread<T, F>(work: F) -> Result<T, Error>
where
    F: FnOnce() -> Result<T, Error> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(answer) => answer,
        Err(join) => match join.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            // Cancellation, which only happens when the runtime is going down — and a blocking task
            // is not cancellable, so this is unreachable while the daemon is the only thing
            // spawning them. Reported as itself rather than unwrapped, because nothing here panics.
            Err(_) => Err(Error::new(
                ErrorCode::Internal,
                "the work behind this call did not finish".to_owned(),
            )),
        },
    }
}

/// A method that takes no arguments, checking that it was called that way.
///
/// Lenient about how "nothing" is spelled: absent, `null`, `[]` and `{}` all mean the same thing,
/// and clients differ on which they send. Anything else is refused, because a client that passed
/// arguments believes they did something.
fn no_params(params: Option<&Value>) -> Result<(), Failure> {
    let empty = match params {
        None | Some(Value::Null) => true,
        Some(Value::Array(items)) => items.is_empty(),
        Some(Value::Object(fields)) => fields.is_empty(),
        Some(_) => false,
    };

    if empty {
        Ok(())
    } else {
        Err(Failure {
            code: RpcCode::INVALID_PARAMS,
            error: Error::new(
                ErrorCode::InvalidArgument,
                "this method takes no parameters",
            ),
        })
    }
}

/// A method's arguments, decoded into the shape that method documents.
///
/// Lenient about "nothing" in the same three spellings [`no_params`] accepts, because a method whose
/// every parameter has a default — `service.start` with no service named is every service — should
/// answer a client that sent `{}`, `null` or nothing at all identically. A type with a required
/// field still refuses all three, which is the point: `service.status` with no subject is a
/// `service.list` that was typed wrongly, and reporting it is better than answering it.
fn arguments<T: serde::de::DeserializeOwned>(params: Option<Value>) -> Result<T, Failure> {
    let given = match params {
        None | Some(Value::Null) => Value::Object(serde_json::Map::new()),
        Some(Value::Array(items)) if items.is_empty() => Value::Object(serde_json::Map::new()),
        Some(value) => value,
    };

    serde_json::from_value(given).map_err(|error| Failure {
        code: RpcCode::INVALID_PARAMS,
        error: Error::new(
            ErrorCode::InvalidArgument,
            format!("these are not this method's parameters: {error}"),
        ),
    })
}

/// A failure MixEngine itself produced: the method ran, and the work did not.
///
/// Everything that reaches this has already been through [`crate::error::ToWire`], where the code
/// and the hint are chosen; all that is added here is the JSON-RPC integer that says the call got as
/// far as running.
fn refused(error: Error) -> Failure {
    Failure {
        code: RpcCode::APPLICATION_ERROR,
        error,
    }
}

/// A handler's return value, as JSON.
///
/// Serialising a type we defined can only fail on something like a map with non-string keys, which
/// no `mixengine-proto` type has — but it is a `Result` all the same rather than an `unwrap`, on
/// the same principle as the panic containment above: nothing in the RPC layer panics.
fn encode_result<T: serde::Serialize>(value: &T) -> Result<Value, Failure> {
    serde_json::to_value(value).map_err(|error| Failure {
        code: RpcCode::INTERNAL_ERROR,
        error: Error::new(
            ErrorCode::Internal,
            format!("the answer could not be encoded: {error}"),
        ),
    })
}

/// A failed call, before it is either dropped (a notification) or written into a [`Response`].
///
/// Carries the JSON-RPC integer *and* the MixEngine error because the two are chosen at different
/// moments: the integer says at which stage the call stopped, the error says what a person should
/// do about it.
#[derive(Debug)]
struct Failure {
    code: RpcCode,
    error: Error,
}

impl Failure {
    fn into_rpc(self) -> RpcError {
        RpcError::at(self.code, self.error)
    }
}

impl Api {
    /// `metrics.snapshot` — every subject's reading, taken now.
    ///
    /// **A reading rather than the last one taken**, which is what the method's own documentation
    /// promises: between clients this daemon samples once a minute, so the cached tick would answer
    /// a person with a number up to a minute old and would not mention a service that started ten
    /// seconds ago. Reuse of a reading younger than a second happens inside the loop, which is the
    /// only thing that measures.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Internal`] when the sampling loop is gone, which happens only while this daemon
    /// is shutting down — and is worth saying rather than answering with an empty frame that would
    /// read as a machine using nothing.
    async fn metrics_snapshot(&self) -> Result<MetricsFrame, Error> {
        self.metrics.snapshot().await.ok_or_else(|| {
            Error::new(
                ErrorCode::Internal,
                "this daemon is no longer measuring anything",
            )
            .with_hint("it is shutting down; ask the one that starts next")
        })
    }

    /// `metrics.history` — the 24-hour history, one row per subject per minute.
    ///
    /// # Errors
    ///
    /// Whatever [`mixengine_core::metrics::history`] reports when the table cannot be read.
    async fn metrics_history(&self, query: &MetricsHistoryQuery) -> Result<MetricsHistory, Error> {
        // The machine's logical processors ride along (T190c), so a client draws the rows as a
        // share of the machine without asking a second question.
        let cores = self.elevation.host().resource_control().support().cores;

        mixengine_core::metrics::history(&self.store, query, self.metrics.retention_hours(), cores)
            .await
            .map_err(|error| error.to_wire())
    }
}

/// Serialise an answer.
///
/// The failure case is unreachable — every type in it is one of ours — and is answered with a
/// hand-written JSON-RPC error rather than an `unwrap`, so that the invariant being wrong costs one
/// bad response instead of the process.
fn encode<T: serde::Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_else(|_| {
        br#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"the answer could not be encoded","data":{"code":"internal"}},"id":null}"#.to_vec()
    })
}

#[cfg(test)]
mod tests {
    use mixengine_proto::rpc::ResponseOutcome;
    use mixengine_proto::{
        AutostartReport, JobSummary, Millis, PathReport, ServiceId, ServiceSpec, StopBehaviour,
    };

    use super::*;
    use crate::api::test_daemon::{Daemon, answer_json, call, daemon, daemon_on, undeclared};
    use crate::services::fixture;

    /// Whether a response says the call succeeded.
    fn succeeded(response: &Response) -> bool {
        matches!(response.outcome, ResponseOutcome::Success { .. })
    }

    #[tokio::test]
    async fn a_history_read_answers_with_what_this_home_keeps() {
        let answer = call(r#"{"jsonrpc":"2.0","method":"metrics.history","id":1}"#).await;

        assert_eq!(answer["result"]["retention_hours"], 24);
        assert_eq!(
            answer["result"]["minutes"].as_array().map(Vec::len),
            Some(0),
            "a home that has measured nothing has no rows, which is an answer and not a failure"
        );
    }

    #[tokio::test]
    async fn a_history_read_refuses_a_subject_it_cannot_read() {
        let answer = call(
            r#"{"jsonrpc":"2.0","method":"metrics.history","params":{"subject":"nonsense"},"id":1}"#,
        )
        .await;

        assert_eq!(answer["error"]["code"], -32602);
        assert_eq!(answer["error"]["data"]["code"], "invalid_argument");
    }

    #[tokio::test]
    async fn a_snapshot_with_no_loop_behind_it_says_so_rather_than_waiting() {
        // The fixture builds a sampler and keeps only its handle, which is the shape a daemon has
        // while it is shutting down. The answer has to be an error: an empty frame would read as a
        // machine using nothing.
        let answer = call(r#"{"jsonrpc":"2.0","method":"metrics.snapshot","id":1}"#).await;

        assert_eq!(answer["error"]["data"]["code"], "internal");
        assert!(
            answer["error"]["data"]["hint"]
                .as_str()
                .is_some_and(|hint| hint.contains("shutting down"))
        );
    }

    #[tokio::test]
    async fn a_string_id_comes_back_as_the_same_string() {
        let answer = call(r#"{"jsonrpc":"2.0","method":"daemon.version","id":"handshake"}"#).await;

        assert_eq!(answer["id"], "handshake");
    }

    /// The three `path.*` methods over the dispatcher — roadmap task **T26**.
    ///
    /// Against `mock::Host`, which is what makes this runnable at all: the real implementations
    /// write a registry value or a file in the person's own home, and a suite that exercised them
    /// would be a `cargo test` that edits the PATH of whoever ran it. The two real ones have their
    /// own tests inside `mixengine-platform`, against a key and a home they create themselves.
    #[tokio::test]
    async fn the_path_is_reported_then_taken_and_then_given_back() {
        let daemon = undeclared().await;

        let before: PathReport = daemon.expect(rpc::method::PATH_STATUS, Value::Null).await;
        assert!(!before.on_path, "{before:?}");
        assert!(before.directory.ends_with("bin"), "{before:?}");

        // The status did not fill `bin/`, and says so rather than listing the table.
        assert!(before.commands.is_empty(), "{before:?}");

        let installed: PathReport = daemon.expect(rpc::method::PATH_INSTALL, Value::Null).await;
        assert!(installed.on_path);
        assert!(installed.places.iter().all(|place| place.changed));
        // T185b: this home has no runtime installed, so `bin/` fronts none of the compiled names —
        // a `node` there would hide a Node the person installed themselves.
        assert!(installed.commands.is_empty(), "{installed:?}");
        assert!(installed.stale.is_empty());

        // Idempotent, and it says which of the two it was — a client that reported a write it did
        // not perform would be indistinguishable from one that did.
        let again: PathReport = daemon.expect(rpc::method::PATH_INSTALL, Value::Null).await;
        assert!(again.on_path);
        assert!(again.places.iter().all(|place| !place.changed), "{again:?}");

        let removed: PathReport = daemon
            .expect(rpc::method::PATH_UNINSTALL, Value::Null)
            .await;
        assert!(!removed.on_path);

        // The shims stay: removing the home is what removes them.
        assert_eq!(removed.commands, installed.commands);

        // Two installs and an uninstall. The status is absent, which is the point: a read is not a
        // mutation, and the mock records only what changed the machine or tried to.
        assert_eq!(daemon.host.path_operations().len(), 3);

        daemon.quiet().await;
    }

    /// The three `autostart.*` methods over the dispatcher — roadmap task **T85b**.
    ///
    /// Against `mock::Host`, for the `path.*` test's reason one step louder: the real
    /// implementations register a logon task, a LaunchAgent or a systemd user unit for the account
    /// running them, and a suite that exercised them would be a `cargo test` that arranges for a
    /// daemon to start at every login of whoever ran it. The three real ones have their own tests
    /// inside `mixengine-platform`, against a scratch name and a directory they create themselves.
    #[tokio::test]
    async fn the_autostart_entry_is_reported_then_registered_and_then_taken_away() {
        let daemon = undeclared().await;

        let before: AutostartReport = daemon
            .expect(rpc::method::AUTOSTART_STATUS, Value::Null)
            .await;
        assert!(!before.enabled, "{before:?}");
        assert!(!before.changed, "a status never claims a write: {before:?}");
        assert!(!before.for_this_home, "{before:?}");
        assert!(before.command.is_empty(), "{before:?}");

        let enabled: AutostartReport = daemon
            .expect(rpc::method::AUTOSTART_ENABLE, Value::Null)
            .await;
        assert!(enabled.enabled && enabled.changed, "{enabled:?}");
        assert!(
            enabled.for_this_home,
            "the entry names this daemon's own home: {enabled:?}"
        );
        assert!(
            enabled.command.iter().any(|word| word == "--home"),
            "{enabled:?}"
        );

        // Idempotent, and it says which of the two it was — the `path.*` distinction, and for its
        // reason.
        let again: AutostartReport = daemon
            .expect(rpc::method::AUTOSTART_ENABLE, Value::Null)
            .await;
        assert!(again.enabled, "{again:?}");
        assert!(!again.changed, "the second enable wrote: {again:?}");

        let disabled: AutostartReport = daemon
            .expect(rpc::method::AUTOSTART_DISABLE, Value::Null)
            .await;
        assert!(!disabled.enabled && disabled.changed, "{disabled:?}");
        assert!(disabled.command.is_empty(), "{disabled:?}");

        // Two enables and a disable. The status is absent for the `path.*` recorder's reason: a
        // read is not a mutation.
        assert_eq!(daemon.host.autostart_operations().len(), 3);

        daemon.quiet().await;
    }

    /// None of the three takes parameters, and a client that sent some is told so.
    #[tokio::test]
    async fn an_autostart_call_with_parameters_is_refused() {
        let answer = call(
            r#"{"jsonrpc":"2.0","method":"autostart.status","params":{"home":"/tmp"},"id":1}"#,
        )
        .await;

        assert_eq!(answer["error"]["data"]["code"], "invalid_argument");
    }

    #[tokio::test]
    async fn an_unknown_method_is_method_not_found_and_says_which_one() {
        // A namespace this build has not reached — `site.create` used to stand here until T39a,
        // `blueprint.apply` until T77 and `extension.install` until T81. Each of them becoming a
        // real method is exactly the drift this test is worth keeping past; what stands here now is
        // a method nobody has written.
        let answer = call(r#"{"jsonrpc":"2.0","method":"extension.open","id":1}"#).await;

        assert_eq!(answer["error"]["code"], -32601);
        assert_eq!(answer["error"]["data"]["code"], "not_found");
        assert!(
            answer["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("extension.open")),
            "{answer}"
        );
    }

    #[tokio::test]
    async fn parameters_to_a_method_that_takes_none_are_refused() {
        let answer =
            call(r#"{"jsonrpc":"2.0","method":"daemon.status","params":{"verbose":true},"id":1}"#)
                .await;

        assert_eq!(answer["error"]["code"], -32602);
        assert_eq!(answer["error"]["data"]["code"], "invalid_argument");
    }

    #[tokio::test]
    async fn the_four_ways_of_spelling_no_parameters_are_all_accepted() {
        for params in [
            r#","params":null"#,
            r#","params":[]"#,
            r#","params":{}"#,
            "",
        ] {
            let body = format!(r#"{{"jsonrpc":"2.0","method":"daemon.version"{params},"id":1}}"#);
            let answer = call(&body).await;

            assert!(answer.get("result").is_some(), "{params}: {answer}");
        }
    }

    #[tokio::test]
    async fn a_body_that_is_not_json_is_a_parse_error_with_a_null_id() {
        let answer = call("not json at all").await;

        assert_eq!(answer["error"]["code"], -32700);
        assert!(answer["id"].is_null(), "{answer}");
    }

    #[tokio::test]
    async fn a_request_that_is_not_one_keeps_the_id_it_claimed() {
        // No `method`, so it never becomes a `Request` — but a client waiting on id 9 still has to
        // be told that id 9 is the one that was wrong.
        let answer = call(r#"{"jsonrpc":"2.0","id":9}"#).await;

        assert_eq!(answer["error"]["code"], -32600);
        assert_eq!(answer["id"], 9);
    }

    #[tokio::test]
    async fn a_notification_is_answered_with_silence() {
        assert!(
            answer(
                &undeclared().await.api,
                br#"{"jsonrpc":"2.0","method":"daemon.status"}"#
            )
            .await
            .is_none()
        );
    }

    #[tokio::test]
    async fn a_null_id_is_answered_rather_than_mistaken_for_a_notification() {
        // The spec discourages a null id and nowhere lets it mean silence: a request carrying one
        // is still a request, and a client that sent it is still waiting. Answered to the id it
        // gave, which is `null`.
        let answer = call(r#"{"jsonrpc":"2.0","method":"daemon.version","id":null}"#).await;

        assert_eq!(answer["result"]["protocol"], 1);
        assert!(answer["id"].is_null(), "{answer}");
    }

    #[tokio::test]
    async fn a_notification_that_fails_is_still_answered_with_silence() {
        assert!(
            answer(
                &undeclared().await.api,
                br#"{"jsonrpc":"2.0","method":"nope.nope"}"#
            )
            .await
            .is_none()
        );
    }

    #[tokio::test]
    async fn a_batch_answers_every_call_that_asked_for_one() {
        let answered = answer(
            &undeclared().await.api,
            br#"[
                {"jsonrpc":"2.0","method":"daemon.version","id":1},
                {"jsonrpc":"2.0","method":"daemon.status"},
                {"jsonrpc":"2.0","method":"nope.nope","id":3}
            ]"#,
        )
        .await
        .expect("two of the three asked for an answer");

        let answers: Vec<Response> = serde_json::from_slice(&answered).expect("an array of them");

        assert_eq!(answers.len(), 2, "the notification is not answered");
        assert!(succeeded(&answers[0]));
        assert!(!succeeded(&answers[1]));
        assert_eq!(answers[1].id, Some(Id::Number(3)));
    }

    #[tokio::test]
    async fn a_batch_of_nothing_but_notifications_answers_nothing_at_all() {
        assert!(
            answer(
                &undeclared().await.api,
                br#"[{"jsonrpc":"2.0","method":"daemon.status"},{"jsonrpc":"2.0","method":"daemon.version"}]"#
            )
            .await
            .is_none()
        );
    }

    #[tokio::test]
    async fn an_empty_batch_is_an_invalid_request_rather_than_an_empty_array() {
        let answer = call("[]").await;

        assert_eq!(answer["error"]["code"], -32600);
    }

    #[tokio::test]
    async fn a_batch_element_that_is_not_an_object_fails_on_its_own() {
        let answered = answer(
            &undeclared().await.api,
            br#"[1, {"jsonrpc":"2.0","method":"daemon.version","id":2}]"#,
        )
        .await
        .expect("both elements produce an answer");

        let answers: Vec<Response> = serde_json::from_slice(&answered).unwrap();

        assert_eq!(answers.len(), 2);
        assert!(!succeeded(&answers[0]));
        assert_eq!(answers[0].id, None, "a bare number has no id to echo");
        assert!(succeeded(&answers[1]), "the sound call is still answered");
    }

    #[tokio::test]
    async fn a_handler_that_panics_answers_internal_and_leaves_the_daemon_up() {
        let api = undeclared().await.api;

        let answered = answer(
            &api,
            br#"{"jsonrpc":"2.0","method":"daemon.__panic","id":1}"#,
        )
        .await
        .expect("a panic is still an answer");
        let answer: Value = serde_json::from_slice(&answered).unwrap();

        assert_eq!(answer["error"]["code"], -32603);
        assert_eq!(answer["error"]["data"]["code"], "internal");
        // The panic message is not handed to the client — a backtrace is not something a user can
        // act on, and `logs/daemon.log` already has it.
        assert!(
            !answer["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("on purpose"),
            "{answer}"
        );

        // The whole point: the next request is answered normally.
        let after = answer_json(
            &api,
            r#"{"jsonrpc":"2.0","method":"daemon.version","id":2}"#,
        )
        .await;
        assert_eq!(after["result"]["version"], "0.1.0");
    }

    #[tokio::test]
    async fn another_protocol_version_is_an_invalid_request() {
        let answer = call(r#"{"jsonrpc":"1.0","method":"daemon.status","id":1}"#).await;

        assert_eq!(answer["error"]["code"], -32600);
    }

    /// A home reached through one front end, declared and rowed.
    ///
    /// **Its program is the one inside the installed package**, unlike [`fixture::spec`]'s, and it
    /// has to be: on Linux the port-80 grant is an attribute of *that file*, so a fixture pointing
    /// at the fake service somewhere else would be a home whose front end could never be found to
    /// hold one.
    fn front_end(id: &str) -> Arc<fixture::Declared> {
        Arc::new(fixture::Declared(vec![
            ServiceSpec::builder(
                fixture::service(id),
                mixengine_core::generate::program(&packages_root(), id),
            )
            .cwd(std::env::temp_dir())
            .ready(mixengine_proto::ReadyCheck::LogPattern {
                regex: mixengine_testkit::service::READY_LINE.to_owned(),
                timeout: Millis::from_secs(20),
            })
            .restart(mixengine_proto::RestartPolicy::Never)
            .stop(StopBehaviour::Signal { grace: Millis(500) })
            .build()
            .expect("a usable spec"),
        ]))
    }

    /// Where the switch tests say a package was installed.
    ///
    /// **Absolute on all three systems, which `fixture::home`'s `/packages/x` is not.** That value
    /// is a path on Unix and a *relative* one on Windows — no drive letter — and these are the first
    /// tests to build a spec's program out of an install path rather than out of the fake service's
    /// own, which is where a spec refuses it. Nothing is ever run from here.
    fn packages_root() -> std::path::PathBuf {
        match cfg!(windows) {
            true => std::path::PathBuf::from(r"C:\packages\x"),
            false => std::path::PathBuf::from("/packages/x"),
        }
    }

    /// Put every installed package where [`packages_root`] says, and install `also` beside them.
    ///
    /// The `also` half is what a home looks like the moment after `mix package install nginx` and
    /// before anything has been created from it: a `packages` row that no service is an instance of.
    async fn installed(daemon: &Daemon, also: &[&str]) {
        let root = packages_root().display().to_string();

        for package in also {
            sqlx::query(
                "INSERT INTO packages (name, version, install_path, installed_at, source_url,
                                       sha256)
                 VALUES (?, '1.0.0', ?, '2026-09-07T00:00:00Z', 'https://example', 'ab')",
            )
            .bind(package)
            .bind(&root)
            .execute(daemon.api.store.pool())
            .await
            .expect("an installed package");
        }

        sqlx::query("UPDATE packages SET install_path = ?")
            .bind(&root)
            .execute(daemon.api.store.pool())
            .await
            .expect("a readable table");
    }

    /// The report a finished switch left behind, or the reason there is not one.
    async fn switched(daemon: &Daemon, params: Value) -> mixengine_proto::FrontEndReport {
        let started: JobSummary = serde_json::from_value(
            daemon.ask(rpc::method::SERVICE_SET_FRONT_END, params).await["result"].clone(),
        )
        .expect("a job");

        let finished = daemon
            .api
            .jobs
            .wait(started.id, Millis::from_secs(30))
            .await
            .expect("a job that ends");

        match finished.outcome {
            Some(mixengine_proto::JobOutcome::Succeeded { result }) => {
                serde_json::from_value(result).expect("a front-end report")
            }
            other => panic!("the switch did not finish: {other:?}"),
        }
    }

    /// **T97.** A switch installs nothing, so a server that is not here is a refusal with the
    /// command that would put it here — not a job that downloads forty megabytes.
    #[tokio::test]
    async fn a_switch_to_a_server_that_is_not_installed_is_refused_with_the_way_to_install_it() {
        let daemon = undeclared().await;

        let answer = daemon
            .ask(
                rpc::method::SERVICE_SET_FRONT_END,
                serde_json::json!({"server": "nginx"}),
            )
            .await;

        assert_eq!(answer["error"]["data"]["code"], "precondition_failed");
        assert!(
            answer["error"]["data"]["hint"]
                .as_str()
                .is_some_and(|hint| hint.contains("mix package install nginx")),
            "{answer}"
        );
    }

    /// **T97.** Asking for the server a home is already on is a question with an answer, and not a
    /// refusal: a switch clicked on the option that is already selected is an ordinary thing to
    /// happen, and refusing it would make a client suppress the call.
    #[tokio::test]
    async fn a_switch_to_the_server_a_home_is_already_on_moves_nothing() {
        let daemon = daemon(front_end("nginx"), &["nginx"]).await;
        installed(&daemon, &[]).await;

        let report = switched(&daemon, serde_json::json!({"server": "nginx"})).await;

        assert!(
            matches!(
                report.outcome,
                mixengine_proto::FrontEndOutcome::Unchanged {}
            ),
            "{report:?}"
        );
        assert_eq!(report.was, report.now);
        assert_eq!(
            report.was.as_ref().map(ServiceId::as_str),
            Some("nginx"),
            "and the home is still on it"
        );
    }

    /// **T97, and ADR 0026's hardest sentence.** A machine that has granted port 80 to the front end
    /// this home is on and not to the one it is being asked to move to **stays where it was**.
    ///
    /// The outcome a switch must never produce is a home whose sites are rendered for a server that
    /// cannot answer, so this is refused before anything is stopped — and the grant it asked for is
    /// left waiting, which is what makes `mix elevation grant` and the same command again work.
    #[tokio::test]
    async fn a_machine_that_will_not_grant_the_new_front_end_keeps_the_one_it_had() {
        let daemon = daemon_on(front_end("nginx"), &["nginx"], |home| {
            mixengine_platform::mock::Host::with_port_access_for(
                home,
                mixengine_platform::PortAccessMethod::Capability,
                "nginx",
            )
        })
        .await;
        installed(&daemon, &["caddy"]).await;

        let report = switched(&daemon, serde_json::json!({"server": "caddy"})).await;

        assert!(
            matches!(
                report.outcome,
                mixengine_proto::FrontEndOutcome::NotGranted { .. }
            ),
            "{report:?}"
        );
        assert_eq!(
            report.now.as_ref().map(ServiceId::as_str),
            Some("nginx"),
            "the home did not move"
        );
        assert!(
            report.answering,
            "and what it is on can still answer, which is what was being protected"
        );
        assert_eq!(
            mixengine_core::services::front_end::held_by(
                &daemon.api.store,
                &crate::services::catalogue()
            )
            .await
            .expect("a readable table"),
            Some("nginx".to_owned()),
            "the row is untouched"
        );
        assert_eq!(
            daemon
                .api
                .elevation
                .status()
                .await
                .expect("a readable queue")
                .pending
                .len(),
            1,
            "and the grant it asked for is waiting, so allowing it and asking again works"
        );
    }

    /// **T97.** The rule is *do not make it worse*, not *require a grant*.
    ///
    /// A Linux home where nobody ever granted has a front end that cannot bind 80 and therefore does
    /// not start. Refusing to move it would trap somebody on a server that cannot answer in order to
    /// protect them from a server that cannot answer — so this switch goes ahead, and the report
    /// says the home still cannot answer rather than pretending the switch fixed it.
    #[tokio::test]
    async fn a_switch_from_a_front_end_that_cannot_answer_either_goes_ahead_and_says_so() {
        let daemon = daemon_on(front_end("nginx"), &["nginx"], |home| {
            mixengine_platform::mock::Host::without_port_access(
                home,
                mixengine_platform::PortAccessMethod::Capability,
                "nobody has granted anything on this machine",
            )
        })
        .await;
        installed(&daemon, &["caddy"]).await;

        let report = switched(&daemon, serde_json::json!({"server": "caddy"})).await;

        assert!(
            matches!(
                report.outcome,
                mixengine_proto::FrontEndOutcome::Switched { .. }
            ),
            "{report:?}"
        );
        assert_eq!(report.now.as_ref().map(ServiceId::as_str), Some("caddy"));
        assert!(
            !report.answering,
            "and it is still a home whose front end cannot bind 80, which is the truth"
        );
    }

    /// **T97.** The row moves, what is about the home's front end travels with it, and what belonged
    /// to the program that is going is named rather than silently dropped.
    #[tokio::test]
    async fn a_switch_carries_the_home_s_own_settings_and_names_what_it_left() {
        let daemon = daemon(front_end("nginx"), &["nginx"]).await;

        sqlx::query(
            "UPDATE services
                SET autostart = 1, bind_addr = '0.0.0.0',
                    config_overrides_json = '{\"worker_processes\":4}'
              WHERE id = 'nginx'",
        )
        .execute(daemon.api.store.pool())
        .await
        .expect("a front end somebody had configured");
        installed(&daemon, &["caddy"]).await;

        let report = switched(&daemon, serde_json::json!({"server": "caddy"})).await;

        assert_eq!(report.was.as_ref().map(ServiceId::as_str), Some("nginx"));
        assert_eq!(report.now.as_ref().map(ServiceId::as_str), Some("caddy"));
        assert_eq!(
            mixengine_core::services::front_end::held_by(
                &daemon.api.store,
                &crate::services::catalogue()
            )
            .await
            .expect("a readable table"),
            Some("caddy".to_owned()),
            "exactly one front end, and it is the new one"
        );

        let moved =
            mixengine_core::services::declaration(&daemon.api.store, &fixture::service("caddy"))
                .await
                .expect("the new row");

        assert!(moved.autostart, "starting with the daemon is about the job");
        assert_eq!(
            moved.bind_addr.as_deref(),
            Some("0.0.0.0"),
            "and so is the address this home's front end answers on"
        );
        assert!(
            crate::api::front_end::is_empty_document(&moved.overrides),
            "an nginx.conf setting is a syntax error in a Caddyfile, so it does not travel"
        );
        assert!(
            report
                .not_carried
                .iter()
                .any(|left| left.contains("overriding")),
            "and what did not travel is named: {:?}",
            report.not_carried
        );
    }

    /// **T97.** The lock is taken for the one role that has an invariant spanning rows, and for
    /// nothing else.
    ///
    /// A home may have as many databases as it likes and they contend for nothing here; a home may
    /// have one front end, and the window in which that is momentarily untrue is what this closes.
    #[tokio::test]
    async fn only_a_front_end_change_waits_for_another_one() {
        let daemon = undeclared().await;
        let catalogue = crate::services::catalogue();

        let held = daemon
            .api
            .one_front_end_change_at_a_time(&catalogue, &fixture::service("nginx"))
            .await;

        assert!(held.is_some(), "a front end takes the lock");
        assert!(
            daemon.api.front_end.try_lock().is_err(),
            "and holds it, which is the whole of what it is for"
        );

        assert!(
            daemon
                .api
                .one_front_end_change_at_a_time(&catalogue, &fixture::service("mariadb@main"))
                .await
                .is_none(),
            "a database goes straight past it, even while a front end holds it"
        );
        assert!(
            daemon
                .api
                .one_front_end_change_at_a_time(&catalogue, &fixture::service("not-a-package"))
                .await
                .is_none(),
            "and so does a package this build has no recipe for"
        );
    }

    /// A service with no database vocabulary is told so **in those words** — roadmap task T77a.
    ///
    /// Deliberately not `unsupported`: that code means *this operating system cannot*, and every
    /// system this ships to runs the fixture service perfectly well. What has no databases is the
    /// package. The same distinction T77 drew for `blueprint.apply`.
    #[tokio::test]
    async fn creating_a_database_on_a_service_that_has_none_is_refused_by_name() {
        let daemon = daemon(
            Arc::new(fixture::Declared(vec![
                fixture::spec("db").build().expect("a usable spec"),
            ])),
            &["db"],
        )
        .await;

        let answer = daemon
            .ask(
                rpc::method::DATABASE_CREATE,
                serde_json::json!({"service": "db", "database": "blog"}),
            )
            .await;

        assert_eq!(
            answer["error"]["data"]["code"], "invalid_argument",
            "{answer}"
        );
        assert!(
            answer["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("db"),
            "{answer}"
        );

        daemon.quiet().await;
    }

    /// A service this home does not declare is a different miss, and says so: "no such service"
    /// would send somebody looking for a service that is right there, and this is the case where it
    /// genuinely is not.
    #[tokio::test]
    async fn creating_a_database_on_a_service_nothing_declares_is_not_found() {
        let daemon = daemon(Arc::new(fixture::Declared(Vec::new())), &[]).await;

        let answer = daemon
            .ask(
                rpc::method::DATABASE_CREATE,
                serde_json::json!({"service": "mariadb@main", "database": "blog"}),
            )
            .await;

        assert_eq!(answer["error"]["data"]["code"], "not_found", "{answer}");

        daemon.quiet().await;
    }

    /// `database.client`, `database.open` and `database.credentials` reach their handler, and a
    /// service nothing declares is the same miss to all three — roadmap tasks **T83** and **T77b**.
    /// What the methods *do* is proved beside them, in `crate::databases`, on a mock host that
    /// records what it was asked to start.
    #[tokio::test]
    async fn asking_where_a_service_nothing_declares_could_be_opened_is_not_found() {
        let daemon = daemon(Arc::new(fixture::Declared(Vec::new())), &[]).await;

        for method in [
            rpc::method::DATABASE_CLIENT,
            rpc::method::DATABASE_OPEN,
            rpc::method::DATABASE_CREDENTIALS,
        ] {
            let answer = daemon
                .ask(method, serde_json::json!({"service": "mariadb@main"}))
                .await;

            assert_eq!(answer["error"]["data"]["code"], "not_found", "{answer}");
        }

        daemon.quiet().await;
    }

    /// A name that could not be a database name is refused **before** anything is started: a
    /// caller's typo should not first cost a database server coming up.
    #[tokio::test]
    async fn a_database_name_that_is_not_one_is_refused_before_anything_starts() {
        let daemon = daemon(Arc::new(fixture::Declared(Vec::new())), &[]).await;

        let answer = daemon
            .ask(
                rpc::method::DATABASE_CREATE,
                serde_json::json!({"service": "mariadb@main", "database": "Blog; DROP"}),
            )
            .await;

        assert_eq!(
            answer["error"]["data"]["code"], "invalid_argument",
            "{answer}"
        );

        daemon.quiet().await;
    }
}
