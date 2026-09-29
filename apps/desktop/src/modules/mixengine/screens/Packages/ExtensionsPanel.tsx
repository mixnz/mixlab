import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";

import Card from "../../../../components/Card";
import EmptyState from "../../../../components/EmptyState";
import ErrorBanner from "../../../../components/ErrorBanner";
import Input from "../../../../components/Input";
import SegmentedControl from "../../../../components/SegmentedControl";
import SwitchTile from "../../../../components/SwitchTile";
import { errorMessage } from "../../../../core/errors";
import { LockIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { RuntimeExtension } from "@mixengine/api";
import type { RuntimeTarget } from "@mixengine/api";
import { poolBanner, type PoolBanner } from "../../runtimeState";
import styles from "./ExtensionsPanel.module.css";

type OnFilter = "all" | "on" | "off";

export default function ExtensionsPanel({
  target,
  leading,
}: {
  target: RuntimeTarget;
  /** Controls at the start of the toolbar row — the PHP extensions screen puts its version picker here. */
  leading?: ReactNode;
}) {
  const [extensions, setExtensions] = useState<RuntimeExtension[]>([]);
  const [banner, setBanner] = useState<{ name: string; kind: PoolBanner } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [filter, setFilter] = useState("");
  const [onFilter, setOnFilter] = useState<OnFilter>("all");
  const { t } = useTranslation();

  // Compiled in and always on: not something to switch, so they are listed apart as chips.
  const optional = useMemo(() => extensions.filter((ext) => ext.linkage !== "static"), [extensions]);
  const builtIn = useMemo(() => extensions.filter((ext) => ext.linkage === "static"), [extensions]);

  const needle = filter.trim().toLowerCase();
  const matches = (ext: RuntimeExtension) => needle === "" || ext.name.toLowerCase().includes(needle);
  const searched = optional.filter(matches);
  const shown = searched.filter(
    (ext) => onFilter === "all" || (onFilter === "on" ? ext.enabled : !ext.enabled),
  );
  const shownBuiltIn = onFilter === "off" ? [] : builtIn.filter(matches);
  const onCount = optional.filter((ext) => ext.enabled).length;

  const reload = useCallback(async () => {
    try {
      setExtensions((await api.runtimeExtensions(target)).extensions);
      setError("");
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }, [t, target]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // Emptied whenever another target is put on screen, so its extensions are not hidden by a
  // search typed against the version before.
  useEffect(() => {
    setFilter("");
  }, [target.kind, target.version]);

  async function toggle(name: string, enabled: boolean) {
    setBusy(name);
    setError("");
    try {
      const result = await api.runtimeSetExtension({ ...target, name, enabled });
      setExtensions((current) =>
        current.map((ext) => (ext.name === name ? result.extension : ext)),
      );
      const kind = poolBanner(result.pool);
      setBanner(kind === "none" ? null : { name, kind });
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className={styles.panel}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <div className={styles.toolbar}>
        {leading}
        <Input
          allowClear
          className={styles.search}
          placeholder={t("mixengine.runtimes.extensions.search")}
          aria-label={t("mixengine.runtimes.extensions.search")}
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          onKeyDown={(e) => {
            if (e.key !== "Escape" || filter === "") return;
            e.preventDefault();
            e.stopPropagation();
            setFilter("");
          }}
        />
        <SegmentedControl
          aria-label={t("mixengine.runtimes.extensions.filter")}
          value={onFilter}
          onChange={setOnFilter}
          segments={[
            { value: "all", label: t("mixengine.runtimes.extensions.filterAll"), count: searched.length },
            {
              value: "on",
              label: t("mixengine.runtimes.extensions.filterOn"),
              count: searched.filter((ext) => ext.enabled).length,
            },
            {
              value: "off",
              label: t("mixengine.runtimes.extensions.filterOff"),
              count: searched.filter((ext) => !ext.enabled).length,
            },
          ]}
        />
      </div>

      {banner && banner.kind === "restartRequired" && (
        <p className={styles.banner}>{t("mixengine.runtimes.extensions.restartRequired")}</p>
      )}
      {banner && banner.kind === "appliesNextStart" && (
        <p className={styles.banner}>{t("mixengine.runtimes.extensions.appliesNextStart")}</p>
      )}

      <Card
        headingLevel={3}
        title={t("mixengine.runtimes.extensions.title", { version: target.version })}
        count={t("mixengine.runtimes.extensions.onCount", { on: onCount, total: optional.length })}
      >
        {shown.length === 0 ? (
          <EmptyState title={t("mixengine.runtimes.extensions.noMatches")} />
        ) : (
          <ul className={styles.grid}>
            {shown.map((ext) => (
              <li key={ext.name}>
                <SwitchTile
                  mono
                  label={ext.name}
                  checked={ext.enabled}
                  disabled={busy === ext.name}
                  onChange={(next) => void toggle(ext.name, next)}
                />
              </li>
            ))}
          </ul>
        )}
      </Card>

      {shownBuiltIn.length > 0 && (
        <Card
          headingLevel={3}
          title={
            <span className={styles.builtInTitle}>
              <LockIcon size={14} />
              {t("mixengine.runtimes.extensions.builtIn")}
            </span>
          }
          description={t("mixengine.runtimes.extensions.builtInAbout")}
        >
          <ul className={styles.chips}>
            {shownBuiltIn.map((ext) => (
              <li key={ext.name} className={styles.chip}>
                {ext.name}
              </li>
            ))}
          </ul>
        </Card>
      )}
    </div>
  );
}
