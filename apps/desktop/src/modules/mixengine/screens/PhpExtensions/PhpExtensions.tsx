import { useCallback, useEffect, useMemo, useState } from "react";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import EmptyState from "../../../../components/EmptyState";
import ErrorBanner from "../../../../components/ErrorBanner";
import PageHeader from "../../../../components/PageHeader";
import Select from "../../../../components/Select";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { RuntimeSummary } from "@mixengine/api";
import type { RuntimeTarget } from "@mixengine/api";
import ExtensionsPanel from "../Packages/ExtensionsPanel";
import styles from "./PhpExtensions.module.css";

/**
 * Turning PHP extensions on and off — T118.
 *
 * **No new method.** `runtime.list_extensions` and `runtime.set_extension` have existed since T28,
 * and `ExtensionsPanel` has drawn them since the Runtimes screen (now Packages) existed. What was
 * missing was *the way there*: it sat behind a version row that had to be expanded on a screen then
 * called Runtimes, four rows above a sidebar item called *Extensions* that was about something else
 * entirely.
 *
 * **The same component, drawn in two places**, not two copies: opening it from Packages still
 * works, and a second copy would drift apart on exactly the day one of them gets changed.
 *
 * **No PHP means one sentence and one button**, not an empty table: an empty table makes people
 * guess which step they are missing.
 */
export default function PhpExtensions({
  active,
  onInstallPhp,
}: {
  active: boolean;
  onInstallPhp: () => void;
}) {
  const [installed, setInstalled] = useState<RuntimeSummary[] | null>(null);
  const [version, setVersion] = useState("");
  const [error, setError] = useState("");
  const { t } = useTranslation();

  /**
   * Keeps the same reference as long as `version` has not changed.
   *
   * `ExtensionsPanel` has `target` in `reload`'s deps, so an object literal built inline in JSX is
   * a new `target` **on every render** — and `MixEngineTab` re-renders every mounted pane on each
   * screen switch. That means every screen switch costs an extra `runtime.list_extensions` asking
   * again exactly what was just asked.
   */
  const target = useMemo<RuntimeTarget>(() => ({ kind: "php", version }), [version]);

  const reload = useCallback(async () => {
    try {
      const listed = await api.runtimesInstalled("php");
      setInstalled(listed.runtimes);
      // The home's default version, because that is the `php` the terminal runs; failing that, the
      // first one.
      setVersion((current) => {
        if (listed.runtimes.some((runtime) => runtime.version === current)) return current;
        const preferred = listed.runtimes.find((runtime) => runtime.default) ?? listed.runtimes[0];
        return preferred?.version ?? "";
      });
      setError("");
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }, [t]);

  /**
   * Rereads every time we come back to this screen, not only on mount — `MixEngineTab.pane` keeps
   * every screen mounted and only hides it, so "mounted" does not mean "just looked at".
   *
   * This is the only screen that ever skipped that agreement, and the cost was exactly one bug:
   * removing the selected PHP version on the Packages screen left the `Select` here holding it, and
   * `ExtensionsPanel` asked the daemon about a runtime that no longer exists ("no such runtime: php
   * …"). The rule for picking another version when the selected one disappears was already in
   * `reload()` — all that was missing was one more call.
   */
  useEffect(() => {
    if (active) void reload();
  }, [active, reload]);

  return (
    <div className={`mixengine-page ${styles.screen}`}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <PageHeader title={t("mixengine.phpExtensions.title")} description={t("mixengine.phpExtensions.intro")} />

      {installed !== null && installed.length === 0 ? (
        <Card>
          <EmptyState
            title={t("mixengine.phpExtensions.noPhp")}
            action={
              <Button variant="primary" onClick={onInstallPhp}>
                {t("mixengine.phpExtensions.installPhp")}
              </Button>
            }
          />
        </Card>
      ) : (
        version !== "" && (
          <ExtensionsPanel
            target={target}
            leading={
              <>
                <Select
                  className={styles.select}
                  value={version}
                  onChange={setVersion}
                  ariaLabel={t("mixengine.phpExtensions.version")}
                  options={(installed ?? []).map((runtime) => ({
                    value: runtime.version,
                    label: runtime.default
                      ? `PHP ${runtime.version} — ${t("mixengine.phpExtensions.isDefault")}`
                      : `PHP ${runtime.version}`,
                  }))}
                />
                <Button variant="link" onClick={onInstallPhp}>
                  {t("mixengine.phpExtensions.installAnother")}
                </Button>
              </>
            }
          />
        )
      )}
    </div>
  );
}