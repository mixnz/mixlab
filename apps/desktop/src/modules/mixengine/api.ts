import { Channel, invoke } from "@tauri-apps/api/core";

import type { DaemonShutdown } from "@mixengine/api";
import type { DaemonStatus } from "@mixengine/api";
import type { StorageReport } from "@mixengine/api";
import type { ElevationStatus } from "@mixengine/api";
import type { ServiceList } from "@mixengine/api";
import type { SiteCreate } from "@mixengine/api";
import type { SiteCreation } from "@mixengine/api";
import type { SiteDetail } from "@mixengine/api";
import type { SiteList } from "@mixengine/api";
import type { SiteRemoval } from "@mixengine/api";
import type { SiteShare } from "@mixengine/api";
import type { SiteSharing } from "@mixengine/api";
import type { SiteUpdate } from "@mixengine/api";
import type { ProjectList } from "@mixengine/api";
import type { ProjectDetail } from "@mixengine/api";
import type { ProjectCreate } from "@mixengine/api";
import type { ProjectUpdate } from "@mixengine/api";
import type { ProjectRemoval } from "@mixengine/api";
import type { ProjectExport } from "@mixengine/api";
import type { ProjectSummary } from "@mixengine/api";
import type { RuntimeKind } from "@mixengine/api";
import type { RuntimeList } from "@mixengine/api";
import type { RuntimeCatalogue } from "@mixengine/api";
import type { RuntimeTarget } from "@mixengine/api";
import type { RuntimeInstall } from "@mixengine/api";
import type { Requirements } from "@mixengine/api";
import type { RuntimeUninstall } from "@mixengine/api";
import type { RuntimeRemoval } from "@mixengine/api";
import type { RuntimeSummary } from "@mixengine/api";
import type { ExtensionList } from "@mixengine/api";
import type { ExtensionChoice } from "@mixengine/api";
import type { ExtensionChange } from "@mixengine/api";
import type { JobSummary } from "@mixengine/api";
import type { DiskUsage } from "@mixengine/api";
import type { CleanupQuery } from "@mixengine/api";
import type { MetricsHistory } from "@mixengine/api";
import type { MetricsHistoryQuery } from "@mixengine/api";
import type { PackageList } from "@mixengine/api";
import type { PackageSummary } from "@mixengine/api";
import type { PackageCatalogue } from "@mixengine/api";
import type { PackageUpgrade } from "@mixengine/api";
import type { PackageUpgradeQuery } from "@mixengine/api";
import type { RuntimeUpgrade } from "@mixengine/api";
import type { RuntimeUpgradeQuery } from "@mixengine/api";
import type { UpgradePlan } from "@mixengine/api";
import type { PackageTarget } from "@mixengine/api";
import type { PackageInstall } from "@mixengine/api";
import type { PackageRemoval } from "@mixengine/api";
import type { ServiceLimitsReport } from "@mixengine/api";
import type { ResourceLimits } from "@mixengine/api";
import type { FrontEndSwitch } from "@mixengine/api";
import type { SaveResources, SaveResourcesSet, ServiceAutostartSet } from "@mixengine/api";
import type { CredentialStore, CredentialStoreChange, CredentialStoreSet } from "@mixengine/api";
import type { ServiceIdleSet } from "@mixengine/api";
import type { ServiceSummary } from "@mixengine/api";
import type { ServiceFoundList } from "@mixengine/api";
import type { PackageFoundList, RuntimeFoundList } from "@mixengine/api";
import type { HomePrevious, HomeRestoreReport } from "@mixengine/api";
import type { ServiceCreate } from "@mixengine/api";
import type { ServiceCreation } from "@mixengine/api";
import type { ServiceDelete } from "@mixengine/api";
import type { ServiceRemoval } from "@mixengine/api";
import type { DatabaseCreate } from "@mixengine/api";
import type { DatabaseAccount } from "@mixengine/api";
import type { DatabaseClientReport } from "@mixengine/api";
import type { DatabaseCredentials } from "@mixengine/api";
import type { ServiceWalk } from "@mixengine/api";
import type { DomainStatusReport } from "@mixengine/api";
import type { CaStatus } from "@mixengine/api";
import type { CertIssueReport } from "@mixengine/api";
import type { CertStatusReport } from "@mixengine/api";
import type { BlueprintList } from "@mixengine/api";
import type { BlueprintSummary } from "@mixengine/api";
import type { BlueprintCapture } from "@mixengine/api";
import type { BlueprintImport } from "@mixengine/api";
import type { BlueprintApply } from "@mixengine/api";
import type { BlueprintApplyResponse } from "@mixengine/api";
import type { InstalledExtensions } from "@mixengine/api";
import type { ExtensionCatalogue } from "@mixengine/api";
import type { ExtensionPlanRequest } from "@mixengine/api";
import type { ExtensionPlan } from "@mixengine/api";
import type { ExtensionInstall } from "@mixengine/api";
import type { ExtensionUninstall } from "@mixengine/api";
import type { ExtensionRemoval } from "@mixengine/api";
import type { AutostartReport } from "@mixengine/api";
import type { PathReport } from "@mixengine/api";
import type { DoctorReport } from "@mixengine/api";
import type { DoctorRepair } from "@mixengine/api";
import type { RepairReport } from "@mixengine/api";
import type { BundleReport } from "@mixengine/api";

