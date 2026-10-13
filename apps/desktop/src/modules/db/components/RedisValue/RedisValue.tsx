import { useCallback, useEffect, useMemo, useState } from "react";
import {
  redisDeleteKeys,
  redisKeyValue,
  type RedisValueItem,
  type RedisValuePage,
} from "../../redis/api";
import { parseJsonDocument } from "../../redis/json";
import ActionBar from "../../../../components/ActionBar";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import JsonView from "../../../../components/JsonView";
import LoadingOverlay from "../../../../components/LoadingOverlay";
import { ReloadIcon, TrashIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import { errorMessage } from "../../../../core/errors";
import { useReloadShortcut, withReloadShortcut } from "../../../../core/reload";
import styles from "./RedisValue.module.css";

interface Props {
  connectionId: string;
  /** The key on screen. Changing it starts the value over from its first page. */
  keyName: string;
  /** The connection is marked read-only: the key is shown as it is, and the delete button is
   *  closed with the reason on it. */
  readOnly?: boolean;
  onError: (message: string) => void;
  /** Called once the key is gone, so the sidebar can drop it and clear the selection. */
  onDeleted: (key: string) => void;
  /**
   * `Ctrl+R` is pointed at this pane rather than at the keyspace beside it.
   *
   * Decided by the workspace, which is the only place that can see both halves at once — see
   * `redis/reloadTarget.ts`. It also names the button, so the pane the key would press is the one
   * saying so.
   */
  reloadActive?: boolean;
}

/** How many items one page of a value holds. A hint for the scanned types (set, hash), an exact
 * count for the ordered ones (list, sorted set). */
const VALUE_PAGE_SIZE = 100;

/** TTL sentinels, as Redis reports them. */
const TTL_NO_EXPIRY = -1;
const TTL_MISSING = -2;

/** The types shown as a table of items — everything but a plain string. */
const COLLECTION_TYPES = new Set(["list", "set", "zset", "hash"]);

/** Seconds as the largest two units that fit — `2h 5m`, `3d 4h`, `45s`. Full precision is
 * noise for something that is counting down while you read it. */
function formatTtl(seconds: number): string {
  const units: [number, string][] = [
    [86400, "d"],
    [3600, "h"],
    [60, "m"],
    [1, "s"],
  ];
  const parts: string[] = [];
  let rest = seconds;
  for (const [size, suffix] of units) {
    const n = Math.floor(rest / size);
    rest -= n * size;
    if (n > 0 || parts.length > 0) parts.push(`${n}${suffix}`);
    if (parts.length === 2) break;
  }
  return parts.length > 0 ? parts.join(" ") : "0s";
}

/**
 * One item of a collection, in a table cell.
 *
 * A JSON document is a common thing to park in a hash field or a list item, and printed raw it
 * is one unreadable line. Formatting it is opt-in per cell rather than automatic: a page holds up
 * to a hundred items, and expanding every one of them by default would bury the list under it.
 */
function CellValue({ text }: { text: string }) {
  const { t } = useTranslation();
  const json = useMemo(() => parseJsonDocument(text), [text]);
  const [formatted, setFormatted] = useState(false);

  if (json === undefined) return <>{text}</>;

  if (!formatted) {
    return (
      <button
        type="button"
        className={styles.jsonCollapsed}
        title={t("redisValue.expandJson")}
        onClick={() => setFormatted(true)}
      >
        <span className={styles.jsonChip}>JSON</span>
        <span className={styles.jsonPreview}>{text}</span>
      </button>
    );
  }

  return (
    <div className={styles.jsonExpanded}>
      <button
        type="button"
        className={styles.jsonChipButton}
        title={t("redisValue.collapseJson")}
        onClick={() => setFormatted(false)}
      >
        <span className={styles.jsonChip}>JSON</span>
      </button>
      <JsonView value={json} />
    </div>
  );
}

/**
 * One Redis key: what it holds, and the button that removes it.
 *
 * Values load a page at a time behind a Load more button rather than by numbered pages. Two of
 * the five types leave no choice — a set and a hash are read with `SSCAN`/`HSCAN`, cursor walks
 * with no notion of a page number to jump to — and having the other three behave the same way
 * keeps one control for all of them.
 */
function RedisValue({
  connectionId,
  keyName,
  readOnly = false,
  onError,
  onDeleted,
  reloadActive = false,
}: Props) {
  const { t } = useTranslation();
  const [page, setPage] = useState<RedisValuePage | null>(null);
  const [items, setItems] = useState<RedisValueItem[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  // Only meaningful for a string key that parses as JSON: which of the two forms the pane shows.
  const [showRaw, setShowRaw] = useState(false);

  const load = useCallback(() => {
    let cancelled = false;
    setLoading(true);
    redisKeyValue(connectionId, keyName, null, VALUE_PAGE_SIZE)
      .then((result) => {
        if (cancelled) return;
        setPage(result);
        setItems(result.items);
        setCursor(result.nextCursor);
      })
      .catch((e) => {
        if (!cancelled) onError(errorMessage(t, e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [connectionId, keyName, onError, t]);

  useEffect(() => load(), [load]);

  /** Appends the next page. The cursor is what the previous page handed back, so nothing here
   * depends on how many items are already on screen. */
  function loadMore() {
    if (cursor === null || loadingMore) return;
    setLoadingMore(true);
    redisKeyValue(connectionId, keyName, cursor, VALUE_PAGE_SIZE)
      .then((result) => {
        setItems((prev) => [...prev, ...result.items]);
        setCursor(result.nextCursor);
        // The totals and the TTL are re-read with every page: both move on a live server, and
        // what the last page reported is the fresher answer.
        setPage((prev) => (prev ? { ...prev, ttl: result.ttl, total: result.total } : result));
      })
      .catch((e) => onError(errorMessage(t, e)))
      .finally(() => setLoadingMore(false));
  }

  async function deleteKey() {
    setConfirmingDelete(false);
    setDeleting(true);
    try {
      await redisDeleteKeys(connectionId, [keyName]);
      onDeleted(keyName);
    } catch (e) {
      onError(errorMessage(t, e));
    } finally {
      setDeleting(false);
    }
  }

  const type = page?.type ?? "";
  // A string key gets the whole pane to itself, so a JSON one is formatted from the start —
  // unlike the table cells, where that would bury the list. The raw text stays one button away:
  // it is what is actually stored, and what you would copy back out.
  const stringJson = useMemo(
    () => (type === "string" && items.length > 0 ? parseJsonDocument(items[0].value) : undefined),
    [type, items],
  );
  const isCollection = COLLECTION_TYPES.has(type);
  // A stream, or a type some module registered: the key is real, this viewer simply has no
  // reading for it — worth saying so rather than showing it as an empty value.
  const isUnsupported = type !== "" && type !== "none" && type !== "string" && !isCollection;
  const busy = loading || deleting;

  // Gated on the same state the button is: a re-read asked for while one is already out, or over a
  // key mid-delete, is one the button would refuse.
  useReloadShortcut(reloadActive, () => {
    if (busy) return;
    load();
  });

  const ttlText =
    page === null
      ? ""
      : page.ttl === TTL_NO_EXPIRY
        ? t("redisValue.noExpiry")
        : page.ttl === TTL_MISSING
          ? t("redisValue.expired")
          : t("redisValue.expiresIn", { time: formatTtl(page.ttl) });

  return (
    <div className={styles.redisValue}>
      <div className={styles.header}>
        <div className={styles.identity}>
          <span className={styles.keyName} title={keyName}>
            {keyName}
          </span>
          <div className={styles.meta}>
            {type && <span className={styles.type}>{type}</span>}
            {ttlText && <span>{ttlText}</span>}
            {page !== null && page.total >= 0 && type !== "string" && (
              <span>{t("redisValue.itemCount", { n: page.total })}</span>
            )}
            {stringJson !== undefined && (
              <button
                type="button"
                className={styles.formatToggle}
                aria-pressed={!showRaw}
                onClick={() => setShowRaw((raw) => !raw)}
              >
                {showRaw ? t("redisValue.viewFormatted") : t("redisValue.viewRaw")}
              </button>
            )}
          </div>
        </div>
        <ActionBar
          actions={[
            {
              key: "reload",
              icon: ReloadIcon,
              label: reloadActive
                ? withReloadShortcut(t("redisValue.reload"))
                : t("redisValue.reload"),
              disabled: busy,
              busy: loading,
              onClick: load,
            },
            {
              key: "delete",
              icon: TrashIcon,
              label: t("redisValue.deleteKey"),
              danger: true,
              disabled: readOnly || busy,
              disabledHint: readOnly ? t("common.readOnlyConnection") : undefined,
              onClick: () => setConfirmingDelete(true),
            },
          ]}
        />
      </div>

      <div className={styles.scrollWrap}>
        <div className={styles.scroll}>
          {type === "none" && !loading && <p className="muted">{t("redisValue.keyGone")}</p>}
          {type === "string" &&
            (items.length > 0 ? (
              stringJson !== undefined && !showRaw ? (
                <JsonView value={stringJson} />
              ) : (
                <pre className={styles.stringValue}>{items[0].value}</pre>
              )
            ) : (
              !loading && <p className="muted">{t("redisValue.emptyValue")}</p>
            ))}
          {isCollection && (
            <>
              {items.length === 0 && !loading && <p className="muted">{t("redisValue.emptyValue")}</p>}
              {items.length > 0 && (
                <table className={styles.table}>
                  <thead>
                    <tr>
                      {type === "list" && <th className={styles.numberColumn}>#</th>}
                      {type === "hash" && <th className={styles.fieldColumn}>{t("redisValue.field")}</th>}
                      {type === "zset" && <th className={styles.scoreColumn}>{t("redisValue.score")}</th>}
                      <th>{type === "hash" ? t("redisValue.value") : t("redisValue.member")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {items.map((item, i) => (
                      <tr key={`${i}:${item.field ?? item.value}`}>
                        {type === "list" && <td className={styles.numberColumn}>{item.index ?? i}</td>}
                        {type === "hash" && <td className={styles.fieldColumn}>{item.field}</td>}
                        {type === "zset" && <td className={styles.scoreColumn}>{String(item.score ?? "")}</td>}
                        <td className={styles.valueCell}>
                          <CellValue text={item.value} />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </>
          )}
          {isUnsupported && <p className="muted">{t("redisValue.unsupportedType", { type })}</p>}
        </div>
        {busy && (
          <LoadingOverlay label={deleting ? t("redisValue.deleting") : t("redisValue.loading")} />
        )}
      </div>

      {cursor !== null && (
        <div className={styles.footer}>
          <span className="muted">
            {page !== null && page.total >= 0
              ? t("redisValue.loadedOf", { loaded: items.length, total: page.total })
              : t("redisValue.loadedCount", { loaded: items.length })}
          </span>
          <button type="button" className={styles.loadMore} disabled={loadingMore} onClick={loadMore}>
            {loadingMore ? t("redis.loadingMore") : t("redisValue.loadMoreItems")}
          </button>
        </div>
      )}

      {confirmingDelete && (
        <ConfirmDialog
          title={t("redisValue.deleteKeyTitle")}
          message={t("redisValue.deleteKeyMessage", { key: keyName })}
          confirmLabel={t("common.delete")}
          danger
          onConfirm={deleteKey}
          onCancel={() => setConfirmingDelete(false)}
        />
      )}
    </div>
  );
}

export default RedisValue;
