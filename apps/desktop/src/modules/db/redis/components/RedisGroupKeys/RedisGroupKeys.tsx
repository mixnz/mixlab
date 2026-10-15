import { useEffect, useMemo, useState } from "react";
import { redisDeleteKeys, type RedisKeyInfo } from "../../api";
import Button from "../../../../../components/Button";
import ConfirmDialog from "../../../../../components/ConfirmDialog";
import Input from "../../../../../components/Input";
import LoadingOverlay from "../../../../../components/LoadingOverlay";
import Checkbox from "../../../../../components/Checkbox";
import RedisTypeBadge from "../RedisTypeBadge";
import { CloseIcon } from "../../../../../icons";
import { useTranslation } from "../../../../../i18n";
import { errorMessage } from "../../../../../core/errors";
import styles from "./RedisGroupKeys.module.css";

interface Props {
  connectionId: string;
  /** The group's own prefix, as the sidebar's separator reads it — a name, not a glob. */
  prefix: string;
  /** The keys under that prefix. Already in hand from the sidebar's sweep, so nothing here goes
   * back to the server to list them. */
  keys: RedisKeyInfo[];
  /** The sweep behind {@link keys} has not run to the end of the keyspace, so this listing is
   * what has been read rather than everything the prefix holds. */
  partial?: boolean;
  /** The connection is marked read-only: the group is listed as it is — which is what the pane is
   *  for — and the button that would delete the ticked keys is closed with the reason on it. */
  readOnly?: boolean;
  onError: (message: string) => void;
  /** The names that are gone, once the server has confirmed it. */
  onDeleted: (names: string[]) => void;
  onClose: () => void;
}

/** How many names go into one `UNLINK`. A group can hold tens of thousands of keys, and one
 * command carrying all of them is a single round trip the server spends entirely on this client —
 * batching keeps each one short and gives the progress line something to count. */
const DELETE_BATCH_SIZE = 500;

/** How many rows the list starts with, and how many more one press of Show more draws. Every key
 * is already in hand; what this holds off is the cost of laying out thousands of rows at once. */
const ROW_REVEAL_STEP = 200;

/**
 * The keys under one group of the sidebar, listed with a checkbox each and a button that deletes
 * whichever are ticked.
 *
 * Redis has no notion of deleting a namespace — `user:*` is a pattern over a flat keyspace, not a
 * thing the server can drop in one call — so removing a group means naming every key in it. That
 * is exactly what this pane is: the names it is about to delete, on screen, before anything is
 * deleted. Nothing is ticked to begin with, and what the list shows is what the sidebar has
 * already read, so a scan that stopped short says so rather than passing a partial group off as
 * the whole of it.
 */
