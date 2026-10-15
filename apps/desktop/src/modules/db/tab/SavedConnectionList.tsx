import Button from "../../../components/Button";
import EmptyState from "../../../components/EmptyState";
import FilterChip, { FilterChipRow } from "../../../components/FilterChip";
import Input from "../../../components/Input";
import { LockIcon, PinIcon, PlusIcon } from "../../../icons";
import { useTranslation } from "../../../i18n";
import EngineBadge from "../components/EngineBadge";
import { kindLabel } from "./connectionForm";
import { connectionPlace } from "./connectionString";
import { DatabaseIcon } from "../icons";
import type { SavedConnection } from "../types";
import type { SavedList } from "./useSavedList";

interface Props {
  list: SavedList;
  savedConnections: SavedConnection[];
  /** The saved connection the form holds, marked in the list. */
  editingId: string | null;
  onSelect: (entry: SavedConnection) => void;
  onOpen: (entry: SavedConnection) => void;
  onNew: () => void;
}

/** The saved connections beside the form: search, engine chips, pinned first, the one the form
 *  holds marked and kept in view. */
export default function SavedConnectionList({
  list,
  savedConnections,
  editingId,
  onSelect,
  onOpen,
  onNew,
}: Props) {
  const { t } = useTranslation();
  const {
    query: listQuery,
    setQuery: setListQuery,
    kinds: listKinds,
    toggleKind: toggleListKind,
    clearFilters: clearListFilters,
    engines,
    shown: shownConnections,
    openContextMenu,
    listRef: savedListRef,
    headerRef: savedHeaderRef,
    activeRowRef,
    scrollMemory: savedListScrollRef,
  } = list;

  return (
    <aside
      className="saved-list"
      ref={savedListRef}
      onScroll={(e) => {
        savedListScrollRef.current = e.currentTarget.scrollTop;
      }}
    >
      {/* Title, search and chips stay put while the rows scroll under them — one sticky block,
          whose height is what `scrollTopFor` keeps a scrolled-to row clear of. */}
      <div className="saved-list-header" ref={savedHeaderRef}>
        <div className="saved-list-title">
          <h3>{t("connection.connections")}</h3>
          <span className="saved-list-count">{savedConnections.length}</span>
          {/* Creating a connection is an action on the list, not one of its rows, so it sits in
              the header where it stays reachable however far the names scroll. */}
          <Button
            size="small"
            variant="soft"
            className="saved-list-new"
            onClick={onNew}
            title={t("connection.newConnection")}
            aria-label={t("connection.newConnection")}
          >
            <PlusIcon size={15} />
          </Button>
        </div>
        <Input
          allowClear
          value={listQuery}
          onChange={(e) => setListQuery(e.target.value)}
          placeholder={t("connection.searchConnections")}
          aria-label={t("connection.searchConnections")}
        />
        {engines.length > 1 && (
          <FilterChipRow>
            {engines.map(([engine, count]) => (
              <FilterChip
                key={engine}
                compact
                pressed={listKinds.has(engine)}
                count={count}
                title={t(kindLabel(engine))}
                leading={<DatabaseIcon kind={engine} size="0.95em" className={`choice-icon kind-${engine}`} />}
                onClick={() => toggleListKind(engine)}
              >
                {t(kindLabel(engine))}
              </FilterChip>
            ))}
          </FilterChipRow>
        )}
      </div>
      {savedConnections.length > 0 && shownConnections.length === 0 ? (
        <EmptyState
          title={t("connection.noConnectionsMatch")}
          action={
            <Button size="small" onClick={clearListFilters}>
              {t("connection.clearFilters")}
            </Button>
          }
        />
      ) : (
        [
          { key: "pinned", label: t("connection.pinnedGroup"), rows: shownConnections.filter((c) => c.pinned) },
          { key: "all", label: t("connection.connections"), rows: shownConnections.filter((c) => !c.pinned) },
        ]
          // The Pinned group only when something is pinned; the other always, so an empty list
          // still reads as a list.
          .filter((group) => group.key === "all" || group.rows.length > 0)
          .map((group) => (
            <section key={group.key} className="saved-group">
              <h4 className="saved-group-label">
                {group.label}
                <span>{group.rows.length}</span>
              </h4>
              <ul>
                {group.rows.map((c) => (
                  <li key={c.id} ref={c.id === editingId ? activeRowRef : null}>
                    <button
                      type="button"
                      className={`saved-item${c.id === editingId ? " saved-item-active" : ""}${
                        c.readOnly ? " saved-item-readonly" : ""
                      }`}
                      onClick={() => onSelect(c)}
                      onDoubleClick={() => onOpen(c)}
                      onContextMenu={(e) => openContextMenu(e, c.id)}
                      title={t("connection.savedItemTooltip")}
                    >
                      {/* The engine's own logo, in its own tile: the row is recognised by a
                          shape the user already knows from everywhere else. The engine's name
                          is written in the line under the connection's. */}
                      <EngineBadge kind={c.config.kind} size={30} />
                      <span className="saved-item-text">
                        <strong>{c.name}</strong>
                        <span className="saved-item-sub">
                          {t(kindLabel(c.config.kind))}
                          {connectionPlace(c.config) !== "" && ` · ${connectionPlace(c.config)}`}
                        </span>
                      </span>
                      {/* Read-only is about what the row will let you do, so it says the word
                          rather than only drawing a lock — a shape alone would be one more badge
                          to learn. */}
                      {c.readOnly && (
                        <span className="saved-item-readonly-badge">
                          <LockIcon size={12} />
                          {t("common.readOnly")}
                        </span>
                      )}
                      {/* Says why this one sits above the alphabet. The button's own `title`
                          describes the row, so the mark carries its word in a `<span>` for
                          screen readers rather than in a second tooltip that would replace it. */}
                      {c.pinned && (
                        <span className="saved-item-pin">
                          <PinIcon size={14} />
                          <span className="visually-hidden">{t("connection.pinnedTooltip")}</span>
                        </span>
                      )}
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          ))
      )}
    </aside>
  );
}
