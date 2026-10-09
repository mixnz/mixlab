import type { Channel } from "@tauri-apps/api/core";
import type {
  BlueprintApply,
  BlueprintApplyResponse,
  BlueprintList,
  BlueprintSummary,
  DaemonStatus,
  DatabaseClientReport,
  DiskUsage,
  ElevationStatus,
  HomePrevious,
  JobSummary,
  MetricsFrame,
  PackageCatalogue,
  PackageFoundList,
  PathReport,
  ProjectDetail,
  ProjectList,
  ProjectSummary,
  ProjectUpdate,
  RuntimeCatalogue,
  RuntimeKind,
  RuntimeList,
  RuntimeFoundList,
  RuntimeSummary,
  ServiceFoundList,
  ServiceList,
  ServiceSummary,
  SiteCreate,
  SiteCreation,
  SiteDetail,
  SiteList,
  SiteSummary,
} from "@mixengine/api";
import type { PresenceReport } from "../../src/modules/mixengine/api";
import pkg from "../../package.json";
import { returns, type Handlers } from "../ipc/dispatch";
import { demoOptions } from "./options";
import { createApplyRunner } from "./laravelApply";
import { createPinRegistry } from "./pinRegistry";
import { createSiteRegistry } from "./siteRegistry";
import { DAY, HOUR, MINUTE, NOW } from "./time";

/**
 * One believable machine: a developer's Mac with two projects, the PHP pools they use, a Node API,
 * and the three data services most web work needs — all running, one mail catcher stopped.
 */

const HOME = "/Users/ada/.mixengine";

function service(
  id: string,
  pid: number | null,
  port: number | null,
  startedMinutesAgo: number,
  extra: Partial<ServiceSummary> = {},
): ServiceSummary {
  return {
    id,
    state: pid === null ? "stopped" : "running",
    supervised: true,
    pid,
    port,
    last_started_at: pid === null ? null : NOW - startedMinutesAgo * MINUTE,
    last_exit_code: null,
    depends_on: [],
    role: { role: "other" },
    autostart: pid !== null,
    ...extra,
  };
}

const SERVICES: ServiceSummary[] = [
  service("caddy", 4102, 443, 4 * 60 + 12, { role: { role: "front_end", server: "caddy" } }),
  service("php-fpm@8.4", 4118, null, 4 * 60 + 12),
  service("php-fpm@8.3", 4121, null, 4 * 60 + 12),
  service("postgresql@17", 4133, 5432, 4 * 60 + 11),
  service("mariadb@main", 4140, 3306, 4 * 60 + 11),
  service("redis@main", 4152, 6379, 4 * 60 + 11),
  service("mailpit@main", null, null, 0),
];

const DATABASE_PROTOCOLS: Record<string, DatabaseClientReport["protocol"]> = {
  "postgresql@17": "postgres",
  "mariadb@main": "mysql",
  "redis@main": "redis",
};

function databaseClient(serviceId: string): DatabaseClientReport {
  const protocol = DATABASE_PROTOCOLS[serviceId] ?? null;
  return {
    service: serviceId,
    protocol,
    creates_databases: protocol === "postgres" || protocol === "mysql",
    client:
      protocol === null
        ? { state: "no_client" }
        : { state: "installed", name: "MixLab", program: "/Applications/MixLab.app" },
  };
}

const project = (name: string) => ({ type: "project" as const, name });

export const SITES: SiteSummary[] = [
  {
    domain: "acme-shop.test",
    owner: project("acme-shop"),
    kind: { kind: "php-fpm", pool: "php-fpm@8.4" },
    doc_root: "public",
    https: true,
    https_redirect: true,
    state: "enabled",
  },
  {
    domain: "api.acme.test",
    owner: project("acme-shop"),
    kind: { kind: "node-app", port: 3000 },
    doc_root: ".",
    https: true,
    https_redirect: true,
    state: "enabled",
  },
  {
    domain: "docs.acme.test",
    owner: project("acme-shop"),
    kind: { kind: "static" },
    doc_root: "docs/dist",
    https: true,
    https_redirect: false,
    state: "enabled",
  },
  {
    domain: "blog.test",
    owner: project("blog"),
    kind: { kind: "php-fpm", pool: "php-fpm@8.3" },
    doc_root: "public",
    https: true,
    https_redirect: true,
    state: "enabled",
  },
];

const PROJECTS: ProjectList = {
  projects: [
    { name: "acme-shop", root: "/Users/ada/Sites/acme-shop", created_at: "2025-11-03T09:12:00Z", keep_warm: true },
    { name: "blog", root: "/Users/ada/Sites/blog", created_at: "2025-12-18T16:40:00Z", keep_warm: false },
  ],
};

function siteDetail(domain: string): SiteDetail {
  const site = SITES.find((s) => s.domain === domain) ?? SITES[0];
  const owner = site.owner.type === "project" ? site.owner.name : "acme-shop";
  const root = `/Users/ada/Sites/${owner}`;
  const pool = site.kind.kind === "php-fpm" ? (site.kind.pool ?? null) : null;
  return {
    site,
    root,
    doc_root_full: `${root}/${site.doc_root}`,
    doc_root_exists: true,
    domains: [site.domain],
    pool: { declared: pool, resolved: pool },
    services: [
      { service: "postgresql@17", state: "running" },
      { service: "redis@main", state: "running" },
    ],
  };
}

