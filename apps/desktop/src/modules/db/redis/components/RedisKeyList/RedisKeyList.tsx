import { useCallback, useEffect, useEffectEvent, useMemo, useRef, useState } from "react";
import type { RedisKeyInfo } from "../../api";
import {
  ancestorPaths,
  buildKeyTree,
  flatRows,
  resolveTreeKey,
  TREE_NAV_KEYS,
  visibleRows,
} from "../../keyTree";
import ContextMenu from "../../../../../components/ContextMenu";
import RedisTypeBadge from "../RedisTypeBadge";
import { ChevronDownIcon, ChevronRightIcon, FolderIcon } from "../../../../../icons";
import { useTranslation } from "../../../../../i18n";
import styles from "./RedisKeyList.module.css";

/** One entry of the menu a group row opens on right-click. */
export interface RedisGroupAction {
  /** Distinguishes this entry from the others in the menu. */
  key: string;
  label: string;
  /** Paints the entry as destructive. For actions that lose data, not merely risky ones. */
  danger?: boolean;
  /** Given the prefix the menu was opened on — the menu closes first, so this may open a pane. */
  onSelect: (path: string) => void;
}

interface Props {
  keys: RedisKeyInfo[];
  /** What splits a key name into levels — `:` by convention. Empty renders the flat list. */
  separator: string;
  selectedKey?: string | null;
  onSelect: (key: string) => void;
  emptyMessage?: string;
  /** The sweep over the keyspace is still running: more names are on their way, so the tree is
   * still being re-sorted around what is on screen and the folder counts are not final. */
  scanning?: boolean;
  /** Whether the keyspace scan has more to hand back. Only true once the sweep has stopped at
   * its ceiling — a sweep that ran to the end leaves nothing behind. */
  hasMore: boolean;
  /** The sweep stopped because it hit its ceiling rather than because the keyspace ran out. */
  limitReached?: boolean;
  /** How many keys have been read so far, across every round of the scan. */
  loadedCount: number;
  loadingMore?: boolean;
  onLoadMore: () => void;
  /** What right-clicking a group row offers. Left out, right-click does what it always does.
   * Group rows only: an entry here acts on a prefix and everything under it, which is a thing a
   * single key is not. */
  groupActions?: RedisGroupAction[];
  className?: string;
}

/** Where the group menu was opened, and on which prefix. */
interface MenuState {
  path: string;
  x: number;
  y: number;
}

/** How many more rows one press of Show more draws, and how many the list starts with. This is a
 * limit on rows rather than on keys: the whole keyspace is already in hand, and what it holds off
 * is the cost of laying out thousands of rows nobody has scrolled to. */
const ROW_REVEAL_STEP = 200;

/**
 * The key list in the Redis sidebar, drawn as a tree over the keys' shared prefixes. Nothing in
 * Redis makes `user:1:name` a child of anything — the keyspace is flat, and the separator is a
 * convention the caller picks — so the tree is a reading of the names, and switching the
 * separator re-reads the same keys.
 *
 * The list grows from the bottom. That takes some doing, because `SCAN` hands the keyspace back
 * in no order at all and the tree sorts by name: a name arriving late belongs wherever it sorts,
 * which is usually somewhere above what the user is looking at. So the caller sweeps the whole
 * keyspace first and this list only ever *reveals* more of an order that is already settled —
 * Show more appends below and never disturbs a row above it. Only when that sweep stops at its
 * ceiling ({@link Props.limitReached}) does Load more go back to the server, and then the footer
 * says as much and the folder counts are marked as not final.
 */