/**
 * The only place this module calls `invoke()`.
 *
 * The frontend touches neither the network nor the disk: it calls through here and draws what
 * comes back. The types of what comes back are MixEngine's contract, taken straight from
 * `bindings/` at the repo root through the `@mixengine/api` alias — do not rewrite them here; that
 * is `npm run bindings`.
 */

/** What state the daemon is in, seen from this machine. */
export type Presence = "running" | "notAnswering" | "notRunning" | "notInstalled";

/**
 * The state, with the directories searched for `mixengined` — in the order searched (T111).
 *
 * `searched` only has content when `presence` is `notInstalled`; the other three states search
 * nothing.
 */
export type PresenceReport = { presence: Presence; searched: string[] };

export function presence(): Promise<PresenceReport> {
  return invoke<PresenceReport>("mixengine_presence");
}

/**
 * Where the four growing directories go, and whether that can still be changed — T146.
 *
 * Answerable **before any daemon exists**: it runs `mixengined --storage`, a command that reads and
 * creates nothing. That is what makes asking not the thing that closes off the choice.
 */
export function storage(): Promise<StorageReport> {
  return invoke<StorageReport>("mixengine_storage");
}

/** The four directories the user just picked, in the form the start command takes — T146. */
export type ChosenPaths = {
  runtimes?: string;
  packages?: string;
  data?: string;
  logs?: string;
};

/**
 * Starts the daemon; returns the endpoint it prints once it is ready.
 *
 * `chosen` carries only the keys the user actually changed. The daemon writes them into
 * `config.toml` — the flags here configure *a home*, not a process — and refuses to start if
 * anything is already installed, because by then the locations live in the database's rows.
 */
export function startDaemon(chosen?: ChosenPaths): Promise<string> {
  return invoke<string>("mixengine_start", { chosen: chosen ?? null });
}

export function status(): Promise<DaemonStatus> {
  return invoke<DaemonStatus>("mixengine_status");
}

/** `service.list` returns `{ services: [...] }`, not a bare array — measured on a real daemon,
 *  and `ServiceList` in the contract says exactly that. */
export function services(): Promise<ServiceList> {
  return invoke<ServiceList>("mixengine_services");
}

export type ServiceAction = "start" | "stop" | "restart";

export function serviceAction(id: string, action: ServiceAction): Promise<unknown> {
  return invoke("mixengine_service_action", { id, action });
}

/**
 * `service.start` with a project scope — *every service this project needs*, in dependency order
 * (T125).
 *
 * That set is the daemon's answer: the services the project's sites declare, the php-fpm pools
 * they name, and the front end they are served through. Before T125 this sent an empty target,
 * meaning every service the home declares — a home with four PHP versions started all four to
 * bring up one site.
 */
