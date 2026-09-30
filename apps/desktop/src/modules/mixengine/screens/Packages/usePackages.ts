import { useCallback, useEffect, useRef, useState } from "react";

import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { PackageRelease } from "@mixengine/api";
import type { PackageSummary } from "@mixengine/api";
import type { PackageFoundList } from "@mixengine/api";
import type { PackageUpdate, UpgradePlan } from "@mixengine/api";
import type { CatalogueGap } from "@mixengine/api";
import { applyJob, type JobRow } from "../../daemonState";
import { subscribeDaemonWatch } from "../../daemonWatch";
import {
  askingStep,
  needLabel,
  requirementStep,
  splitLibraries,
  type AskingStep,
} from "../../requirementStep";
import { jobFinished, versionKey } from "../../runtimeState";

/** Everything `Packages.tsx` needs to draw a group, and nothing more. */
export interface PackagesState {
  installed: PackageSummary[];
  available: PackageRelease[];
  /** False until the first read has answered — until then both lists being empty means "not known
   *  yet", not "nothing installed". */
  loaded: boolean;
  stale: boolean;
  /** What the package index could not be read for (T196); empty when it read everything. */
  unavailable: CatalogueGap[];
  jobs: JobRow[];
  installingJob: Record<string, number>;
  error: string;
  clearError: () => void;
  /** What an install said about this machine and went on anyway — T27e. */
  notice: string;
  clearNotice: () => void;
  install: (release: PackageRelease) => Promise<void>;
  uninstall: (target: PackageSummary) => Promise<void>;
  /** The one question an install is waiting on — T151. */
  asking: { release: PackageRelease; step: AskingStep } | null;
  agree: () => Promise<void>;
  chooseInstead: (version: string) => Promise<void>;
  dismissAsking: () => void;
  /** Package directories on disk with no row, as `package.found` answers them — T182i. */
  onDisk: PackageFoundList;
  /** The `name@version` being adopted now. */
  adopting: string | null;
  adopt: (pkg: string, version: string) => Promise<void>;
  /** Each installed version whose line has a newer release — T193c. */
  updates: PackageUpdate[];
  /** The update being asked about, with the daemon's plan for it. */
  upgrading: { update: PackageUpdate; plan: UpgradePlan } | null;
  askToUpgrade: (update: PackageUpdate) => Promise<void>;
  upgrade: (update: PackageUpdate, keep: boolean) => Promise<void>;
  dismissUpgrade: () => void;
  /** What the new version lacks, asked as an install asks it — T151. */
  askingUpgrade: { update: PackageUpdate; keep: boolean; step: AskingStep } | null;
  agreeUpgrade: () => Promise<void>;
  dismissAskingUpgrade: () => void;
}

/**
 * `package.*`'s state for the whole group tab strip (Web servers / Databases / Cache & queues /
 * Other). It lives in `Packages.tsx`, not in each tab: the four groups are four display slices of
 * exactly one `package.list_installed`/`package.list_available` pair, so it is called once and then
 * filtered — not four components asking the daemon the same question. It is also why switching
 * groups does not lose track of a job that is installing: `installingJob` lives here, above every
 * tab.
 */