/** The pool `SERVICES` runs for `blog` — what the daemon resolves a new php-fpm site to here. */
const DEFAULT_POOL = "php-fpm@8.3";

const MiB = 1024 ** 2;

function php(version: string, mebibytes: number, installedDaysAgo: number, isDefault: boolean): RuntimeSummary {
  return {
    kind: "php",
    version,
    channel: "stable",
    path: `${HOME}/runtimes/php/${version}`,
    installed_at: NOW - installedDaysAgo * DAY,
    bytes: mebibytes * MiB,
    default: isDefault,
  };
}

/** The PHP builds behind the two pools `SERVICES` runs. */
const RUNTIMES: RuntimeSummary[] = [php("8.4.26", 32, 12, true), php("8.3.35", 31, 40, false)];

/**
 * The pin-runtime clip's machine. The versions are the ones `docs/demo/pin-runtime-terminal.md`
 * recorded `php -v` printing through the real shim, so the clip and the website's terminal agree.
 */
const PIN_RUNTIMES: RuntimeSummary[] = [php("8.4.26", 32, 12, true), php("8.1.34", 28, 3, false)];
const PIN_PROJECTS: ProjectSummary[] = [
  { name: "blog", root: "/Users/ada/Sites/blog", created_at: "2025-12-18T16:40:00Z", keep_warm: false },
  { name: "legacy", root: "/Users/ada/Sites/legacy", created_at: "2026-02-09T10:05:00Z", keep_warm: false },
];

const pinMachine = demoOptions().runtimePins === true;

const sites = createSiteRegistry({
  sites: pinMachine ? [] : SITES,
  projects: pinMachine ? PIN_PROJECTS : PROJECTS.projects,
  without: demoOptions().sitesWithout,
  fresh: demoOptions().fresh,
  defaultPool: DEFAULT_POOL,
  detailOf: (site) => siteDetail(site.domain),
});

/** The Laravel blueprint apply the quick-start clip films; it holds the daemon's watch channel. */
const applying = createApplyRunner({ registry: sites, now: NOW });

const runtimes = createPinRegistry(pinMachine ? PIN_RUNTIMES : RUNTIMES);

function findProject(name: string): ProjectSummary {
  const found = sites.projects().projects.find((p) => p.name === name);
  if (found === undefined) throw new Error(`demo: no project named ${name}`);
  return found;
}

function projectDetail(name: string): ProjectDetail {
  return { project: findProject(name), pins: runtimes.pins(name) };
}

/** Pins only: no clip renames a project or moves its root. */
function projectUpdate(params: ProjectUpdate): ProjectSummary {
  if (!("name" in params.project)) throw new Error("demo: projects are updated by name");
  if (params.name || params.root) throw new Error("demo: only a project's pins change here");
  const found = findProject(params.project.name);
  if (params.pins) runtimes.replace(found.name, params.pins);
  return found;
}

const STATUS: DaemonStatus = {
  version: pkg.version,
  protocol: 1,
  pid: 4096,
  home: HOME,
  endpoint: `${HOME}/run/mixengined.sock`,
  database: `${HOME}/state.db`,
  started_at: NOW - (4 * HOUR + 12 * MINUTE),
  uptime: 4 * 3600 + 12 * 60,
  elevation: { elevated: false, can_prompt: true, pending: 0 },
  dns: { mode: "dns", listening: "127.0.0.1:53", wildcards: ["*.test"] },
  update: null,
};

const GB = 1024 ** 3;
const MB = 1024 ** 2;

const DISK: DiskUsage = {
  root: HOME,
  measured_at: NOW - 3 * MINUTE,
  categories: [
    {
      id: "runtimes",
      location: `${HOME}/runtimes`,
      bytes: Math.round(3.4 * GB),
      files: 41_210,
      reclaim: { reclaim: "by_method", method: "mix runtime uninstall", because: "a runtime a project still uses" },
    },
    {
      id: "data",
      location: `${HOME}/data`,
      bytes: Math.round(1.8 * GB),
      files: 3_904,
      reclaim: { reclaim: "never", because: "your databases" },
    },
    {
      id: "logs",
      location: `${HOME}/logs`,
      bytes: 212 * MB,
      files: 96,
      reclaim: { reclaim: "by_cleanup", bytes: 180 * MB, files: 71 },
    },
    {
      id: "certs",
      location: `${HOME}/certs`,
      bytes: 48_000,
      files: 12,
      reclaim: { reclaim: "never", because: "trusted local HTTPS" },
    },
    {
      id: "cache",
      location: `${HOME}/cache`,
      bytes: 640 * MB,
      files: 1_388,
      reclaim: { reclaim: "by_cleanup", bytes: 640 * MB, files: 1_388 },
    },
  ],
  other_bytes: 24 * MB,
};

const ELEVATION: ElevationStatus = { elevated: false, can_prompt: true, pending: [] };