function RedisKeyList({
  keys,
  separator,
  selectedKey,
  onSelect,
  emptyMessage,
  scanning,
  hasMore,
  limitReached,
  loadedCount,
  loadingMore,
  onLoadMore,
  groupActions,
  className,
}: Props) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState<Set<string>>(() => ancestorPaths(selectedKey, separator));
  // How far down the row list is drawn. Only ever raised by hand, so nothing the scan or a
  // delete does to `keys` can pull rows back out from under the user mid-read.
  const [revealed, setRevealed] = useState(ROW_REVEAL_STEP);
  // Which row the arrow keys move from, held by path rather than by index: loading more keys
  // re-sorts the siblings around it, and an index would then point at a different row than the
  // one the user left the focus on.
  const [focusedPath, setFocusedPath] = useState<string | null>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const rowRefs = useRef(new Map<string, HTMLButtonElement>());

  // A path means something else under a different separator, so what was open under the old one
  // says nothing about the new one — the tree starts closed again, reopened only down to the
  // selected key so it doesn't vanish out from under the selection. `selectedKey` is read through
  // an effect event rather than listed: selecting a key must not close the tree around it.
  const closeTreeFor = useEffectEvent((nextSeparator: string) => {
    setExpanded(ancestorPaths(selectedKey, nextSeparator));
    setRevealed(ROW_REVEAL_STEP);
  });
  useEffect(() => {
    closeTreeFor(separator);
  }, [separator]);

  // A shorter list than last render is a different list — a rescan, another database, a deleted
  // key — so the reveal starts over. Growth is left alone: that is the scan filling in, and
  // collapsing the list back under someone reading it would be the whole complaint again.
  const previousKeyCount = useRef(keys.length);
  useEffect(() => {
    if (keys.length < previousKeyCount.current) setRevealed(ROW_REVEAL_STEP);
    previousKeyCount.current = keys.length;
  }, [keys.length]);

  const tree = useMemo(() => buildKeyTree(keys, separator), [keys, separator]);
  // One list of rows either way — grouped by prefix, or every key at depth 0 — so the rendering
  // below and the keyboard handling have a single shape to work on.
  const allRows = useMemo(
    () => (separator ? visibleRows(tree, expanded) : flatRows(keys)),
    [separator, tree, expanded, keys],
  );
  // Sliced before anything else reads it, so the keyboard walks exactly the rows on screen —
  // an index into a longer list would step the focus onto a row that was never drawn.
  const rows = useMemo(
    () => (allRows.length > revealed ? allRows.slice(0, revealed) : allRows),
    [allRows, revealed],
  );
  const hiddenRows = allRows.length - rows.length;
  // Until the sweep has run to the end, a folder's count is only what has been read so far.
  const countsPartial = Boolean(scanning) || hasMore;

  // The tree this menu was opened over can be rebuilt under it — a rescan, another database,
  // another separator, a folder collapsed — and an entry would then act on a prefix that is no
  // longer on screen.
  useEffect(() => {
    setMenu((open) => (open && rows.some((row) => row.node.path === open.path) ? open : null));
  }, [rows]);

  const closeMenu = useCallback(() => setMenu(null), []);

  const hasGroupMenu = groupActions !== undefined && groupActions.length > 0;

  // Where the arrows move from, and the one row in the tab order. Held by path above, but that
  // path may be gone (a rescan) or never set (nothing focused yet) — in which case the selected
  // key stands in, so the first arrow press continues from the key already open on the right
  // rather than jumping to the top of the list. The first row is the last resort.
  const focusedIndex = rows.findIndex((row) => row.node.path === focusedPath);
  const selectedIndex = rows.findIndex((row) => row.node.key?.name === selectedKey);
  const activeIndex = focusedIndex >= 0 ? focusedIndex : Math.max(0, selectedIndex);

  function setExpansion(path: string, open: boolean) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (open) next.add(path);
      else next.delete(path);
      return next;
    });
  }

  /** Moves the roving focus to `index`, if there is a row there. The element is focused directly
   * rather than through an effect: every move below leaves the rows themselves unchanged, so the
   * target is already mounted. */
  function moveTo(index: number) {
    const row = rows[index];
    if (!row) return;
    setFocusedPath(row.node.path);
    rowRefs.current.get(row.node.path)?.focus();
  }

  /** Arrow-key navigation. What each key means is {@link resolveTreeKey}'s; this carries it out
   * and swallows the keystroke either way, so an arrow never scrolls the sidebar out from under
   * a list that was going to handle it. */
  function onKeyDown(e: React.KeyboardEvent) {
    if (!TREE_NAV_KEYS.has(e.key)) return;
    e.preventDefault();
    const action = resolveTreeKey(e.key, rows, activeIndex);
    if (!action) return;
    if (action.kind === "move") moveTo(action.index);
    else setExpansion(action.path, action.kind === "expand");
  }

  return (
    <div className={`${styles.keyList}${className ? ` ${className}` : ""}`}>
      {/* A flattened tree: the nesting lives in `aria-level` rather than in nested lists, which
          is what lets one row list serve both the grouped and the ungrouped view. */}
      <ul className={styles.rows} role="tree" aria-label={t("redis.keyTreeLabel")} onKeyDown={onKeyDown}>
        {rows.map(({ node, depth, open }, index) => {
          const isFolder = node.children.length > 0;
          const selected = node.key !== undefined && node.key.name === selectedKey;
          return (
            <li key={node.path} role="none">
              <button
                type="button"
                role="treeitem"
                aria-level={depth + 1}
                aria-selected={selected}
                aria-expanded={isFolder ? open : undefined}
                // Exactly one row is in the tab order at a time, so Tab moves past the list
                // rather than through every key in it; the arrows move within. Which row that
                // is starts as the selected one, so Tab lands where the arrows will start.
                tabIndex={index === activeIndex ? 0 : -1}
                ref={(el) => {
                  if (el) rowRefs.current.set(node.path, el);
                  else rowRefs.current.delete(node.path);
                }}
                className={`${styles.item}${selected ? ` ${styles.itemActive}` : ""}`}
                // The row is one control rather than a row plus a chevron button: a button
                // inside a button is not valid, and a node that is both a key and a folder
                // wants both things to happen anyway.
                onClick={() => {
                  setFocusedPath(node.path);
                  if (isFolder) setExpansion(node.path, !open);
                  if (node.key) onSelect(node.key.name);
                }}
                // Group rows only. What the menu offers acts on a prefix and everything under it,
                // which is nothing a leaf row stands for — and opening an empty menu on one would
                // read as the entries having gone missing.
                onContextMenu={
                  hasGroupMenu && isFolder
                    ? (e) => {
                        e.preventDefault();
                        setFocusedPath(node.path);
                        setMenu({ path: node.path, x: e.clientX, y: e.clientY });
                      }
                    : undefined
                }
                style={{ paddingLeft: `${0.5 + depth * 0.75}rem` }}
                title={node.key ? node.key.name : node.path}
              >
                <span className={styles.chevron}>
                  {isFolder && (open ? <ChevronDownIcon size={12} /> : <ChevronRightIcon size={12} />)}
                </span>
                {/* The badge slot is a fixed column, so folder rows and key rows line their
                    names up. A prefix has no type to show there — it gets the folder mark
                    instead, which is what the slot used to sit empty for. */}
                {node.key ? (
                  <RedisTypeBadge type={node.key.type} />
                ) : (
                  <span className={styles.folderBadge}>
                    <FolderIcon size={16} />
                  </span>
                )}
                <span className={styles.name}>{node.label}</span>
                {isFolder && (
                  // Marked while the keyspace is still being read: the number is how many keys
                  // under this prefix have arrived, not how many the server holds.
                  <span
                    className={styles.count}
                    title={countsPartial ? t("redis.partialCountTooltip") : undefined}
                  >
                    {node.count}
                    {countsPartial ? "+" : ""}
                  </span>
                )}
              </button>
            </li>
          );
        })}
        {keys.length === 0 && emptyMessage && (
          <li role="none" className={`muted ${styles.empty}`}>
            {emptyMessage}
          </li>
        )}
      </ul>
      {/* Outside the scrolling rows, so what the list is still holding back is readable without
          scrolling to the end of it — the button at the far bottom of a long list is what made
          the old one look like a complete list with a stray button under it. */}
      <div className={styles.footer}>
        {(scanning || loadedCount > 0) && (
          <span className={`muted ${styles.status}`}>
            {scanning
              ? t("redis.scanningKeys", { n: loadedCount.toLocaleString() })
              : hasMore
                ? t("redis.keysLoadedPartial", { n: loadedCount.toLocaleString() })
                : t("redis.keysLoadedAll", { n: loadedCount.toLocaleString() })}
          </span>
        )}
        {limitReached && !scanning && (
          <span className={styles.notice}>{t("redis.scanLimitNotice")}</span>
        )}
        {/* Two different buttons, never both. Showing more rows is free and settles nothing —
            the names are already here, in order. Loading more keys goes back to the server, and
            only exists at all once the sweep has given up at its ceiling. */}
        {hiddenRows > 0 ? (
          <button
            type="button"
            className={styles.loadMore}
            onClick={() => setRevealed((n) => n + ROW_REVEAL_STEP)}
          >
            {t("redis.showMoreRows", { n: Math.min(hiddenRows, ROW_REVEAL_STEP) })}
          </button>
        ) : (
          hasMore &&
          !scanning && (
            <button
              type="button"
              className={styles.loadMore}
              disabled={loadingMore}
              onClick={onLoadMore}
            >
              {loadingMore ? t("redis.loadingMore") : t("redis.loadMoreKeys")}
            </button>
          )
        )}
      </div>
      {menu !== null && groupActions !== undefined && (
        <ContextMenu x={menu.x} y={menu.y} onClose={closeMenu}>
          {groupActions.map((action) => (
            <button
              key={action.key}
              type="button"
              className={action.danger ? "context-menu-delete" : undefined}
              onClick={() => {
                setMenu(null);
                action.onSelect(menu.path);
              }}
            >
              {action.label}
            </button>
          ))}
        </ContextMenu>
      )}
    </div>
  );
}

export default RedisKeyList;
