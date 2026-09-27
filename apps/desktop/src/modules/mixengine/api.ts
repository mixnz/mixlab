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
import type { SiteShare } from "@mixengine/api";
import type { SiteSharing } from "@mixengine/api";
import type { SiteUpdate } from "@mixengine/api";
import type { ProjectList } from "@mixengine/api";
import type { ProjectDetail } from "@mixengine/api";
import type { ProjectCreate } from "@mixengine/api";
import type { ProjectUpdate } from "@mixengine/api";
import type { ProjectRemoval } from "@mixengine/api";
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
import type { PackageTarget } from "@mixengine/api";
import type { PackageInstall } from "@mixengine/api";
import type { PackageRemoval } from "@mixengine/api";
import type { ServiceLimitsReport } from "@mixengine/api";
import type { ResourceLimits } from "@mixengine/api";
import type { FrontEndSwitch } from "@mixengine/api";
import type { SaveResources, SaveResourcesSet, ServiceAutostartSet } from "@mixengine/api";
import type { ServiceIdleSet } from "@mixengine/api";
import type { ServiceSummary } from "@mixengine/api";
import type { ServiceFoundList } from "@mixengine/api";
import type { PackageFoundList, RuntimeFoundList } from "@mixengine/api";
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
 * Chỗ duy nhất module này gọi `invoke()`.
 *
 * Frontend không chạm mạng và không chạm đĩa: nó gọi qua đây và vẽ thứ quay về. Kiểu của những
 * thứ quay về là hợp đồng của MixEngine, lấy thẳng từ `bindings/` ở gốc repo qua alias `@mixengine/api` — đừng viết lại chúng ở
 * đây, đó là `npm run bindings`.
 */

/** Daemon đang ở trạng thái nào, nhìn từ máy này. */
export type Presence = "running" | "notAnswering" | "notRunning" | "notInstalled";

/**
 * Trạng thái, kèm những thư mục đã tìm `mixengined` — đúng thứ tự đã tìm (T111).
 *
 * `searched` chỉ có nội dung khi `presence` là `notInstalled`; ba trạng thái kia không tìm gì cả.
 */
export type PresenceReport = { presence: Presence; searched: string[] };

export function presence(): Promise<PresenceReport> {
  return invoke<PresenceReport>("mixengine_presence");
}

/**
 * Bốn thư mục phình to được đặt ở đâu, và điều đó còn đổi được không — T146.
 *
 * Trả lời được **khi chưa có daemon nào**: nó chạy `mixengined --storage`, một lệnh đọc và không
 * tạo ra thứ gì. Đó là điều làm cho việc hỏi không phải là thứ đóng mất quyền chọn.
 */
export function storage(): Promise<StorageReport> {
  return invoke<StorageReport>("mixengine_storage");
}

/** Bốn thư mục người dùng vừa chọn, đúng dạng lệnh khởi động nhận — T146. */
export type ChosenPaths = {
  runtimes?: string;
  packages?: string;
  data?: string;
  logs?: string;
};

/**
 * Khởi động daemon; trả về endpoint nó in ra khi đã sẵn sàng.
 *
 * `chosen` chỉ mang những khoá người dùng thật sự đổi. Daemon ghi chúng vào `config.toml` — cờ ở
 * đây cấu hình *một home*, không phải một tiến trình — và từ chối lần khởi động nếu đã có thứ gì
 * được cài, vì lúc đó chỗ đặt đã nằm trong các dòng của database.
 */
export function startDaemon(chosen?: ChosenPaths): Promise<string> {
  return invoke<string>("mixengine_start", { chosen: chosen ?? null });
}

export function status(): Promise<DaemonStatus> {
  return invoke<DaemonStatus>("mixengine_status");
}

/** `service.list` trả `{ services: [...] }`, không phải một mảng trần — đo được trên daemon thật,
 *  và `ServiceList` trong hợp đồng nói đúng như vậy. */
export function services(): Promise<ServiceList> {
  return invoke<ServiceList>("mixengine_services");
}

export type ServiceAction = "start" | "stop" | "restart";

export function serviceAction(id: string, action: ServiceAction): Promise<unknown> {
  return invoke("mixengine_service_action", { id, action });
}

/**
 * `service.start` với scope project — *mọi service project này cần*, theo thứ tự phụ thuộc (T125).
 *
 * Tập ấy là câu trả lời của daemon: service các site của project khai, pool php-fpm chúng đặt tên,
 * và front end chúng được phục vụ qua. Trước T125 chỗ này gửi target rỗng, nghĩa là mọi service
 * home khai — một home bốn bản PHP bật cả bốn để dựng một site.
 */