export function usePackages(active: boolean): PackagesState {
  const [installed, setInstalled] = useState<PackageSummary[]>([]);
  const [available, setAvailable] = useState<PackageRelease[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [onDisk, setOnDisk] = useState<PackageFoundList>({ found: [] });
  const [adopting, setAdopting] = useState<string | null>(null);
  const [stale, setStale] = useState(false);
  const [unavailable, setUnavailable] = useState<CatalogueGap[]>([]);
  const [jobs, setJobs] = useState<JobRow[]>([]);
  const [installingJob, setInstallingJob] = useState<Record<string, number>>({});
  const [error, setError] = useState("");
  // What an install said about this machine and went on anyway — T27e.
  const [notice, setNotice] = useState("");
  const [asking, setAsking] = useState<{ release: PackageRelease; step: AskingStep } | null>(null);
  const [updates, setUpdates] = useState<PackageUpdate[]>([]);
  const [upgrading, setUpgrading] = useState<{ update: PackageUpdate; plan: UpgradePlan } | null>(
    null,
  );
  const [askingUpgrade, setAskingUpgrade] = useState<{
    update: PackageUpdate;
    keep: boolean;
    step: AskingStep;
  } | null>(null);
  const { t } = useTranslation();

  // The same reason `Languages.tsx` follows: read the latest `installingJob` inside the `watch`
  // callback registered once, rather than registering the watch again every time that map changes.
  const installingJobRef = useRef(installingJob);
  useEffect(() => {
    installingJobRef.current = installingJob;
  }, [installingJob]);

  // `stillShow` is the error sentence that has to survive this reread. A failed install job still
  // has to be reported even if the read right after it answers normally: being able to read again
  // does not mean the install finished. Empty — the default — is "once read, the screen is clean",
  // just as before.
  const reload = useCallback(
    async (stillShow = "") => {
      try {
        const [inst, avail, found] = await Promise.all([
          api.packagesInstalled(),
          api.packagesAvailable(),
          api.packagesFound(),
        ]);
        setInstalled(inst.packages);
        setOnDisk(found);
        setAvailable(avail.packages);
        setStale(avail.stale);
        setUnavailable(avail.unavailable ?? []);
        setUpdates(avail.updates ?? []);
        setError(stillShow);
      } catch (e) {
        setError(errorMessage(t, e));
      } finally {
        setLoaded(true);
      }
    },
    [t],
  );

  // Reread on mount and every time we come back to this screen — the same reason as
  // `Languages.tsx`/`Dashboard.tsx`. Runs even while on the Languages tab: the tab strip above has
  // to know right away whether any package falls into "Other", and it can only know after this
  // read.
  useEffect(() => {
    if (active) void reload();
  }, [active, reload]);

  useEffect(() => {
    return subscribeDaemonWatch((raw) => {
      setJobs((current) => applyJob(current, raw));
      // The job being followed just finished: reread "installed"/"available" — no other news
      // reports this; see `Languages.tsx`.
      const finished = jobFinished(raw);
      if (finished !== null && Object.values(installingJobRef.current).includes(finished.id)) {
        // For a failed job, `job_finished` is the only place that says why — see `jobFinished`.
        // The reread still has to run (a job that failed halfway may still have changed
        // something), but it must not wipe out the error sentence that just arrived.
        void reload(finished.error === null ? "" : errorMessage(t, finished.error));
        setInstallingJob((current) => {
          const next = { ...current };
          for (const key of Object.keys(next)) {
            if (next[key] === finished.id) delete next[key];
          }
          return next;
        });
      }
    });
  }, [reload, t]);

  const start = useCallback(
    async (name: string, version: string, installPrerequisites: boolean) => {
      setError("");
      try {
        const job = await api.packageInstall({
          package: name,
          version,
          install_prerequisites: installPrerequisites,
        });
        setInstallingJob((current) => ({ ...current, [versionKey(name, version)]: job.id }));
      } catch (e) {
        setError(errorMessage(t, e));
      }
    },
    // `t` changes when the person switches language; the error has to follow it.
    [t],
  );

  // What the machine lacks is asked about before the download starts — T151.
  const install = useCallback(
    async (release: PackageRelease) => {
      setError("");
      let unmet;
      try {
        ({ unmet } = await api.packageRequirements({
          package: release.package,
          version: release.version,
        }));
      } catch {
        // A question the daemon cannot answer asks nothing: the install reports the same failure
        // in its own words, as `mix` does.
        await start(release.package, release.version, false);
        return;
      }

      const step = requirementStep(unmet);
      if (step.kind === "proceed") {
        await start(release.package, release.version, false);
        return;
      }

      // **Said, not asked** — T27e, the same rule `Languages` follows and `mix` follows.
      if (step.kind === "notice") {
        setNotice(
          t("mixengine.requirements.librariesNotice", {
            name: `${release.package} ${release.version}`,
            libraries: splitLibraries(step.needs).libraries.join(", "),
          }),
        );
        await start(release.package, release.version, false);
        return;
      }

      const asked = askingStep(step);
      if (asked === null) {
        setError(
          t("mixengine.requirements.unavailable", {
            name: `${release.package} ${release.version}`,
            needs: step.needs.map(needLabel).join(", "),
          }),
        );
        return;
      }
      setAsking({ release, step: asked });
    },
    [start, t],
  );

  const agree = useCallback(async () => {
    if (asking === null) return;
    setAsking(null);
    await start(asking.release.package, asking.release.version, true);
  }, [asking, start]);

  const chooseInstead = useCallback(
    async (version: string) => {
      if (asking === null) return;
      setAsking(null);
      await start(asking.release.package, version, false);
    },
    [asking, start],
  );

  const dismissAsking = useCallback(() => setAsking(null), []);

  /** There is no `force` — refusing because `services` is not empty is final (D6). Draw the list
   *  and stop there. */
  const uninstall = useCallback(
    async (target: PackageSummary) => {
      setError("");
      try {
        await api.packageUninstall({ package: target.package, version: target.version });
        void reload();
      } catch (e) {
        setError(errorMessage(t, e));
      }
    },
    [reload, t],
  );

  const adopt = useCallback(
    async (pkg: string, version: string) => {
      setAdopting(`${pkg}@${version}`);
      setError("");
      try {
        await api.packageAdopt(pkg, version);
        void reload();
      } catch (e) {
        setError(errorMessage(t, e));
      } finally {
        setAdopting(null);
      }
    },
    [reload, t],
  );

  // T193c: the plan first, then what the new version lacks (T151), then the job — as `Languages`.
  const askToUpgrade = useCallback(
    async (update: PackageUpdate) => {
      setError("");
      try {
        const plan = await api.packageUpgradePlan({
          package: update.package,
          from: update.from,
          to: update.to,
        });
        setUpgrading({ update, plan });
      } catch (e) {
        setError(errorMessage(t, e));
      }
    },
    [t],
  );

  const startUpgrade = useCallback(
    async (update: PackageUpdate, keep: boolean, installPrerequisites: boolean) => {
      setError("");
      try {
        const job = await api.packageUpgrade({
          package: update.package,
          from: update.from,
          to: update.to,
          keep,
          install_prerequisites: installPrerequisites,
        });
        setInstallingJob((current) => ({
          ...current,
          [versionKey(update.package, update.from)]: job.id,
        }));
      } catch (e) {
        setError(errorMessage(t, e));
      }
    },
    [t],
  );

  const upgrade = useCallback(
    async (update: PackageUpdate, keep: boolean) => {
      setUpgrading(null);
      const step = requirementStep(update.needs ?? []);
      if (step.kind === "proceed" || step.kind === "notice") {
        await startUpgrade(update, keep, false);
        return;
      }

      // Only agreeing to install what MixEngine can install continues an update; which release
      // of the line to go to instead is the version list's question.
      const asked = askingStep(step);
      if (asked === null || asked.kind !== "consent") {
        setError(
          t("mixengine.requirements.unavailable", {
            name: `${update.package} ${update.to}`,
            needs: step.needs.map(needLabel).join(", "),
          }),
        );
        return;
      }
      setAskingUpgrade({ update, keep, step: asked });
    },
    [startUpgrade, t],
  );

  const agreeUpgrade = useCallback(async () => {
    if (askingUpgrade === null) return;
    setAskingUpgrade(null);
    await startUpgrade(askingUpgrade.update, askingUpgrade.keep, true);
  }, [askingUpgrade, startUpgrade]);

  const dismissUpgrade = useCallback(() => setUpgrading(null), []);
  const dismissAskingUpgrade = useCallback(() => setAskingUpgrade(null), []);

  const clearError = useCallback(() => setError(""), []);
  const clearNotice = useCallback(() => setNotice(""), []);

  return {
    installed,
    available,
    loaded,
    stale,
    unavailable,
    jobs,
    installingJob,
    error,
    clearError,
    notice,
    clearNotice,
    install,
    uninstall,
    asking,
    agree,
    chooseInstead,
    dismissAsking,
    onDisk,
    adopting,
    adopt,
    updates,
    upgrading,
    askToUpgrade,
    upgrade,
    dismissUpgrade,
    askingUpgrade,
    agreeUpgrade,
    dismissAskingUpgrade,
  };
}