export function serviceStartProject(project: string): Promise<unknown> {
  return invoke("mixengine_service_start_project", { project });
}

/**
 * Opens the event stream.
 *
 * Each message is **raw** JSON: the caller parses it, because an unknown `type` has to be
 * ignorable rather than break anything. MixEngine's events are internally tagged, and a variant
 * born in a later version has to reach an older MixLab as an object it recognises and ignores.
 */
export function watch(onMessage: (raw: string) => void): Promise<void> {
  const channel = new Channel<string>();
  channel.onmessage = onMessage;
  return invoke("mixengine_watch", { onEvent: channel });
}

export function unwatch(): Promise<void> {
  return invoke("mixengine_unwatch");
}

/**
 * Every operation waiting for administrator rights, with the description the daemon wrote for
 * each.
 *
 * `daemon.status` carries only a number. A tab opened while operations are already waiting receives
 * no `elevation_required` — that event only fires when the queue changes — so this is the only way
 * to see them.
 */
export function elevationStatus(): Promise<ElevationStatus> {
  return invoke<ElevationStatus>("mixengine_elevation_status");
}

/** Grants the whole batch of waiting operations — exactly one OS prompt.
 *
 *  **Returns a job, not a result.** The daemon creates the job row and answers at once; the prompt
 *  comes up *afterwards*, inside the job. The caller has to follow it through `jobStatus` until the
 *  job finishes — the job's `result` is a `GrantOutcome` (`completed`/`declined`/`unavailable`). */
export function elevationGrant(): Promise<JobSummary> {
  return invoke<JobSummary>("mixengine_elevation_grant");
}

/** Drops the whole batch. Declining is an ordinary outcome, not an error. */
export function elevationDrop(): Promise<unknown> {
  return invoke("mixengine_elevation_drop");
}

/** `project` filters by name; leave it empty to see every site in the home. */
export function sites(project?: string): Promise<SiteList> {
  return invoke<SiteList>("mixengine_sites", { project });
}

/** Everything only a lookup can answer: `doc_root_full`, `pool`, `services`. */
export function site(domain: string): Promise<SiteDetail> {
  return invoke<SiteDetail>("mixengine_site", { domain });
}

export function siteCreate(input: SiteCreate): Promise<SiteCreation> {
  return invoke<SiteCreation>("mixengine_site_create", { params: input });
}

/** `domains`/`services` replace the site's whole existing list; nothing is merged. */
export function siteUpdate(input: SiteUpdate): Promise<{ site: SiteDetail }> {
  return invoke("mixengine_site_update", { params: input });
}

export function siteShare(input: SiteShare): Promise<SiteSharing> {
  return invoke<SiteSharing>("mixengine_site_share", { params: input });
}

export function siteUnshare(domain: string): Promise<unknown> {
  return invoke("mixengine_site_unshare", { domain });
}

/** Serves the site again. A flag and a re-render of the front end's configuration: no service is
 *  started by it, which is still Open's job. */
export function siteStart(domain: string): Promise<SiteDetail> {
  return invoke<SiteDetail>("mixengine_site_start", { domain });
}

/** Stops serving the site and keeps its declaration; the services it uses keep running. */
export function siteStop(domain: string): Promise<SiteDetail> {
  return invoke<SiteDetail>("mixengine_site_stop", { domain });
}

/** Refused for an extension's site. The doc root is kept on disk, and `doc_root_kept` names it. */
export function siteDelete(domain: string): Promise<SiteRemoval> {
  return invoke<SiteRemoval>("mixengine_site_delete", { domain });
}

export function projects(): Promise<ProjectList> {
  return invoke<ProjectList>("mixengine_projects");
}

/** The **effective** pins (the file wins over the row) with the project — used by both the detail
 *  page and the edit form. */
export function projectShow(name: string): Promise<ProjectDetail> {
  return invoke<ProjectDetail>("mixengine_project_show", { name });
}