export function serviceStartProject(project: string): Promise<unknown> {
  return invoke("mixengine_service_start_project", { project });
}

/**
 * Mở stream sự kiện.
 *
 * Mỗi message là JSON **thô**: người gọi tự parse, vì một `type` chưa biết phải bỏ qua được chứ
 * không phải làm vỡ gì. Sự kiện của MixEngine internally tagged, và một biến thể sinh ra ở phiên
 * bản sau phải tới được một MixDB cũ như một object nó nhận ra và lờ đi.
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
 * Mọi thao tác đang chờ quyền quản trị, kèm câu daemon tự viết cho từng cái.
 *
 * `daemon.status` chỉ mang một con số. Một tab mở ra khi đã có sẵn thao tác chờ không nhận
 * `elevation_required` nào — sự kiện đó chỉ bắn lúc hàng đợi đổi — nên đây là đường duy nhất thấy
 * chúng.
 */
export function elevationStatus(): Promise<ElevationStatus> {
  return invoke<ElevationStatus>("mixengine_elevation_status");
}

/** Cho phép cả lô thao tác đang chờ — đúng một prompt của hệ điều hành.
 *
 *  **Trả về một job, không phải kết quả.** Daemon tạo hàng job rồi trả lời ngay; prompt bật lên
 *  *sau đó*, bên trong job. Ai gọi phải theo dõi qua `jobStatus` tới khi job xong — `result` của
 *  job là một `GrantOutcome` (`completed`/`declined`/`unavailable`). */
export function elevationGrant(): Promise<JobSummary> {
  return invoke<JobSummary>("mixengine_elevation_grant");
}

/** Bỏ cả lô đi. Từ chối là một kết cục bình thường, không phải một lỗi. */
export function elevationDrop(): Promise<unknown> {
  return invoke("mixengine_elevation_drop");
}

/** `project` lọc theo tên; bỏ trống thấy mọi site trong home. */
export function sites(project?: string): Promise<SiteList> {
  return invoke<SiteList>("mixengine_sites", { project });
}

/** Mọi thứ chỉ một lookup mới trả lời được: `doc_root_full`, `pool`, `services`. */
export function site(domain: string): Promise<SiteDetail> {
  return invoke<SiteDetail>("mixengine_site", { domain });
}

export function siteCreate(input: SiteCreate): Promise<SiteCreation> {
  return invoke<SiteCreation>("mixengine_site_create", { params: input });
}

/** `domains`/`services` thay thế toàn bộ danh sách site đang có, không merge. */
export function siteUpdate(input: SiteUpdate): Promise<{ site: SiteDetail }> {
  return invoke("mixengine_site_update", { params: input });
}

export function siteShare(input: SiteShare): Promise<SiteSharing> {
  return invoke<SiteSharing>("mixengine_site_share", { params: input });
}

export function siteUnshare(domain: string): Promise<unknown> {
  return invoke("mixengine_site_unshare", { domain });
}

export function projects(): Promise<ProjectList> {
  return invoke<ProjectList>("mixengine_projects");
}

/** Pin **hiệu lực** (file thắng row) kèm project — dùng cho cả trang chi tiết và form sửa. */
export function projectShow(name: string): Promise<ProjectDetail> {
  return invoke<ProjectDetail>("mixengine_project_show", { name });
}

/** `project.create` trả cả pin hiệu lực, đúng hình `ProjectDetail` — không phải `ProjectSummary`
 *  trần. Gọi `.project` để lấy hàng vừa tạo (xem `ProjectForm.tsx`, chỗ đã đọc nhầm tầng này). */
export function projectCreate(input: ProjectCreate): Promise<ProjectDetail> {
  return invoke<ProjectDetail>("mixengine_project_create", { params: input });
}

/** `pins` thay thế toàn bộ — gửi lại mọi pin hiện có cộng thay đổi. */
export function projectUpdate(input: ProjectUpdate): Promise<ProjectSummary> {
  return invoke<ProjectSummary>("mixengine_project_update", { params: input });
}

/** Thư mục và `mixengine.toml` được giữ nguyên — chỉ gỡ đăng ký. */
export function projectDelete(name: string): Promise<ProjectRemoval> {
  return invoke<ProjectRemoval>("mixengine_project_delete", { name });
}

/** `domain.dns_status` là cả liệt kê lẫn chẩn đoán một tên — bỏ trống `domain` thấy mọi tên. */
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