function blueprint(slug: string, name: string, description: string): BlueprintSummary {
  return {
    slug,
    name,
    description,
    created_at: "2025-10-01T00:00:00Z",
    source: "builtin",
    trusted: true,
    signature: "verified",
    file: `${HOME}/blueprints/${slug}.toml`,
  };
}

const BLUEPRINTS: BlueprintList = {
  blueprints: [
    // The gallery manifest's own description (`crates/mixengine-core/src/blueprints/gallery/laravel.toml`).
    blueprint("laravel", "Laravel", "Laravel on PHP-FPM, with MariaDB and Redis"),
    blueprint("wordpress", "WordPress", "PHP 8.3 and MariaDB, ready for wp-cli."),
    blueprint("nextjs", "Next.js", "Node 22 behind HTTPS, with PostgreSQL."),
  ],
};

/** A minute of frames, two seconds apart, as smooth curves — the same every run. */
function metricFrames(): MetricsFrame[] {
  const running = SERVICES.filter((s) => s.pid !== null);
  return Array.from({ length: 30 }, (_, i) => ({
    at: NOW - (29 - i) * 2_000,
    // Eight logical processors: the tray and the Dashboard draw a share of the machine (T190c).
    cores: 8,
    samples: [
      { subject: "daemon", cpu_percent: 0.6 + 0.3 * Math.sin(i / 4), rss_bytes: 38 * MB, processes: 1 },
      ...running.map((s, n) => ({
        subject: `service:${s.id}`,
        cpu_percent: 1.5 + n * 0.8 + (1.2 + n * 0.3) * Math.sin(i / 5 + n),
        rss_bytes: (60 + n * 45) * MB + Math.round(4 * MB * Math.sin(i / 7 + n)),
        processes: s.id.startsWith("php-fpm") ? 4 : 1,
      })),
    ],
  }));
}

export const mixengineHandlers: Handlers = {
  mixengine_presence: returns<PresenceReport>({ presence: "running", searched: [] }),
  mixengine_status: returns<DaemonStatus>(STATUS),
  mixengine_services: returns<ServiceList>({ services: SERVICES }),
  mixengine_sites: (args): SiteList => sites.list(args.project as string | undefined),
  mixengine_site: (args): SiteDetail => sites.detail(args.domain as string),
  mixengine_site_create: (args): SiteCreation => sites.create(args.params as SiteCreate),
  mixengine_project_show: (args): ProjectDetail => projectDetail(args.name as string),
  mixengine_projects: (): ProjectList => sites.projects(),
  mixengine_project_update: (args): ProjectSummary => projectUpdate(args.params as ProjectUpdate),
  mixengine_runtime_list_installed: (args): RuntimeList =>
    runtimes.installed((args.filter as { kind?: RuntimeKind } | undefined)?.kind),
  mixengine_disk_usage: returns<DiskUsage>(DISK),
  // Set up already, so the Dashboard's PATH reminder stays out of the pictures.
  mixengine_path_status: returns<PathReport>({
    directory: `${HOME}/bin`,
    on_path: true,
    places: [{ name: "/Users/ada/.zprofile", present: true, changed: false }],
    commands: ["composer", "node", "npm", "php", "python3"],
  }),
  // Nothing an earlier install left, so the adopt cards stay out of the pictures (T182g, T182i).
  mixengine_service_found: returns<ServiceFoundList>({ found: [] }),
  mixengine_home_previous: returns<HomePrevious>({}),
  mixengine_runtime_found: returns<RuntimeFoundList>({ found: [] }),
  mixengine_package_found: returns<PackageFoundList>({ found: [] }),
  // Read only for Ruby's devkit offer beside a blueprint's plan (phase 44). Laravel needs no
  // devkit, so an empty index keeps the offer out of the clip.
  mixengine_runtime_list_available: returns<RuntimeCatalogue>({ runtimes: [], stale: false }),
  mixengine_package_list_available: returns<PackageCatalogue>({ packages: [], stale: false }),
  mixengine_elevation_status: returns<ElevationStatus>(ELEVATION),
  mixengine_blueprints: returns<BlueprintList>(BLUEPRINTS),
  mixengine_database_client: (args): DatabaseClientReport => databaseClient(args.service as string),
  mixengine_watch: (args) => applying.watch(args.onEvent as Channel<string>),
  mixengine_blueprint_apply: (args): BlueprintApplyResponse => applying.apply(args.params as BlueprintApply),
  mixengine_job_status: (args): JobSummary => applying.jobStatus(args.job as number),
  mixengine_job_logs_watch: (args) => applying.logsWatch(args.onLine as Channel<string>),
  mixengine_logs_unwatch: () => applying.logsUnwatch(),
  mixengine_service_start_project: returns(null),
  mixengine_unwatch: returns(null),
  mixengine_metrics_watch: (args) => {
    const channel = args.onFrame as Channel<string>;
    setTimeout(() => {
      for (const frame of metricFrames()) channel.onmessage(JSON.stringify(frame));
    }, 0);
    return null;
  },
  mixengine_metrics_unwatch: returns(null),
};