/** `project.create` returns the effective pins too, in the shape of `ProjectDetail` — not a bare
 *  `ProjectSummary`. Use `.project` for the row just created (see `ProjectForm.tsx`, where this
 *  layer was once misread). */
export function projectCreate(input: ProjectCreate): Promise<ProjectDetail> {
  return invoke<ProjectDetail>("mixengine_project_create", { params: input });
}

/** `pins` replaces everything — send back every existing pin plus the change. */
export function projectUpdate(input: ProjectUpdate): Promise<ProjectSummary> {
  return invoke<ProjectSummary>("mixengine_project_update", { params: input });
}

/** The directory and `mixengine.toml` are kept — only the registration is removed. */
export function projectDelete(name: string): Promise<ProjectRemoval> {
  return invoke<ProjectRemoval>("mixengine_project_delete", { name });
}

/** Writes `<root>/mixengine.toml`, merging into one that is there. `sites_omitted` names the sites
 *  the file could not hold, since a manifest has one `[site]`. */
export function projectExport(name: string): Promise<ProjectExport> {
  return invoke<ProjectExport>("mixengine_project_export", { name });
}

/** `domain.dns_status` is both the listing and the diagnosis of one name — leave `domain` empty to
 *  see every name. */
export function domains(domain?: string): Promise<DomainStatusReport> {
  return invoke<DomainStatusReport>("mixengine_domains", { domain });
}

export function domainAdd(site: string, domain: string, acceptRiskyTld: boolean): Promise<unknown> {
  return invoke("mixengine_domain_add", {
    params: { site: { domain: site }, domain, accept_risky_tld: acceptRiskyTld },
  });
}

export function domainRemove(domain: string): Promise<unknown> {
  return invoke("mixengine_domain_remove", { domain });
}

/** Two trust answers, not one: `trust` is the system store, `browsers` is the NSS database. */
export function caStatus(): Promise<CaStatus> {
  return invoke<CaStatus>("mixengine_ca_status");
}

/** The two-pass T64 flow, like `doctorRepair`: `grant: false` to enqueue, read `elevation.status`,
 *  and only then `elevation.grant` once the user has seen the queue — see `CaBlock.tsx`. */
export function caRepair(input: DoctorRepair): Promise<unknown> {
  return invoke("mixengine_ca_repair", { params: input });
}

/** Leave `domain` empty to issue for every site that declares HTTPS — one call both draws the table
 *  and reissues. */
export function certs(domain?: string): Promise<CertIssueReport> {
  return invoke<CertIssueReport>("mixengine_certs", { domain });
}

/** What the running front end presents, through a TLS handshake per site. Reads only, and slow
 *  when the front end is down, so it is asked for rather than read on every reload. */
export function certStatus(domain?: string): Promise<CertStatusReport> {
  return invoke<CertStatusReport>("mixengine_cert_status", { domain });
}

export function runtimesInstalled(kind?: RuntimeKind): Promise<RuntimeList> {
  return invoke<RuntimeList>("mixengine_runtime_list_installed", { filter: { kind } });
}

export function runtimesAvailable(kind?: RuntimeKind): Promise<RuntimeCatalogue> {
  return invoke<RuntimeCatalogue>("mixengine_runtime_list_available", { filter: { kind } });
}

export function runtimeRequirements(target: RuntimeTarget): Promise<Requirements> {
  return invoke<Requirements>("mixengine_runtime_requirements", { target });
}

export function runtimeInstall(params: RuntimeInstall): Promise<JobSummary> {
  return invoke<JobSummary>("mixengine_runtime_install", { target: params });
}

export function runtimeUninstall(params: RuntimeUninstall): Promise<RuntimeRemoval> {
  return invoke<RuntimeRemoval>("mixengine_runtime_uninstall", { params });
}

export function runtimeSetDefault(target: RuntimeTarget): Promise<RuntimeSummary> {
  return invoke<RuntimeSummary>("mixengine_runtime_set_default", { target });
}

