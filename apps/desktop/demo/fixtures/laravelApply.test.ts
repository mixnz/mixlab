import { readFileSync } from "node:fs";
import type { BlueprintApply, ProjectSummary, SiteDetail, SiteSummary } from "@mixengine/api";
import { describe, expect, it } from "vitest";
import { COMPOSER_LOG, JOB_ID, appliedFrom, createApplyRunner, laravelPlan } from "./laravelApply";
import { createSiteRegistry } from "./siteRegistry";

const MANIFEST = readFileSync(
  new URL("../../../../crates/mixengine-core/src/blueprints/gallery/laravel.toml", import.meta.url),
  "utf8",
);
const SCAFFOLD = /command = "([^"]+)"/.exec(MANIFEST)?.[1];
const ROOT = "/Users/ada/Sites/blog";

const request = (dry_run: boolean): BlueprintApply => ({
  blueprint: "laravel",
  project: "blog",
  root: ROOT,
  root_is_parent: false,
  dry_run,
  front_end: true,
  autostart: false,
});

function setup() {
  const registry = createSiteRegistry({
    sites: [] as SiteSummary[],
    projects: [] as ProjectSummary[],
    fresh: true,
    defaultPool: "php-fpm@8.3",
    detailOf: () => ({}) as SiteDetail,
  });
  const queue: (() => void)[] = [];
  const runner = createApplyRunner({ registry, now: 1_000_000, schedule: (fn) => void queue.push(fn) });
  const flush = () => {
    while (queue.length > 0) queue.shift()?.();
  };
  return { registry, runner, flush };
}

describe("laravelPlan", () => {
  it("is the gallery manifest, in the order the daemon plans it", () => {
    for (const line of ['php = "8.4"', 'node = "24"', 'composer = "2"', 'name = "mariadb"', 'name = "redis"']) {
      expect(MANIFEST).toContain(line);
    }
    expect(MANIFEST).toContain('doc_root = "public"');
    expect(MANIFEST).toContain('extensions = ["redis"]');

    const plan = laravelPlan("blog", ROOT);
    expect(plan.steps.map((s) => s.action.action)).toEqual([
      "register_project",
      "install_runtime",
      "install_runtime",
      "install_runtime",
      "install_package",
      "ensure_service",
      "create_database",
      "install_package",
      "ensure_service",
      "create_site",
      "add_domain",
      "issue_certificate",
      "set_php_extension",
      "run_scaffold",
    ]);
    const runtimes = plan.steps.flatMap((s) => (s.action.action === "install_runtime" ? [[s.action.kind, s.action.wanted]] : []));
    expect(runtimes).toEqual([
      ["php", "8.4"],
      ["node", "24"],
      ["composer", "2"],
    ]);
    const scaffold = plan.steps.at(-1)?.action;
    expect(scaffold).toEqual({ action: "run_scaffold", command: SCAFFOLD });
    expect(plan.steps.find((s) => s.action.action === "create_site")?.action).toEqual({
      action: "create_site",
      kind: { kind: "php-fpm", pool: null },
      doc_root: "public",
      https: true,
    });
    expect(plan.steps.find((s) => s.action.action === "add_domain")?.action).toEqual({
      action: "add_domain",
      domain: "blog.test",
      primary: true,
    });
    expect(plan.trusted).toBe(true);
  });
});

describe("createApplyRunner", () => {
  it("plans on a dry run and starts a job on the real one", () => {
    const { runner } = setup();
    expect(runner.apply(request(true))).toEqual({ outcome: "planned", plan: laravelPlan("blog", ROOT), needs: [] });
    const started = runner.apply(request(false));
    expect(started.outcome).toBe("started");
    if (started.outcome === "started") expect(started.job).toMatchObject({ id: JOB_ID, state: "running" });
  });

  it("reports progress then the result on the channel it was given before the apply", () => {
    const { registry, runner, flush } = setup();
    const events: { type: string; percent?: number; message?: string; ending?: string; result?: unknown }[] = [];
    runner.watch({ onmessage: (raw: string) => events.push(JSON.parse(raw)) });
    runner.apply(request(false));
    flush();

    const progress = events.filter((e) => e.type === "job_progress");
    // One per step, as the daemon reports them (`position * 100 / total`), the database's note at
    // its step's percent, then the scaffold's end.
    expect(progress.map((e) => e.percent)).toEqual([0, 7, 14, 21, 28, 35, 42, 42, 50, 57, 64, 71, 78, 85, 92, 100]);
    expect(progress[7]).toMatchObject({ message: "the account blog is somebody else's, so this project's is blog-2" });
    expect(progress[0]).toMatchObject({ message: "registering the project blog" });
    expect(progress.at(-1)).toMatchObject({ message: "the blueprint's own command has ended" });
    const finished = events.at(-1);
    expect(finished).toMatchObject({ type: "job_finished", job: JOB_ID, ending: "succeeded" });
    expect(finished?.result).toEqual(appliedFrom(laravelPlan("blog", ROOT)));

    expect(registry.list("blog").sites.map((s) => [s.domain, s.doc_root, s.https])).toEqual([["blog.test", "public", true]]);
    expect(runner.jobStatus(JOB_ID).outcome?.ending).toBe("succeeded");
  });

  it("narrates every step into the job's log, then composer's output, then the end", () => {
    const { runner, flush } = setup();
    const lines: { type: string; stream: string; text: string }[] = [];
    runner.apply(request(false));
    runner.logsWatch({ onmessage: (raw: string) => lines.push(JSON.parse(raw)) });
    flush();
    const texts = lines.map((l) => l.text);
    expect(texts.slice(0, 2)).toEqual(["registering the project blog", "installing php 8.4"]);
    const database = texts.indexOf("creating the database blog on mariadb");
    expect(texts[database + 1]).toBe("the account blog is somebody else's, so this project's is blog-2");
    const scaffold = texts.indexOf("the blueprint's own command");
    expect(texts.slice(scaffold + 1, scaffold + 1 + COMPOSER_LOG.length)).toEqual(COMPOSER_LOG);
    expect(texts.at(-1)).toBe("the apply has finished");
    expect(texts).toHaveLength(14 + 1 + COMPOSER_LOG.length + 1);
    expect(lines.every((l) => l.type === "line" && l.stream === "stdout")).toBe(true);
  });
});

describe("appliedFrom", () => {
  it("reports what was already there as already true and the rest as done", () => {
    const applied = appliedFrom(laravelPlan("blog", ROOT));
    expect(applied.steps.filter((s) => s.result.result === "already_true")).toHaveLength(5);
    expect(applied.steps.filter((s) => s.result.result === "done")).toHaveLength(9);
    expect(applied).toMatchObject({ blueprint: "laravel", project: "blog", root: ROOT });
  });
});
