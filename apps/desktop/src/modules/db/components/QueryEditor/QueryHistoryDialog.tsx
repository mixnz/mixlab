import { useMemo, useState } from "react";
import { TrashIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import {
  clearQueryHistory,
  removeQueryHistoryEntry,
  useQueryHistory,
  type QueryHistoryEntry,
} from "../../queries/queryHistory";
import Button from "../../../../components/Button";
import Input from "../../../../components/Input";
import styles from "./QueryEditor.module.css";
import Modal, { ModalBody } from "../../../../components/Modal";

interface Props {
  /** Only this connection's runs are listed — a query against staging is not an answer to a
   *  question about production. */
  profileId: string;
  /** Puts the query back in the editor. Whether that replaces the script or is inserted at the
   *  caret is the editor's business, not this list's. */
  onPick: (sql: string) => void;
  onClose: () => void;
}

/** The query on one line, so a list of them can be scanned. The whole thing is in the title
 *  attribute, and picking one puts the whole thing back in the editor. */
function oneLine(sql: string): string {
  return sql.replace(/\s+/g, " ").trim();
}

/**
 * Everything this connection has been asked to run, newest first.
 *
 * A list rather than a panel down the side: it is opened to find one thing, and it closes as soon
 * as that thing is found. The filter matches the query text, so the way back to a query is to
 * remember any word that was in it.
 */
function QueryHistoryDialog({ profileId, onPick, onClose }: Props) {
  const { t, lang } = useTranslation();
  const history = useQueryHistory();
  const [filter, setFilter] = useState("");
  const [confirmClear, setConfirmClear] = useState(false);
  /** The entry whose delete button has been pressed once, held by identity — the objects in the
   *  list outlive a render, and there is nothing else about a run that is reliably unique. Only ever
   *  one at a time, so the armed button is unmistakable. */
  const [confirmDrop, setConfirmDrop] = useState<QueryHistoryEntry | null>(null);

  const mine = useMemo(
    () => history.filter((entry) => entry.profileId === profileId),
    [history, profileId]
  );

  const shown = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    if (needle === "") return mine;
    return mine.filter((entry) => entry.sql.toLowerCase().includes(needle));
  }, [mine, filter]);

  const when = useMemo(
    () => new Intl.DateTimeFormat(lang, { dateStyle: "short", timeStyle: "short" }),
    [lang]
  );

  return (
    <Modal
      title={t("query.historyTitle")}
      onClose={onClose}
      fixedHeight
    >
      {(close) => (
        <>
          <ModalBody>
            <div className={styles.historyTools}>
              <Input
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
                placeholder={t("query.historyFilter")}
                aria-label={t("query.historyFilter")}
                autoFocus
              />
              <Button
                size="small"
                onClick={() => {
                  setConfirmDrop(null);
                  if (confirmClear) {
                    clearQueryHistory(profileId);
                    setConfirmClear(false);
                    return;
                  }
                  setConfirmClear(true);
                }}
                disabled={mine.length === 0}
              >
                <TrashIcon size="0.9em" />
                {/* Two presses rather than a second dialog on top of this one: the button says what it
                    is about to do, and clicking anywhere else takes the offer back. */}
                {confirmClear ? t("query.historyClearConfirm") : t("query.historyClear")}
              </Button>
            </div>

            {shown.length === 0 ? (
              <p className={styles.historyEmpty}>
                {mine.length === 0 ? t("query.historyEmpty") : t("query.historyNoMatch")}
              </p>
            ) : (
              <ul
                className={styles.historyList}
                onMouseDown={() => {
                  setConfirmClear(false);
                  setConfirmDrop(null);
                }}
              >
                {shown.map((entry) => (
                  <li key={`${entry.startedAt}-${entry.sql.length}`} className={styles.entryRow}>
                    <button
                      type="button"
                      className={styles.historyEntry}
                      title={entry.sql}
                      onClick={() => {
                        onPick(entry.sql);
                        close(onClose);
                      }}
                    >
                      <span className={styles.historySql}>{oneLine(entry.sql)}</span>
                      <span className={styles.historyMeta}>
                        <span>{when.format(entry.startedAt)}</span>
                        {entry.database !== "" && <span>{entry.database}</span>}
                        <span>{t("query.duration", { ms: entry.durationMs })}</span>
                        {entry.error !== null ? (
                          <span className={styles.historyFailed}>{t("query.historyFailed")}</span>
                        ) : (
                          entry.rowCount !== null && <span>{t("query.rowCount", { n: entry.rowCount })}</span>
                        )}
                      </span>
                    </button>
                    {/* One run forgotten, rather than the whole list. The history fills with attempts
                        at the same query, and the way to keep it readable is to drop the failures as
                        they are recognised — not to clear it and lose the one that worked. */}
                    <button
                      type="button"
                      className={
                        confirmDrop === entry
                          ? `${styles.entryDelete} ${styles.entryDeleteArmed}`
                          : styles.entryDelete
                      }
                      title={
                        confirmDrop === entry ? t("query.historyDropConfirm") : t("query.historyDrop")
                      }
                      aria-label={t("query.historyDrop")}
                      // The list disarms on mouse-down, which lands before this button's click and
                      // would clear `confirmDrop` in time for the confirming press to read it as
                      // unarmed — the second click re-arming for ever instead of deleting.
                      onMouseDown={(e) => e.stopPropagation()}
                      onClick={() => {
                        if (confirmDrop !== entry) {
                          setConfirmDrop(entry);
                          return;
                        }
                        setConfirmDrop(null);
                        removeQueryHistoryEntry(entry);
                      }}
                    >
                      <TrashIcon size="0.9em" />
                      {confirmDrop === entry && <span>{t("query.historyDropConfirm")}</span>}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </ModalBody>
        </>
      )}
    </Modal>
  );
}

export default QueryHistoryDialog;
