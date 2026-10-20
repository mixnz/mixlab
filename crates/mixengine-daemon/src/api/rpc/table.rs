//! The routing table: one arm per daemon method, from the name a request carries to the handler
//! that answers it.

use std::sync::Arc;

use mixengine_proto::rpc;
use mixengine_proto::rpc::RpcCode;
use mixengine_proto::{
    BlueprintApply, BlueprintCapture, BlueprintImport, CaRotateQuery, CaStatusQuery,
    CaUninstallQuery, CertIssue, CertStatusQuery, CleanupQuery, DatabaseClientQuery,
    DatabaseCreate, DatabaseCredentialsQuery, DatabaseOpen, DiagnosticsBundle, DiskUsageQuery,
    DoctorRepair, DomainAdd, DomainRemove, DomainStatusQuery, ElevationDrop, Error, ErrorCode,
    ExtensionAvailable, ExtensionChoice, ExtensionInspect, ExtensionInstall, ExtensionPlanRequest,
    ExtensionTarget, ExtensionUninstall, FrontEndSwitch, JobFilter, JobList, JobQuery, JobWait,
    MetricsHistoryQuery, PackageFilter, PackageInstall, PackageTarget, PackageUpgrade,
    PackageUpgradeQuery, ProjectCreate, ProjectQuery, ProjectUpdate, ResetCredential,
    RuntimeFilter, RuntimeInstall, RuntimeQuestion, RuntimeTarget, RuntimeUninstall,
    RuntimeUpgrade, RuntimeUpgradeQuery, SaveResourcesSet, ServiceAutostartSet, ServiceCreate,
    ServiceDelete, ServiceIdleSet, ServiceLimitsSet, ServiceQuery, ServiceTarget, SiteCreate,
    SiteListQuery, SiteQuery, SiteShare, SiteUpdate, UninstallQuery, UpdateApply, UpdateCheck,
    UpdateDecide, UpdateFinish, UpdateHandOver,
};
use serde_json::Value;

use super::super::Api;
use super::{Failure, arguments, encode_result, no_params, refused};

/// Answer one call of `method` with `params`, or say why it was not answered.
pub(super) async fn route(
    api: Arc<Api>,
    method: &str,
    params: Option<Value>,
) -> Result<Value, Failure> {
    match method {
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

        rpc::method::RUNTIME_FOUND => encode_result(&api.runtimes.found().await.map_err(refused)?),

        rpc::method::PACKAGE_FOUND => encode_result(&api.packages.found().await.map_err(refused)?),

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
                        mixengine_proto::Timestamp::from_system_time(std::time::SystemTime::now()),
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
                Some(reference) => Some(api.sites.expect(reference).await.map_err(refused)?.0),
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
                Some(reference) => Some(api.sites.expect(reference).await.map_err(refused)?.0),
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
        // its rule: only ever when somebody asks. The blocking hop — a tool to run and a file to
        // write — is inside `Autostart`, which also reads and writes the home's linger record.
        rpc::method::AUTOSTART_STATUS => {
            no_params(params.as_ref())?;
            encode_result(&api.autostart.status().await.map_err(refused)?)
        }

        rpc::method::AUTOSTART_ENABLE => {
            no_params(params.as_ref())?;
            encode_result(&api.autostart.enable().await.map_err(refused)?)
        }

        rpc::method::AUTOSTART_DISABLE => {
            no_params(params.as_ref())?;
            encode_result(&api.autostart.disable().await.map_err(refused)?)
        }

        rpc::method::AUTOSTART_ENABLE_AT_BOOT => {
            no_params(params.as_ref())?;
            encode_result(&api.autostart.enable_at_boot().await.map_err(refused)?)
        }

        rpc::method::AUTOSTART_DISABLE_AT_BOOT => {
            no_params(params.as_ref())?;
            encode_result(&api.autostart.disable_at_boot().await.map_err(refused)?)
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

        rpc::method::HOME_PREVIOUS => encode_result(&api.home_previous().await.map_err(refused)?),

        rpc::method::HOME_RESTORE => encode_result(&api.home_restore().await.map_err(refused)?),

        rpc::method::SERVICE_FOUND => encode_result(&api.service_found().await.map_err(refused)?),

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
