import { useCallback, useEffect, useState } from "react";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import EmptyState from "../../../../components/EmptyState";
import ErrorBanner from "../../../../components/ErrorBanner";
import Input from "../../../../components/Input";
import MonogramBadge from "../../../../components/MonogramBadge";
import PageHeader from "../../../../components/PageHeader";
import StatusPill from "../../../../components/StatusPill";
import { PlusIcon, UploadIcon } from "../../../../icons";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { BlueprintApplied } from "@mixengine/api";
import type { BlueprintSummary } from "@mixengine/api";
import AfterApply from "../../components/AfterApply";
import CaptureDialog from "./CaptureDialog";
import ImportDialog from "./ImportDialog";
import ApplyDialog from "./ApplyDialog";
import styles from "./Blueprints.module.css";

/** The blueprint list of this home — capture, import, apply. There is no edit/delete
 *  (`blueprint.delete` does not exist) — overwriting on capture/import is the only way to replace a
 *  slug. */
export default function Blueprints({ active }: { active: boolean }) {
  const [rows, setRows] = useState<BlueprintSummary[]>([]);
  const [error, setError] = useState("");
  const [search, setSearch] = useState("");
  const [capturing, setCapturing] = useState(false);
  const [importing, setImporting] = useState(false);
  const [applying, setApplying] = useState<BlueprintSummary | null>(null);
  /** The apply that just finished — the ask-for-rights → start → open-site chain running for this
   *  project. */
  const [settling, setSettling] = useState<BlueprintApplied | null>(null);
  const { t } = useTranslation();

  const reload = useCallback(async () => {
    try {
      const list = await api.blueprints();
      setRows(list.blueprints);
      setError("");
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }, [t]);

  // Reread on mount and every time we come back to this screen — the same reason as
  // `Dashboard.tsx`.
  useEffect(() => {
    if (active) void reload();
  }, [active, reload]);

  function sourceLabel(row: BlueprintSummary): string {
    if (row.source === "builtin") return t("mixengine.blueprints.sourceBuiltin");
    if (row.source === "captured") return t("mixengine.blueprints.sourceCaptured");
    return t("mixengine.blueprints.sourceImported");
  }

  const needle = search.trim().toLowerCase();
  const shown = rows.filter(
    (row) =>
      needle === "" ||
      row.name.toLowerCase().includes(needle) ||
      row.description.toLowerCase().includes(needle),
  );

  return (
    <div className={`mixengine-page ${styles.blueprints}`}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <PageHeader
        title={t("mixengine.sidebar.blueprints")}
        description={t("mixengine.blueprints.about")}
        actions={
          <>
            <Button size="large" onClick={() => setImporting(true)}>
              <UploadIcon size={15} />
              {t("mixengine.blueprints.importButton")}
            </Button>
            <Button size="large" variant="primary" onClick={() => setCapturing(true)}>
              <PlusIcon size={15} />
              {t("mixengine.blueprints.newButton")}
            </Button>
          </>
        }
      />

      <Input
        allowClear
        className={styles.search}
        placeholder={t("mixengine.blueprints.search")}
        aria-label={t("mixengine.blueprints.search")}
        value={search}
        onChange={(e) => setSearch(e.target.value)}
      />

      {shown.length === 0 ? (
        <Card>
          <EmptyState
            title={rows.length === 0 ? t("mixengine.blueprints.empty") : t("mixengine.blueprints.noMatches")}
          />
        </Card>
      ) : (
        <ul className={styles.grid}>
          {shown.map((row) => (
            <li key={row.slug} className={styles.card}>
              <div className={styles.cardHead}>
                <MonogramBadge name={row.name} size={38} />
                <div className={styles.cardTitle}>
                  <h2 className={styles.name}>{row.name}</h2>
                  <span className={styles.source}>{sourceLabel(row)}</span>
                </div>
                {row.trusted ? (
                  <StatusPill tone="success">{t("mixengine.blueprints.trustedBadge")}</StatusPill>
                ) : row.source === "imported" && row.signature === "rejected" ? (
                  <StatusPill tone="danger">{t("mixengine.blueprints.signatureRejected")}</StatusPill>
                ) : (
                  <StatusPill tone="warning">{t("mixengine.blueprints.untrustedBadge")}</StatusPill>
                )}
              </div>
              <p className={styles.description}>{row.description}</p>
              <div className={styles.cardActions}>
                <Button variant="soft" onClick={() => setApplying(row)}>
                  {t("mixengine.blueprints.applyButton")}
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
      {capturing && (
        <CaptureDialog
          onCancel={() => setCapturing(false)}
          onCaptured={() => {
            setCapturing(false);
            void reload();
          }}
        />
      )}

      {importing && (
        <ImportDialog
          onCancel={() => setImporting(false)}
          onImported={() => {
            setImporting(false);
            void reload();
          }}
        />
      )}

      {applying && (
        <ApplyDialog
          blueprint={applying}
          onCancel={() => setApplying(null)}
          onDone={(applied) => {
            setApplying(null);
            // A failed apply has nothing to start and no site to open.
            setSettling(applied);
            void reload();
          }}
        />
      )}

      {/* Brought up **after** `ApplyDialog` closes, not nested inside it: both are `Modal`s, and
          `Modal` listens for Escape at `window` level — stacked, one key closes both. */}
      {settling && (
        <AfterApply applied={settling} onFinished={() => setSettling(null)} />
      )}
    </div>
  );
}