/** Hai câu trả lời tin cậy, không phải một: `trust` là kho hệ thống, `browsers` là NSS database. */
export function caStatus(): Promise<CaStatus> {
  return invoke<CaStatus>("mixengine_ca_status");
}

/** Luồng hai lượt T64, giống `doctorRepair`: `grant: false` để enqueue, đọc `elevation.status`
 *  rồi mới `elevation.grant` sau khi người dùng đã xem hàng đợi — xem `CaBlock.tsx`. */
export function caRepair(input: DoctorRepair): Promise<unknown> {
  return invoke("mixengine_ca_repair", { params: input });
}

/** Bỏ trống `domain` để cấp cho mọi site có khai HTTPS — cùng một call vẽ bảng lẫn cấp lại. */
export function certs(domain?: string): Promise<CertIssueReport> {
  return invoke<CertIssueReport>("mixengine_certs", { domain });
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

/** `ServiceLimitsSet` gửi toàn bộ ba field — không có patch. */
export function serviceSetLimits(
  service: string,
  limits: ResourceLimits,
): Promise<ServiceLimitsReport> {
  return invoke<ServiceLimitsReport>("mixengine_service_set_limits", {
    params: { service, limits },
  });
}

/** Hình dạng câu trả lời chưa có type đã vendor — đọc như `unknown`, ép kiểu tại chỗ gọi sau khi
 *  xác nhận với daemon thật (spec, Kiểm thử). */
export function serviceIdle(service: string): Promise<unknown> {
  return invoke("mixengine_service_idle", { service });
}

export function serviceSetIdle(params: ServiceIdleSet): Promise<unknown> {
  return invoke("mixengine_service_set_idle", { params });
}

/** `service.save_resources` — home này có dừng service không ai dùng không ("Save battery",
 *  T167b). Tắt trừ khi người dùng đã bật (ADR 0041). */
export function saveResources(): Promise<SaveResources> {
  return invoke<SaveResources>("mixengine_service_save_resources");
}

/** `service.set_save_resources` — bật/tắt "Save battery". Không dừng và không khởi động gì: lượt
 *  quét idle kế tiếp mới đọc nó. Trả trạng thái mới. */
export function setSaveResources(on: boolean): Promise<SaveResources> {
  const params: SaveResourcesSet = { on };
  return invoke<SaveResources>("mixengine_service_set_save_resources", { params });
}

/** `service.set_autostart` — service này có khởi động cùng MixEngine không (T112).
 *  Không khởi động và không dừng gì: thứ nó đổi là walk ở lần daemon khởi động sau. */
export function serviceSetAutostart(params: ServiceAutostartSet): Promise<ServiceSummary> {
  return invoke<ServiceSummary>("mixengine_service_set_autostart", { params });
}

/** Đổi web server mặc định — một job (theo dõi qua `jobStatus`), kết quả là `FrontEndReport`.
 *  Server đang active không có method đọc riêng: đọc `ServiceSummary.role` từ `services()`. */
export function serviceSetFrontEnd(params: FrontEndSwitch): Promise<JobSummary> {
  return invoke<JobSummary>("mixengine_service_set_front_end", { params });
}

/** `version` là bắt buộc — không có `service.resolve` nào chọn hộ, xem doc của `ServiceCreate`. */
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
 * Mật khẩu MixEngine đang giữ cho một account — `database.credentials`, T77b.
 *
 * **Câu trả lời duy nhất trong cả API mang chính mật khẩu** (ADR 0025); mọi `database.*` khác chỉ
 * trả *địa chỉ* của nó trong credential store. `user` vắng nghĩa là quản trị viên của server —
 * đúng mặc định `database.open` dùng.
 */
export function databaseCredentials(service: string, user?: string): Promise<DatabaseCredentials> {
  return invoke<DatabaseCredentials>("mixengine_database_credentials", { service, user });
}

/**
 * Ghi lại credential quản trị viên vào data directory của chính database — `service.reset_credential`,
 * T127.
 *
 * Dừng service này và mọi thứ phụ thuộc nó, chạy bước đặt mật khẩu offline của recipe, rồi bật lại.
 * **Mọi database trong thư mục ấy được giữ nguyên** — đó là câu quyết định chuyện này cho một
 * người, nên nơi nào gọi hàm này cũng phải nói nó ra trước.
 */
export function serviceResetCredential(service: string): Promise<ServiceWalk> {
  return invoke<ServiceWalk>("mixengine_service_reset_credential", { service });
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

/** Không trả gì — thành công nghĩa là một tab `db` mới đã được xếp hàng mở, xem
 *  `Handoff`/`crate::launch::request` phía Rust. */
export function databaseOpenInMixDB(service: string, database?: string): Promise<void> {
  return invoke("mixengine_database_open_in_mixdb", { service, database });
}

/** Mở stream log của một service. Cùng khuôn `watch`/`unwatch` — một `Channel` mới, người gọi tự
 *  parse JSON thô. */
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

/** Không bao giờ trả lỗi vì chữ ký sai — đọc lại `trusted`/`signature` trên kết quả, không bắt lỗi
 *  riêng cho trường hợp đó. */
export function blueprintImport(input: BlueprintImport): Promise<BlueprintSummary> {
  return invoke<BlueprintSummary>("mixengine_blueprint_import", { params: input });
}

/** Một method, gọi hai lượt — `input.dry_run` quyết định lượt nào. */
export function blueprintApply(input: BlueprintApply): Promise<BlueprintApplyResponse> {
  return invoke<BlueprintApplyResponse>("mixengine_blueprint_apply", { params: input });
}

/** Đọc job đã kết thúc — `applyJob` đã xoá hàng của nó khỏi danh sách job đang chạy trên stream. */
export function jobStatus(job: number): Promise<JobSummary> {
  return invoke<JobSummary>("mixengine_job_status", { job });
}

/** Output thật của một job (vd. lệnh `[scaffold]` của một blueprint) — cùng khuôn `logsWatch`, khác
 *  route phía Rust (`GET /logs/job/{id}` thay vì `/logs/service/{id}`). */
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

/** Cùng state phía Rust với `logsUnwatch` — đóng bất cứ stream log nào đang mở, service hay job. */
export function jobLogsUnwatch(): Promise<void> {
  return invoke("mixengine_logs_unwatch");
}

export function extensionsInstalled(): Promise<InstalledExtensions> {
  return invoke<InstalledExtensions>("mixengine_extension_list_installed");
}

export function extensionsAvailable(): Promise<ExtensionCatalogue> {
  return invoke<ExtensionCatalogue>("mixengine_extension_list_available");
}

/** Bước duy nhất trước khi cài — không có `extensionInspect`, xem Quyết định D2 spec. */
export function extensionPlan(input: ExtensionPlanRequest): Promise<ExtensionPlan> {
  return invoke<ExtensionPlan>("mixengine_extension_plan", { params: input });
}

/** `input.consent` phải trích nguyên từ `ExtensionPlan` vừa nhận — xem Quyết định D3 spec. */
export function extensionInstall(input: ExtensionInstall): Promise<unknown> {
  return invoke("mixengine_extension_install", { params: input });
}

export function extensionUninstall(input: ExtensionUninstall): Promise<ExtensionRemoval> {
  return invoke<ExtensionRemoval>("mixengine_extension_uninstall", { params: input });
}

/** Gọi `extension.*`, không phải `service.*` — xem Global Constraints. */
export function extensionStart(id: string): Promise<unknown> {
  return invoke("mixengine_extension_start", { id });
}

export function extensionStop(id: string): Promise<unknown> {
  return invoke("mixengine_extension_stop", { id });
}

/** Mở `GET /metrics`. Cùng khuôn `logsWatch` — một `Channel` mới, người gọi tự parse JSON thô.
 *  **Mở kết nối này chính là subscribe**: gọi đúng lúc màn hình cần số "bây giờ", đóng lại bằng
 *  `metricsUnwatch()` ngay khi không còn cần — không mở suốt đời app như `watch()`/`/events`. */
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

/** `path.status` — `<root>/bin` có trên PATH đã lưu của user này chưa. */
export function pathStatus(): Promise<PathReport> {
  return invoke<PathReport>("mixengine_path_status");
}

/** `path.install` — điền `<root>/bin` và đưa nó vào PATH. Không bật hộp thoại quản trị. */
export function pathInstall(): Promise<PathReport> {
  return invoke<PathReport>("mixengine_path_install");
}

/** `path.uninstall` — gỡ `<root>/bin` khỏi PATH, các lệnh trong đó vẫn còn. */
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

/** `grant: false` (đường thường) enqueue vào đúng hàng đợi `elevation.status` chung — đọc lại đó để
 *  biết có cần mở `ElevationDialog` không, không tự trả một dialog riêng. */
export function doctorRepair(input: DoctorRepair): Promise<RepairReport> {
  return invoke<RepairReport>("mixengine_doctor_repair", { params: input });
}

export function bundle(): Promise<BundleReport> {
  return invoke<BundleReport>("mixengine_bundle");
}
