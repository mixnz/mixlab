import type {
  BlueprintApplied,
  BlueprintApply,
  BlueprintApplyResponse,
  BlueprintPlan,
  JobSummary,
  PlanAction,
  PlanStep,
} from "@mixengine/api";
import type { SiteRegistry } from "./siteRegistry";

/**
 * The Laravel blueprint applied on the demo machine, answered in time — the quick-start clip.
 *
 * The plan is the gallery manifest (`crates/mixengine-core/src/blueprints/gallery/laravel.toml`)
 * in the order `crates/mixengine-core/src/blueprints/plan.rs` plans it, and the progress reads the
 * way `crates/mixengine-daemon/src/api/apply.rs` reports it; `laravelApply.test.ts` reads the
 * manifest so the two cannot drift apart unnoticed. No front-end steps: the daemon plans them only
 * when no front end is held, and this machine runs Caddy as its front end.
 *
 * Pure apart from `schedule`, which is the page's `setTimeout` unless a test hands in its own.
 */

export const JOB_ID = 7;
/** Between two progress reports: one per step. */
export const STEP_MS = 170;
/** Between two lines of `composer create-project` output. */
export const LOG_MS = 55;

const KIND = "blueprint.apply";
const DOMAIN_OF = (project: string) => `${project}.test`;
const POOL = "php-fpm@8.4";
const SCAFFOLD = "composer create-project laravel/laravel . --no-interaction";

/**
 * Lines of what `composer create-project laravel/laravel . --no-interaction` printed, word for word,
 * run with PHP 8.4 on 2026-10-06 (laravel/laravel v13.10.1); only the project path is the demo's.
 * An excerpt: the per-package lines are left out, and it stops where the key is set.
 */
export const COMPOSER_LOG = [
  'Creating a "laravel/laravel" project at "./"',
  "Installing laravel/laravel (v13.10.1)",
  "Created project in /Users/ada/Sites/blog/.",
  "Loading composer repositories with package information",
  "Updating dependencies",
  "Lock file operations: 109 installs, 0 updates, 0 removals",
  "Writing lock file",
  "Installing dependencies from lock file (including require-dev)",
  "Package operations: 109 installs, 0 updates, 0 removals",
  "Generating optimized autoload files",
  "> @php artisan package:discover --ansi",
  " INFO Discovering packages.",
  "> @php artisan key:generate --ansi",
  " INFO Application key set successfully.",
];

const create = (action: PlanAction): PlanStep => ({ action, disposition: { disposition: "create" }, elevates: false });
const satisfied = (action: PlanAction): PlanStep => ({
  action,
  disposition: { disposition: "satisfied" },
  elevates: false,
});

export function laravelPlan(project: string, root: string): BlueprintPlan {
  return {
    blueprint: "laravel",
    project,
    root,
    source: "builtin",
    trusted: true,
    steps: [
      create({ action: "register_project", name: project, root, pins: { php: "8.4", node: "24", composer: "2" } }),
      // PHP 8.4 is already here (`php-fpm@8.4` runs); Node 24 and Composer 2 are not.
      satisfied({ action: "install_runtime", kind: "php", wanted: "8.4" }),
      create({ action: "install_runtime", kind: "node", wanted: "24" }),
      create({ action: "install_runtime", kind: "composer", wanted: "2" }),
      satisfied({ action: "install_package", package: "mariadb", wanted: null }),
      satisfied({ action: "ensure_service", package: "mariadb", instance: "main", version: null, dedicated: false }),
      create({ action: "create_database", package: "mariadb", instance: "main", database: project, user: project }),
      satisfied({ action: "install_package", package: "redis", wanted: null }),
      satisfied({ action: "ensure_service", package: "redis", instance: "main", version: null, dedicated: false }),
      create({ action: "create_site", kind: { kind: "php-fpm", pool: null }, doc_root: "public", https: true }),
      create({ action: "add_domain", domain: DOMAIN_OF(project), primary: true }),
      create({ action: "issue_certificate", domains: [DOMAIN_OF(project)] }),
      create({ action: "set_php_extension", runtime: null, name: "redis" }),
      create({ action: "run_scaffold", command: SCAFFOLD }),
    ],
  };
}

export function appliedFrom(plan: BlueprintPlan): BlueprintApplied {
  return {
    blueprint: plan.blueprint,
    project: plan.project,
    root: plan.root,
    steps: plan.steps.map((step) => ({
      action: step.action,
      result:
        step.disposition.disposition === "satisfied"
          ? { result: "already_true" }
          : step.action.action === "create_database"
            ? // T202, D2: the one step whose outcome can differ from the plan, shown in the demo.
              { result: "done", note: `the account ${step.action.user} is somebody else's, so this project's is ${step.action.user}-2` }
            : { result: "done" },
    })),
  };
}

