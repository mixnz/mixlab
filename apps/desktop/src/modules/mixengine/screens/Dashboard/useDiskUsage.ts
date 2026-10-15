import { useCallback, useState } from "react";
import type { DiskUsage } from "@mixengine/api";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";

/** The disk usage panel's reading: the copy the daemon keeps, and the walk Refresh asks for. */
export function useDiskUsage({ setError }: { setError: (message: string) => void }) {
  const { t } = useTranslation();
  const [disk, setDisk] = useState<DiskUsage | null>(null);
  const [refreshingDisk, setRefreshingDisk] = useState(false);

  /**
   * The disk usage panel's reading — the copy the daemon keeps, up to a minute old.
   *
   * **Never awaited by `reload()`.** When the daemon holds no copy — the first read after it starts,
   * or a return to the tab more than a minute later — this is a walk of the whole home, measured at
   * twenty seconds on a Windows home of 80,000 files. Joined to the service table's read, it held
   * that table on a spinner the whole time for the sake of a panel at the bottom of the page.
   */
  const readDisk = useCallback(async () => {
    try {
      setDisk(await api.diskUsage(false));
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }, [setError, t]);

  /** The disk usage table's "Refresh" button — `refresh: true` walks the disk again, unlike
   *  `readDisk()` above, which reads the copy the daemon keeps (up to a minute old) so that every
   *  return to the tab does not become a disk walk. */
  const refreshDisk = useCallback(async () => {
    setRefreshingDisk(true);
    try {
      setDisk(await api.diskUsage(true));
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setRefreshingDisk(false);
    }
  }, [setError, t]);

  return { disk, refreshingDisk, readDisk, refreshDisk };
}