export function runtimeExtensions(target: RuntimeTarget): Promise<ExtensionList> {
  return invoke<ExtensionList>("mixengine_runtime_list_extensions", { target });
}

export function runtimeSetExtension(choice: ExtensionChoice): Promise<ExtensionChange> {
  return invoke<ExtensionChange>("mixengine_runtime_set_extension", { choice });
}

/** What updating one installed runtime within its line would do; changes nothing — T193b. */
export function runtimeUpgradePlan(query: RuntimeUpgradeQuery): Promise<UpgradePlan> {
  return invoke<UpgradePlan>("mixengine_runtime_upgrade_plan", { query });
}

/** Starts the update; the job's result is the plan, marked with what was done — T193b. */
export function runtimeUpgrade(asked: RuntimeUpgrade): Promise<JobSummary> {
  return invoke<JobSummary>("mixengine_runtime_upgrade", { asked });
}

/** `runtimeUpgradePlan` for a server package — T193c. */
export function packageUpgradePlan(query: PackageUpgradeQuery): Promise<UpgradePlan> {
  return invoke<UpgradePlan>("mixengine_package_upgrade_plan", { query });
}

/** `runtimeUpgrade` for a server package. Always sends `grant: true` — see the Tauri command. */
export function packageUpgrade(asked: PackageUpgrade): Promise<JobSummary> {
  return invoke<JobSummary>("mixengine_package_upgrade", { asked: { ...asked, grant: true } });
}

export function packagesInstalled(name?: string): Promise<PackageList> {
  return invoke<PackageList>("mixengine_package_list", { filter: { package: name } });
}

export function packagesAvailable(name?: string): Promise<PackageCatalogue> {
  return invoke<PackageCatalogue>("mixengine_package_list_available", { filter: { package: name } });
}

export function packageRequirements(target: PackageTarget): Promise<Requirements> {
  return invoke<Requirements>("mixengine_package_requirements", { target });
}

export function packageInstall(params: PackageInstall): Promise<JobSummary> {
  return invoke<JobSummary>("mixengine_package_install", { target: params });
}

export function packageUninstall(target: PackageTarget): Promise<PackageRemoval> {
  return invoke<PackageRemoval>("mixengine_package_uninstall", { target });
}

export function serviceLimits(service: string): Promise<ServiceLimitsReport> {
  return invoke<ServiceLimitsReport>("mixengine_service_limits", { service });
}

/** `ServiceLimitsSet` sends all three fields — there is no patch. */
export function serviceSetLimits(
  service: string,
  limits: ResourceLimits,
): Promise<ServiceLimitsReport> {
  return invoke<ServiceLimitsReport>("mixengine_service_set_limits", {
    params: { service, limits },
  });
}

/** The answer's shape has no vendored type yet — read as `unknown`, and cast at the call site once
 *  confirmed against a real daemon (spec, Testing). */
export function serviceIdle(service: string): Promise<unknown> {
  return invoke("mixengine_service_idle", { service });
}

export function serviceSetIdle(params: ServiceIdleSet): Promise<unknown> {
  return invoke("mixengine_service_set_idle", { params });
}

/** `service.save_resources` — whether this home stops services nobody is using ("Save battery",
 *  T167b). Off unless the user has turned it on (ADR 0041). */
export function saveResources(): Promise<SaveResources> {
  return invoke<SaveResources>("mixengine_service_save_resources");
}

/** `service.set_save_resources` — turns "Save battery" on/off. Stops nothing and starts nothing:
 *  the next idle sweep is what reads it. Returns the new state. */
export function setSaveResources(on: boolean): Promise<SaveResources> {
  const params: SaveResourcesSet = { on };
  return invoke<SaveResources>("mixengine_service_set_save_resources", { params });
}

/** `daemon.set_credential_store` — where this home keeps its passwords from the next start (T194). */
export function setCredentialStore(store: CredentialStore): Promise<CredentialStoreChange> {
  const params: CredentialStoreSet = { store };
  return invoke<CredentialStoreChange>("mixengine_daemon_set_credential_store", { params });
}