/** `steps::describe` in the daemon: the line a job's progress says while a step runs. */
function describe(action: PlanAction): string {
  switch (action.action) {
    case "register_project":
      return `registering the project ${action.name}`;
    case "install_runtime":
      return `installing ${action.kind} ${action.wanted}`;
    case "install_package":
      return action.wanted ? `installing ${action.package} ${action.wanted}` : `installing ${action.package}`;
    case "ensure_service":
      return `making sure of ${action.package}@${action.instance}`;
    case "create_database":
      return `creating the database ${action.database} on ${action.package}`;
    case "create_site":
      return "creating the site";
    case "add_domain":
      return `adding the name ${action.domain}`;
    case "issue_certificate":
      return "issuing the certificate";
    case "set_php_extension":
      return `turning on the PHP extension ${action.name}`;
    case "run_scaffold":
      return "the blueprint's own command";
  }
}

/** What a Tauri `Channel` is to a fixture: something with `onmessage`. */
interface Sink {
  onmessage: (raw: string) => void;
}

export interface ApplyRunner {
  /** `mixengine_watch`: the channel every daemon event goes to, kept for later. */
  watch(channel: Sink): null;
  apply(params: BlueprintApply): BlueprintApplyResponse;
  jobStatus(id: number): JobSummary;
  logsWatch(channel: Sink): null;
  logsUnwatch(): null;
}

export function createApplyRunner({
  registry,
  now,
  schedule = (fn, ms) => void setTimeout(fn, ms),
}: {
  registry: SiteRegistry;
  now: number;
  schedule?: (fn: () => void, ms: number) => void;
}): ApplyRunner {
  let events: Sink | null = null;
  let logs: Sink | null = null;
  let job: JobSummary | null = null;
  const emit = (event: object) => events?.onmessage(JSON.stringify(event));

  function run(plan: BlueprintPlan) {
    const total = plan.steps.length;
    plan.steps.forEach((step, position) => {
      schedule(() => {
        const percent = Math.floor((position * 100) / total);
        const message = describe(step.action);
        job = { ...(job as JobSummary), percent, message };
        emit({ type: "job_progress", job: JOB_ID, percent, message, at: now + position * STEP_MS });
      }, position * STEP_MS);
    });

    // Composer prints while the last step — the scaffold — runs, and the job ends after it has.
    const scaffoldAt = (total - 1) * STEP_MS;
    COMPOSER_LOG.forEach((text, i) => {
      const at = scaffoldAt + i * LOG_MS;
      schedule(() => {
        logs?.onmessage(JSON.stringify({ type: "line", stream: "stdout", at: now + at, text }));
      }, at);
    });

    const end = scaffoldAt + COMPOSER_LOG.length * LOG_MS + STEP_MS;
    schedule(() => {
      const message = "the blueprint's own command has ended";
      emit({ type: "job_progress", job: JOB_ID, percent: 100, message, at: now + end });

      registry.registerProject({
        name: plan.project,
        root: plan.root,
        created_at: new Date(now).toISOString(),
        keep_warm: false,
      });
      registry.create({
        project: { name: plan.project },
        domains: [DOMAIN_OF(plan.project)],
        doc_root: "public",
        https: true,
        kind: { kind: "php-fpm", pool: POOL },
      });

      const result = appliedFrom(plan);
      job = {
        ...(job as JobSummary),
        state: "succeeded",
        percent: 100,
        message,
        finished_at: now + end,
        outcome: { ending: "succeeded", result },
      };
      emit({ type: "job_finished", job: JOB_ID, at: now + end, ending: "succeeded", result });
    }, end);
  }

  return {
    watch(channel) {
      events = channel;
      return null;
    },

    apply(params) {
      const plan = laravelPlan(params.project, params.root);
      if (params.dry_run) return { outcome: "planned", plan, needs: [] };
      job = { id: JOB_ID, kind: KIND, state: "running", percent: 0, message: "", started_at: now };
      run(plan);
      return { outcome: "started", job };
    },

    jobStatus(id) {
      if (job === null || job.id !== id) throw new Error(`demo: no job ${id}`);
      return job;
    },

    logsWatch(channel) {
      logs = channel;
      return null;
    },

    logsUnwatch() {
      logs = null;
      return null;
    },
  };
}
