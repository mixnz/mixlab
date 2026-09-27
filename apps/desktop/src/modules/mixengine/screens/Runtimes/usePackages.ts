import { useCallback, useEffect, useRef, useState } from "react";

import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { PackageRelease } from "@mixengine/api";
import type { PackageSummary } from "@mixengine/api";
import type { PackageFoundList } from "@mixengine/api";
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

/** Tất cả những gì `Packages.tsx` cần để vẽ một nhóm, và không hơn. */
export interface PackagesState {
  installed: PackageSummary[];
  available: PackageRelease[];
  stale: boolean;
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
}

/**
 * State của `package.*` cho cả dải tab nhóm (Máy chủ web / Cơ sở dữ liệu / Cache & hàng đợi /
 * Khác). Nằm ở `Runtimes.tsx`, không nằm trong từng tab: bốn nhóm là bốn lát cắt hiển thị của
 * đúng một cặp `package.list_installed`/`package.list_available`, nên gọi một lần rồi lọc — chứ
 * không phải bốn component cùng hỏi daemon một câu. Nó cũng là lý do đổi nhóm không làm mất dấu
 * một job đang cài: `installingJob` sống ở đây, cao hơn mọi tab.
 */
export function usePackages(active: boolean): PackagesState {
  const [installed, setInstalled] = useState<PackageSummary[]>([]);
  const [available, setAvailable] = useState<PackageRelease[]>([]);
  const [onDisk, setOnDisk] = useState<PackageFoundList>({ found: [] });
  const [adopting, setAdopting] = useState<string | null>(null);
  const [stale, setStale] = useState(false);
  const [jobs, setJobs] = useState<JobRow[]>([]);
  const [installingJob, setInstallingJob] = useState<Record<string, number>>({});
  const [error, setError] = useState("");
  // What an install said about this machine and went on anyway — T27e.
  const [notice, setNotice] = useState("");
  const [asking, setAsking] = useState<{ release: PackageRelease; step: AskingStep } | null>(null);
  const { t } = useTranslation();

  // Cùng lý do `Languages.tsx` đã theo: đọc `installingJob` mới nhất trong callback `watch` đăng
  // ký một lần, không đăng ký lại watch mỗi lần map đó đổi.
  const installingJobRef = useRef(installingJob);
  useEffect(() => {
    installingJobRef.current = installingJob;
  }, [installingJob]);

  // `stillShow` là câu lỗi phải sống sót qua lần đọc lại này. Một job cài hỏng vẫn phải được kể
  // lại dù lần đọc ngay sau đó trả lời bình thường: đọc lại được không có nghĩa là việc cài đã
  // xong. Rỗng — mặc định — là "đọc xong thì màn hình sạch", đúng như trước.
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
        setError(stillShow);
      } catch (e) {
        setError(errorMessage(t, e));
      }
    },
    [t],
  );

  // Đọc lại lúc mount và mỗi lần vừa quay lại màn này — cùng lý do `Languages.tsx`/`Dashboard.tsx`.
  // Chạy kể cả khi đang đứng ở tab Ngôn ngữ: dải tab cấp trên phải biết ngay có package nào rơi
  // vào nhóm "Khác" hay không, và nó chỉ biết được sau lần đọc này.
  useEffect(() => {
    if (active) void reload();
  }, [active, reload]);

  useEffect(() => {
    return subscribeDaemonWatch((raw) => {
      setJobs((current) => applyJob(current, raw));
      // Job đang theo dõi vừa xong: đọc lại "đã cài"/"có thể cài" — không có tin nào khác báo
      // chuyện này, xem `Languages.tsx`.
      const finished = jobFinished(raw);
      if (finished !== null && Object.values(installingJobRef.current).includes(finished.id)) {
        // Job hỏng thì `job_finished` là chỗ duy nhất nói ra vì sao — xem `jobFinished`. Đọc lại
        // vẫn phải chạy (một job hỏng nửa chừng vẫn có thể đã đổi thứ gì đó), nhưng nó không được
        // xoá mất câu lỗi vừa tới.
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

  /** Không có `force` — refuse vì `services` không rỗng là chốt (D6). Vẽ danh sách, dừng ở đó. */
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

  const clearError = useCallback(() => setError(""), []);
  const clearNotice = useCallback(() => setNotice(""), []);

  return {
    installed,
    available,
    stale,
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
  };
}