/** `service.set_autostart` — whether this service starts along with MixEngine (T112).
 *  Starts nothing and stops nothing: what it changes is the walk at the next daemon start. */
export function serviceSetAutostart(params: ServiceAutostartSet): Promise<ServiceSummary> {
  return invoke<ServiceSummary>("mixengine_service_set_autostart", { params });
}

/** Changes the default web server — a job (followed through `jobStatus`) whose result is
 *  `FrontEndReport`. The active server has no read method of its own: read `ServiceSummary.role`
 *  from `services()`. */
export function serviceSetFrontEnd(params: FrontEndSwitch): Promise<JobSummary> {
  return invoke<JobSummary>("mixengine_service_set_front_end", { params });
}

/** `version` is required — no `service.resolve` picks one for you; see `ServiceCreate`'s doc. */
export function serviceCreate(params: ServiceCreate): Promise<ServiceCreation> {
  return invoke<ServiceCreation>("mixengine_service_create", { params });
}

export function serviceDelete(params: ServiceDelete): Promise<ServiceRemoval> {
  return invoke<ServiceRemoval>("mixengine_service_delete", { params });
}

export function databaseCreate(input: DatabaseCreate): Promise<DatabaseAccount> {
  return invoke<DatabaseAccount>("mixengine_database_create", { params: input });
}

export function databaseClient(service: string): Promise<DatabaseClientReport> {
  return invoke<DatabaseClientReport>("mixengine_database_client", { service });
}

/**
 * The password MixEngine holds for one account — `database.credentials`, T77b.
 *
 * **The only answer in the whole API that carries the password itself** (ADR 0025); every other
 * `database.*` only returns its *address* in the credential store. An absent `user` means the
 * server's administrator — the same default `database.open` uses.
 */
export function databaseCredentials(service: string, user?: string): Promise<DatabaseCredentials> {
  return invoke<DatabaseCredentials>("mixengine_database_credentials", { service, user });
}

/**
 * Writes the administrator credential back into the database's own data directory —
 * `service.reset_credential`, T127.
 *
 * Stops this service and everything that depends on it, runs the recipe's offline set-password
 * step, then starts them again. **Every database in that directory is kept intact** — that is what
 * decides this for a person, so every caller of this function has to say it first.
 */
export function serviceResetCredential(service: string): Promise<ServiceWalk> {
  return invoke<ServiceWalk>("mixengine_service_reset_credential", { service });
}

/** What a copy of an earlier install's state in the kept folders holds — `home.previous`, T182h. */
export function homePrevious(): Promise<HomePrevious> {
  return invoke<HomePrevious>("mixengine_home_previous");
}

/** Bring that copy back into this home — `home.restore`, T182h. */
export function homeRestore(): Promise<HomeRestoreReport> {
  return invoke<HomeRestoreReport>("mixengine_home_restore");
}

/** Runtime directories on disk with no row, and the daemon's reason for each — `runtime.found`, T182i. */
export function runtimesFound(): Promise<RuntimeFoundList> {
  return invoke<RuntimeFoundList>("mixengine_runtime_found");
}

/** Record one runtime version that is on disk without a row — `runtime.adopt`, T182f. */
export function runtimeAdopt(kind: string, version: string): Promise<RuntimeSummary> {
  return invoke<RuntimeSummary>("mixengine_runtime_adopt", { kind, version });
}

/** Package directories on disk with no row, and the daemon's reason for each — `package.found`, T182i. */
export function packagesFound(): Promise<PackageFoundList> {
  return invoke<PackageFoundList>("mixengine_package_found");
}

/** Record one package version that is on disk without a row — `package.adopt`, T182f. */
export function packageAdopt(pkg: string, version: string): Promise<PackageSummary> {
  return invoke<PackageSummary>("mixengine_package_adopt", { package: pkg, version });
}

/** Service data an earlier install left under `data/`, and whether each can be adopted — `service.found`, T182g. */
export function serviceFound(): Promise<ServiceFoundList> {
  return invoke<ServiceFoundList>("mixengine_service_found");
}