function RedisGroupKeys({
  connectionId,
  prefix,
  keys,
  partial,
  readOnly = false,
  onError,
  onDeleted,
  onClose,
}: Props) {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState("");
  const [revealed, setRevealed] = useState(ROW_REVEAL_STEP);
  const [confirming, setConfirming] = useState(false);
  // How far a delete has got, or null when none is running. Counted in keys rather than in
  // batches: the batch size is this component's business, the number of keys is the user's.
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);

  // Keys leave this list — deleted here, deleted from the value pane, dropped by a rescan — and a
  // tick left behind on one of them would put a name back into the next delete that is already
  // gone from the keyspace.
  useEffect(() => {
    setSelected((prev) => {
      if (prev.size === 0) return prev;
      const present = new Set(keys.map((key) => key.name));
      const next = new Set([...prev].filter((name) => present.has(name)));
      return next.size === prev.size ? prev : next;
    });
  }, [keys]);

  const filtered = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    if (!needle) return keys;
    return keys.filter((key) => key.name.toLowerCase().includes(needle));
  }, [keys, filter]);

  const rows = useMemo(
    () => (filtered.length > revealed ? filtered.slice(0, revealed) : filtered),
    [filtered, revealed],
  );
  const hiddenRows = filtered.length - rows.length;

  // Read against the filtered list, since that is what the box next to it acts on: ticking Select
  // all while a filter is on must not quietly select the keys it is hiding.
  const selectedInView = filtered.reduce((n, key) => (selected.has(key.name) ? n + 1 : n), 0);
  const allSelected = filtered.length > 0 && selectedInView === filtered.length;

  function toggleKey(name: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }

  function toggleAll() {
    setSelected((prev) => {
      const next = new Set(prev);
      for (const key of filtered) {
        if (allSelected) next.delete(key.name);
        else next.add(key.name);
      }
      return next;
    });
  }

  /** Deletes every ticked key, in batches. A batch that fails stops the run, but whatever went
   * through before it is gone from the server — so the sidebar hears about that much either way,
   * rather than being left showing keys that no longer exist. */
  async function deleteSelected() {
    setConfirming(false);
    const names = keys.filter((key) => selected.has(key.name)).map((key) => key.name);
    if (names.length === 0) return;
    const removed: string[] = [];
    setProgress({ done: 0, total: names.length });
    try {
      for (let i = 0; i < names.length; i += DELETE_BATCH_SIZE) {
        const batch = names.slice(i, i + DELETE_BATCH_SIZE);
        await redisDeleteKeys(connectionId, batch);
        removed.push(...batch);
        setProgress({ done: removed.length, total: names.length });
      }
    } catch (e) {
      onError(errorMessage(t, e));
    } finally {
      setProgress(null);
      if (removed.length > 0) onDeleted(removed);
    }
  }

  const deleting = progress !== null;
  const selectedCount = selected.size;

  return (
    <div className={styles.groupKeys}>
      <div className={styles.header}>
        <div className={styles.identity}>
          <span className={styles.prefix} title={prefix}>
            {prefix}
          </span>
          <span className={styles.meta}>{t("redisGroup.keyCount", { n: keys.length })}</span>
        </div>
        <button type="button" className={styles.close} title={t("common.close")} onClick={onClose}>
          <CloseIcon size={14} />
        </button>
      </div>

      {partial && <p className={styles.notice}>{t("redisGroup.partialNotice")}</p>}

      <div className={styles.toolbar}>
        <Checkbox
          className={styles.selectAll}
          label={t("redisGroup.selectAll")}
          checked={allSelected}
          // Some but not all of what the filter is showing — the box says so rather than lying
          // either way.
          indeterminate={selectedInView > 0 && !allSelected}
          disabled={deleting || filtered.length === 0}
          onChange={toggleAll}
        />
        <Input
          size="normal"
          className={styles.filter}
          placeholder={t("redisGroup.filterPlaceholder")}
          value={filter}
          disabled={deleting}
          onChange={(e) => {
            setFilter(e.target.value);
            setRevealed(ROW_REVEAL_STEP);
          }}
        />
        <Button
          className={styles.delete}
          disabled={readOnly || selectedCount === 0 || deleting}
          // The button carries its own words, so the reason goes in the tooltip rather than in
          // place of them — unlike the icon-only buttons, which have nothing else to say.
          title={readOnly ? t("common.readOnlyConnection") : undefined}
          onClick={() => setConfirming(true)}
        >
          {t("redisGroup.deleteSelected", { n: selectedCount })}
        </Button>
      </div>

      <div className={styles.scrollWrap}>
        <div className={styles.scroll}>
          {keys.length === 0 ? (
            <p className="muted">{t("redisGroup.noKeys")}</p>
          ) : filtered.length === 0 ? (
            <p className="muted">{t("redisGroup.noMatches")}</p>
          ) : (
            <ul className={styles.rows}>
              {rows.map((key) => (
                <li key={key.name}>
                  <Checkbox
                    className={styles.row}
                    label={
                      <span className={styles.rowLabel}>
                        <RedisTypeBadge type={key.type} />
                        <span className={styles.name}>{key.name}</span>
                      </span>
                    }
                    checked={selected.has(key.name)}
                    disabled={deleting}
                    onChange={() => toggleKey(key.name)}
                  />
                </li>
              ))}
            </ul>
          )}
        </div>
        {progress !== null && (
          <LoadingOverlay
            label={t("redisGroup.deleting", { done: progress.done, total: progress.total })}
          />
        )}
      </div>

      <div className={styles.footer}>
        <span className="muted">
          {t("redisGroup.selectedCount", { selected: selectedCount, total: keys.length })}
        </span>
        {hiddenRows > 0 && (
          <button
            type="button"
            className={styles.showMore}
            onClick={() => setRevealed((n) => n + ROW_REVEAL_STEP)}
          >
            {t("redis.showMoreRows", { n: Math.min(hiddenRows, ROW_REVEAL_STEP) })}
          </button>
        )}
      </div>

      {confirming && (
        <ConfirmDialog
          title={t("redisGroup.confirmTitle")}
          message={t("redisGroup.confirmMessage", { n: selectedCount, prefix })}
          confirmLabel={t("common.delete")}
          danger
          onConfirm={deleteSelected}
          onCancel={() => setConfirming(false)}
        />
      )}
    </div>
  );
}

export default RedisGroupKeys;
