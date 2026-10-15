import { useCallback, useEffect, useRef, useState } from "react";
import type { DatabaseClientReport, DatabaseCredentials } from "@mixengine/api";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { ServiceRow } from "../../daemonState";

interface Options {
  rows: ServiceRow[];
  /** Rereads the services: a reset stops and restarts some of them. */
  reload: () => Promise<void>;
  setError: (message: string) => void;
}

/** What each service answers to `database.client`, and the password dialog and reset that the
 *  database rows' menu offers. */
export function useDatabaseClients({ rows, reload, setError }: Options) {
  const { t } = useTranslation();
  /**
   * `database.client` for each service, looked up **once per id** and then remembered.
   *
   * **It has to be known before the click, so it cannot be looked up when the menu opens.** The ⋮
   * button of a service that is not a database has to be greyed out from the moment it is drawn —
   * opening an empty menu and only then finding out is worse than not inviting the click.
   *
   * One lookup per id is acceptable because **the answer never changes**: it is the recipe's
   * protocol, i.e. a property of the package this service runs. So the first `service.list` read
   * pays N lookups, and every `reload()` after that — every return to the tab, every `resync` —
   * pays 0.
   *
   * An id being absent means not finished asking *or* asked and failed, and both draw a greyed-out
   * button.
   */
  const [databases, setDatabases] = useState<Record<string, DatabaseClientReport>>({});
  /** The ids whose question has been sent, so `rows` changing with the stream does not turn into a
   *  barrage of RPCs. */
  const asked = useRef(new Set<string>());
  /** The password shown in the dialog, or `null`. Never kept in `rows`. */
  const [credentials, setCredentials] = useState<DatabaseCredentials | null>(null);
  /** The service being asked "reset the password?", or `null`. */
  const [resetTarget, setResetTarget] = useState<string | null>(null);

  /**
   * Asks `database.client` for the ids never asked about.
   *
   * **`ServiceSummary` cannot answer this.** `ServiceRole` only tells the front end from everything
   * else, and [ADR 0026] forbids the client from inferring a role from the package name — so
   * `database.client` is the only way. Driven by `rows` because that is where a new service shows
   * up.
   *
   * A failed question **removes the id from `asked`**: the next `reload()` asks again. Keeping it
   * would let a passing glitch lock that row's ⋮ button until the window is closed.
   *
   * [ADR 0026]: https://github.com/mixnz/mixlab/blob/master/docs/decisions/0026-the-active-front-end-is-a-row-and-switching-it-is-a-job.md
   */
  useEffect(() => {
    const missing = rows.map((row) => row.id).filter((id) => !asked.current.has(id));
    if (missing.length === 0) return;
    for (const id of missing) asked.current.add(id);

    void (async () => {
      const answers = await Promise.all(
        missing.map(async (id) => {
          try {
            return [id, await api.databaseClient(id)] as const;
          } catch {
            asked.current.delete(id);
            return [id, null] as const;
          }
        }),
      );
      setDatabases((current) => {
        const next = { ...current };
        for (const [id, report] of answers) if (report !== null) next[id] = report;
        return next;
      });
    })();
  }, [rows]);

  /** Fetches a service's administrator password and opens the dialog. */
  const showCredentials = useCallback(
    async (id: string) => {
      try {
        setCredentials(await api.databaseCredentials(id));
      } catch (e) {
        setError(errorMessage(t, e));
      }
    },
    [setError, t],
  );

  /** Runs `service.reset_credential`, then rereads — it stops and restarts several services. */
  const resetCredential = useCallback(
    async (id: string) => {
      try {
        await api.serviceResetCredential(id);
      } catch (e) {
        setError(errorMessage(t, e));
      } finally {
        await reload();
      }
    },
    [reload, setError, t],
  );

  return {
    databases,
    credentials,
    setCredentials,
    showCredentials,
    resetTarget,
    setResetTarget,
    resetCredential,
  };
}