/**
 * Turn one found data directory back into a service — `service.adopt`, T182g. The service is left
 * stopped, with a new admin password; the databases and accounts in it are kept.
 */
export function serviceAdopt(service: string): Promise<ServiceSummary> {
  return invoke<ServiceSummary>("mixengine_service_adopt", { service });
}

/** Returns nothing — success means a new `db` tab has been queued to open; see
 *  `Handoff`/`crate::launch::request` on the Rust side. */
export function databaseExploreData(service: string, database?: string): Promise<void> {
  return invoke("mixengine_database_explore_data", { service, database });
}

/** Opens a service's log stream. The same pattern as `watch`/`unwatch` — a new `Channel`, and the
 *  caller parses the raw JSON. */
export function logsWatch(
  service: string,
  tail: number,
  follow: boolean,
  onLine: (raw: string) => void,
): Promise<void> {
  const channel = new Channel<string>();
  channel.onmessage = onLine;
  return invoke("mixengine_logs_watch", { service, tail, follow, onLine: channel });
}

export function logsUnwatch(): Promise<void> {
  return invoke("mixengine_logs_unwatch");
}

export function blueprints(): Promise<BlueprintList> {
  return invoke<BlueprintList>("mixengine_blueprints");
}

export function blueprintCapture(input: BlueprintCapture): Promise<BlueprintSummary> {
  return invoke<BlueprintSummary>("mixengine_blueprint_capture", { params: input });
}

/** Never returns an error for a bad signature — read `trusted`/`signature` on the result instead;
 *  there is no separate error to catch for that case. */
export function blueprintImport(input: BlueprintImport): Promise<BlueprintSummary> {
  return invoke<BlueprintSummary>("mixengine_blueprint_import", { params: input });
}

/** One method, called twice — `input.dry_run` decides which pass. */
export function blueprintApply(input: BlueprintApply): Promise<BlueprintApplyResponse> {
  return invoke<BlueprintApplyResponse>("mixengine_blueprint_apply", { params: input });
}

/** Reads a finished job — `applyJob` has already removed its row from the running-job list on the
 *  stream. */
export function jobStatus(job: number): Promise<JobSummary> {
  return invoke<JobSummary>("mixengine_job_status", { job });
}

/** Asks a job to stop — asking is all it does. The work ends when it next looks, and `job_finished`
 *  is what says it did; cancelling a job that has already ended is not an error. */
export function jobCancel(job: number): Promise<JobSummary> {
  return invoke<JobSummary>("mixengine_job_cancel", { job });
}

/** A job's real output (e.g. a blueprint's `[scaffold]` command) — the same pattern as
 *  `logsWatch`, with a different Rust-side route (`GET /logs/job/{id}` instead of
 *  `/logs/service/{id}`). */
export function jobLogsWatch(
  job: number,
  tail: number,
  follow: boolean,
  onLine: (raw: string) => void,
): Promise<void> {
  const channel = new Channel<string>();
  channel.onmessage = onLine;
  return invoke("mixengine_job_logs_watch", { job, tail, follow, onLine: channel });
}

/** The same Rust-side state as `logsUnwatch` — closes whatever log stream is open, service or
 *  job. */
export function jobLogsUnwatch(): Promise<void> {
  return invoke("mixengine_logs_unwatch");
}

export function extensionsInstalled(): Promise<InstalledExtensions> {
  return invoke<InstalledExtensions>("mixengine_extension_list_installed");
}

export function extensionsAvailable(): Promise<ExtensionCatalogue> {
  return invoke<ExtensionCatalogue>("mixengine_extension_list_available");
}

/** The only step before installing — there is no `extensionInspect`; see Decision D2 in the
 *  spec. */
export function extensionPlan(input: ExtensionPlanRequest): Promise<ExtensionPlan> {
  return invoke<ExtensionPlan>("mixengine_extension_plan", { params: input });
}

/** `input.consent` must be taken verbatim from the `ExtensionPlan` just received — see Decision D3
 *  in the spec. Answers the job the install runs as (T200, D3): the install is not done when this
 *  returns. */
