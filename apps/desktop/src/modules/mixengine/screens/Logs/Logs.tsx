import { useEffect, useState } from "react";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import EmptyState from "../../../../components/EmptyState";
import ErrorBanner from "../../../../components/ErrorBanner";
import LoadingState from "../../../../components/LoadingState";
import PageHeader from "../../../../components/PageHeader";
import SegmentedControl from "../../../../components/SegmentedControl";
import Select from "../../../../components/Select";
import { HistoryIcon } from "../../../../icons";
import { errorMessage } from "../../../../core/errors";
import { useTailScroll } from "../../../../core/tailScroll";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import {
  applyLogFrame,
  countStreams,
  filterByStream,
  type LogEntry,
  type StreamFilter,
} from "../../logState";
import { takePendingLogsService } from "../../logsNavigation";
import styles from "./Logs.module.css";

const MAX_ENTRIES = 2000;
const INITIAL_TAIL = 200;
/** How long an open stream with no line yet is still "reading" rather than "an empty log". The
 *  stream never ends, so there is no answer to wait for: a service that has written nothing sends
 *  nothing, and only the clock can say so. */
const FIRST_LINE_WAIT_MS = 2000;

export default function Logs({ active }: { active: boolean }) {
  const [ids, setIds] = useState<string[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [tail, setTail] = useState(INITIAL_TAIL);
  const [filter, setFilter] = useState<StreamFilter>("all");
  const [entries, setEntries] = useState<LogEntry[]>([]);
  /** True from opening a stream until its first line, its failure, or `FIRST_LINE_WAIT_MS`. */
  const [waiting, setWaiting] = useState(false);
  const [error, setError] = useState("");
  const { t } = useTranslation();

  // Read the service list again on mount and on every return to this screen — the same reason
  // `Dashboard.tsx` and `ServicesDetail.tsx` have.
  useEffect(() => {
    if (!active) return;
    // A row menu elsewhere may have asked for one service — see `logsNavigation.ts`.
    const requested = takePendingLogsService();
    if (requested !== null) {
      setSelected(requested);
      setTail(INITIAL_TAIL);
    }
    api
      .services()
      .then((list) => setIds(list.services.map((s) => s.id)))
      .catch((e: unknown) => setError(errorMessage(t, e)));
  }, [active, t]);

  useEffect(() => {
    if (selected === null) return;
    setEntries([]);
    setWaiting(true);
    const timer = setTimeout(() => setWaiting(false), FIRST_LINE_WAIT_MS);
    api
      .logsWatch(selected, tail, true, (raw) => {
        setWaiting(false);
        setEntries((current) => applyLogFrame(current, raw, MAX_ENTRIES));
      })
      .catch((e: unknown) => {
        setWaiting(false);
        setError(errorMessage(t, e));
      });
    return () => {
      clearTimeout(timer);
      void api.logsUnwatch();
    };
  }, [selected, tail, t]);

  const counts = countStreams(entries);
  // Only lines read back from the file, so no stream can be told apart: the stream segments are
  // locked, and whatever was picked falls back to All rather than hiding every line.
  const streamsUnknown = counts.historic > 0 && counts.stdout + counts.stderr === 0;
  const shown: StreamFilter = streamsUnknown ? "all" : filter;
  const visible = filterByStream(entries, shown);
  const hiddenHistoric = shown === "all" ? 0 : counts.historic;

  const linePane = useTailScroll<HTMLDivElement>(visible);

  return (
    <div className={`mixengine-page ${styles.screen}`}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <PageHeader
        title={t("mixengine.sidebar.logs")}
        description={t("mixengine.logs.about")}
        actions={
          <>
            <Select
              className={styles.service}
              value={selected ?? ""}
              onChange={(id) => {
                setSelected(id);
                setTail(INITIAL_TAIL);
              }}
              searchable
              placeholder={t("mixengine.logs.pickService")}
              ariaLabel={t("mixengine.logs.service")}
              options={ids.map((id) => ({ value: id, label: id }))}
            />
            <SegmentedControl
              aria-label={t("mixengine.logs.stream")}
              value={shown}
              onChange={setFilter}
              segments={[
                { value: "all", label: t("mixengine.logs.streamAll"), count: counts.all },
                ...(["stdout", "stderr"] as const).map((stream) => ({
                  value: stream,
                  label: t(stream === "stdout" ? "mixengine.logs.streamStdout" : "mixengine.logs.streamStderr"),
                  count: counts[stream],
                  disabled: streamsUnknown,
                  title: streamsUnknown ? t("mixengine.logs.streamUnknown") : undefined,
                })),
              ]}
            />
          </>
        }
      />

      <Card
        flush
        className={styles.card}
        title={selected ?? t("mixengine.logs.pickService")}
        actions={
          selected !== null && (
            <Button size="small" onClick={() => setTail((current) => current * 2)}>
              <HistoryIcon size={14} />
              {t("mixengine.logs.loadMore")}
            </Button>
          )
        }
      >
        {selected === null ? (
          <EmptyState title={t("mixengine.logs.pickService")} />
        ) : (
          <div className={styles.lines} data-density="compact" {...linePane}>
            {hiddenHistoric > 0 && (
              <p className={styles.note}>{t("mixengine.logs.historicHidden", { count: hiddenHistoric })}</p>
            )}
            {visible.length === 0 &&
              hiddenHistoric === 0 &&
              (waiting ? <LoadingState /> : <p className={styles.empty}>{t("mixengine.logs.empty")}</p>)}
            {visible.map((entry, i) => {
              if (entry.kind === "gap") {
                return (
                  <div key={i} className={styles.gap}>
                    {t("mixengine.logs.gap", { count: entry.missed })}
                  </div>
                );
              }
              return (
                <div
                  key={i}
                  className={entry.kind === "line" && entry.stream === "stderr" ? styles.stderr : styles.line}
                >
                  {entry.text}
                </div>
              );
            })}
          </div>
        )}
      </Card>
    </div>
  );
}