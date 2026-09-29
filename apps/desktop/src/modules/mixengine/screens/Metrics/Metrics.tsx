import { useCallback, useEffect, useState } from "react";

import Card from "../../../../components/Card";
import EmptyState from "../../../../components/EmptyState";
import ErrorBanner from "../../../../components/ErrorBanner";
import PageHeader from "../../../../components/PageHeader";
import Select from "../../../../components/Select";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { MetricsHistory } from "@mixengine/api";
import { segmentsFor } from "../../metricsHistoryState";
import { DAEMON_SUBJECT, machineShare, metricsSubjectFor } from "../../metricsState";
import Chart from "./Chart";
import { CPU_UNIT, RSS_UNIT, windowStart } from "./chartScale";
import styles from "./Metrics.module.css";

const HOUR_MS = 3_600_000;

/**
 * 24-hour history per subject — history only, not repeating the "now" figures the Dashboard
 * already draws (Decision D1, Metrics/Settings spec). `metrics.history` is a plain read RPC; no
 * stream needs to be held open.
 */
export default function Metrics({ active }: { active: boolean }) {
  const [subjects, setSubjects] = useState<string[]>([]);
  const [subject, setSubject] = useState(DAEMON_SUBJECT);
  const [history, setHistory] = useState<MetricsHistory | null>(null);
  // The "now" anchor of this read, not of this render — the axis must stand still between two
  // loads, otherwise every React redraw nudges the chart a little.
  const [loadedAt, setLoadedAt] = useState(() => Date.now());
  const [error, setError] = useState("");
  const { t } = useTranslation();

  // The service list only builds the picker — read once when the screen is first visited; no
  // stream needs following, because this is not a live state table like the Dashboard.
  useEffect(() => {
    if (!active) return;
    void api
      .services()
      .then((list) => setSubjects(list.services.map((service) => metricsSubjectFor(service.id))))
      .catch((e) => setError(errorMessage(t, e)));
  }, [active, t]);

  const reload = useCallback(async () => {
    try {
      setHistory(await api.metricsHistory({ subject, since: null, until: null }));
      setLoadedAt(Date.now());
      setError("");
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }, [subject, t]);

  useEffect(() => {
    if (active) void reload();
  }, [active, reload]);

  const minutes = history?.minutes ?? [];
  const cores = history?.cores ?? 1;
  const segments = segmentsFor(minutes);

  /* The axis does not stretch the measured range across the full width: `windowStart` picks a round
     time step containing it, and the unmeasured part of that step shows up as exactly that. */
  const retention = (history?.retention_hours ?? 24) * HOUR_MS;
  const last = minutes[minutes.length - 1];
  const to = Math.max(loadedAt, last === undefined ? 0 : last.minute + 60_000);
  const from = windowStart(minutes[0]?.minute ?? null, to, retention);

  return (
    <div className={`mixengine-page ${styles.metrics}`}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <PageHeader title={t("mixengine.sidebar.metrics")} description={t("mixengine.metrics.about")} />

      <Card
        title={t("mixengine.metrics.history")}
        description={history ? t("mixengine.metrics.retention", { hours: history.retention_hours }) : undefined}
        actions={
          <Select
            className={styles.subject}
            value={subject}
            onChange={setSubject}
            searchable
            ariaLabel={t("mixengine.metrics.subject")}
            options={[
              { value: DAEMON_SUBJECT, label: t("mixengine.metrics.daemon") },
              ...subjects.map((s) => ({ value: s, label: s.replace(/^service:/, "") })),
            ]}
          />
        }
      >
        {minutes.length === 0 ? (
          <EmptyState title={t("mixengine.metrics.empty")} />
        ) : (
          <div className={styles.charts}>
            <section>
              <h3 className={styles.title}>{t("mixengine.metrics.cpu")}</h3>
              <Chart
                segments={segments}
                from={from}
                to={to}
                unit={CPU_UNIT}
                label={t("mixengine.metrics.cpu")}
                // Rows are stored as a percentage of one core; drawn as a percentage of the whole
                // machine, like Task Manager (T190c).
                avg={(m) => (m.cpu_avg === null ? null : machineShare(m.cpu_avg, cores))}
                peak={(m) => (m.cpu_peak === null ? null : machineShare(m.cpu_peak, cores))}
                hue="sky"
              />
            </section>
            <section>
              <h3 className={styles.title}>{t("mixengine.metrics.rss")}</h3>
              <Chart
                segments={segments}
                from={from}
                to={to}
                unit={RSS_UNIT}
                label={t("mixengine.metrics.rss")}
                avg={(m) => m.rss_avg}
                peak={(m) => m.rss_peak}
                hue="purple"
              />
            </section>
          </div>
        )}
      </Card>
    </div>
  );
}