export function extensionInstall(input: ExtensionInstall): Promise<JobSummary> {
  return invoke<JobSummary>("mixengine_extension_install", { params: input });
}

export function extensionUninstall(input: ExtensionUninstall): Promise<ExtensionRemoval> {
  return invoke<ExtensionRemoval>("mixengine_extension_uninstall", { params: input });
}

/** Calls `extension.*`, not `service.*` — see Global Constraints. */
export function extensionStart(id: string): Promise<unknown> {
  return invoke("mixengine_extension_start", { id });
}

export function extensionStop(id: string): Promise<unknown> {
  return invoke("mixengine_extension_stop", { id });
}

/** Opens `GET /metrics`. The same pattern as `logsWatch` — a new `Channel`, and the caller parses
 *  the raw JSON. **Opening this connection is the subscription**: call it exactly when the screen
 *  needs the "now" figures, and close it with `metricsUnwatch()` as soon as they are no longer
 *  needed — not held open for the app's lifetime like `watch()`/`/events`. */
export function metricsWatch(onFrame: (raw: string) => void): Promise<void> {
  const channel = new Channel<string>();
  channel.onmessage = onFrame;
  return invoke("mixengine_metrics_watch", { onFrame: channel });
}

export function metricsUnwatch(): Promise<void> {
  return invoke("mixengine_metrics_unwatch");
}

export function diskUsage(refresh: boolean): Promise<DiskUsage> {
  return invoke<DiskUsage>("mixengine_disk_usage", { refresh });
}

export function cleanup(query: CleanupQuery): Promise<JobSummary> {
  return invoke<JobSummary>("mixengine_cleanup", { params: query });
}

export function metricsHistory(query: MetricsHistoryQuery): Promise<MetricsHistory> {
  return invoke<MetricsHistory>("mixengine_metrics_history", { params: query });
}

export function autostartStatus(): Promise<AutostartReport> {
  return invoke<AutostartReport>("mixengine_autostart_status");
}

export function autostartEnable(): Promise<AutostartReport> {
  return invoke<AutostartReport>("mixengine_autostart_enable");
}

export function autostartDisable(): Promise<AutostartReport> {
  return invoke<AutostartReport>("mixengine_autostart_disable");
}

/** `path.status` — whether `<root>/bin` is on this user's saved PATH yet. */
export function pathStatus(): Promise<PathReport> {
  return invoke<PathReport>("mixengine_path_status");
}

/** `path.install` — fills `<root>/bin` and puts it on PATH. Raises no administrator dialog. */
export function pathInstall(): Promise<PathReport> {
  return invoke<PathReport>("mixengine_path_install");
}

/** `path.uninstall` — removes `<root>/bin` from PATH; the commands in it stay. */
export function pathUninstall(): Promise<PathReport> {
  return invoke<PathReport>("mixengine_path_uninstall");
}

/** `service.stop` for every declared service, in reverse dependency order, waiting until done. */
export function serviceStopAll(): Promise<unknown> {
  return invoke("mixengine_service_stop_all");
}

/**
 * `daemon.shutdown`. Answers after every service has stopped, with what stopped and what would not,
 * and the daemon exits a moment later — a connection closing after this resolves is the shutdown,
 * not an error.
 */
export function shutdown(): Promise<DaemonShutdown> {
  return invoke<DaemonShutdown>("mixengine_shutdown");
}

export function doctor(): Promise<DoctorReport> {
  return invoke<DoctorReport>("mixengine_doctor");
}

/** `grant: false` (the normal path) enqueues onto the same shared `elevation.status` queue — read
 *  that again to know whether `ElevationDialog` needs to open; no dialog of its own is returned. */
export function doctorRepair(input: DoctorRepair): Promise<RepairReport> {
  return invoke<RepairReport>("mixengine_doctor_repair", { params: input });
}

export function bundle(): Promise<BundleReport> {
  return invoke<BundleReport>("mixengine_bundle");
}
