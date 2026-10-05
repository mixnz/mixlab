import type {
  ProjectList,
  ProjectSummary,
  SiteCreate,
  SiteCreation,
  SiteDetail,
  SiteKind,
  SiteList,
  SiteSummary,
} from "@mixengine/api";

/**
 * The sites one page sees, and the one thing a clip does to them: create another.
 *
 * Pure, so it is tested in node. It lives as long as the page, which is what keeps every scene and
 * every clip starting from the same list.
 */
export interface SiteRegistryOptions {
  sites: SiteSummary[];
  projects: ProjectSummary[];
  /** Domains this page starts without — the ones a clip is about to create. */
  without?: string[];
  /** A machine with no project and no site yet, for a clip that builds the first one. */
  fresh?: boolean;
  /** What a php-fpm site gets when its create names no pool: what the daemon would resolve. */
  defaultPool: string;
  /** The detail of a site the page started with, exactly as the screenshots have always seen it. */
  detailOf: (site: SiteSummary) => SiteDetail;
}

export interface SiteRegistry {
  list(project?: string | null): SiteList;
  detail(domain: string): SiteDetail;
  create(params: SiteCreate): SiteCreation;
  projects(): ProjectList;
  /** What a blueprint apply does first; a name already registered is left as it is. */
  registerProject(summary: ProjectSummary): void;
}

export function createSiteRegistry(options: SiteRegistryOptions): SiteRegistry {
  const without = new Set(options.without ?? []);
  const initial = options.fresh ? [] : options.sites.filter((site) => !without.has(site.domain));
  const projects = options.fresh ? [] : [...options.projects];
  const created = new Map<string, SiteDetail>();

  const all = (): SiteSummary[] => [...initial, ...[...created.values()].map((detail) => detail.site)];

  function rootOf(name: string): string {
    const project = projects.find((p) => p.name === name);
    if (project === undefined) throw new Error(`demo: no project named ${name}`);
    return project.root;
  }

  return {
    list(project) {
      const sites = all();
      if (!project) return { sites };
      return { sites: sites.filter((site) => site.owner.type === "project" && site.owner.name === project) };
    },

    projects() {
      return { projects: [...projects] };
    },

    registerProject(summary) {
      if (!projects.some((p) => p.name === summary.name)) projects.push(summary);
    },

    detail(domain) {
      const made = created.get(domain);
      if (made !== undefined) return made;
      return options.detailOf(initial.find((site) => site.domain === domain) ?? initial[0]);
    },

    create(params) {
      if (!("name" in params.project)) throw new Error("demo: sites are created under a project by name");
      const name = params.project.name;
      const root = rootOf(name);
      const domains = params.domains ?? [`${name}.test`];
      const domain = domains[0];
      if (all().some((site) => site.domain === domain)) throw new Error(`demo: ${domain} is already declared`);

      const asked: SiteKind = params.kind ?? { kind: "php-fpm" };
      const kind: SiteKind =
        asked.kind === "php-fpm" ? { kind: "php-fpm", pool: asked.pool ?? options.defaultPool } : asked;
      const docRoot = params.doc_root ?? "";
      const https = params.https ?? false;
      const site: SiteSummary = {
        domain,
        owner: { type: "project", name },
        kind,
        doc_root: docRoot,
        https,
        // No HTTPS address, nothing to redirect to — the daemon refuses the pair; the demo drops it.
        https_redirect: https && (params.https_redirect ?? false),
        state: "enabled",
      };
      const pool = kind.kind === "php-fpm" ? (kind.pool ?? null) : null;
      const detail: SiteDetail = {
        site,
        root,
        doc_root_full: docRoot === "" ? root : `${root}/${docRoot}`,
        doc_root_exists: true,
        domains,
        pool: kind.kind === "php-fpm" ? { declared: pool, resolved: pool } : null,
        services: [],
      };
      created.set(domain, detail);
      return { site: detail };
    },
  };
}
