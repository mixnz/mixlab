import { useCallback, useMemo, useState } from "react";
import Button from "../../../../components/Button";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import ContextMenu from "../../../../components/ContextMenu";
import Input from "../../../../components/Input";
import NameDialog from "../../../../components/NameDialog";
import Tooltip from "../../../../components/Tooltip";
import { copyText } from "../../../../core/clipboard";
import {
  BackspaceIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  HistoryIcon,
  PinIcon,
  PlusIcon,
} from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import { shortUrl } from "../../format";
import { toCurl } from "../../parsePaste";
import { RECENT_LIMIT } from "../../requests";
import { resolveRequest } from "../../resolveRequest";
import type { RequestLists, RestRequest } from "../../types";
import styles from "./RequestList.module.css";

interface Props {
  lists: RequestLists;
  activeId: string | null;
  /** The chosen environment's values, or null when none is. Only *Copy as cURL* reads them: what
   *  is copied is meant to be run somewhere else, and `{{token}}` runs nowhere but here. */
  vars: Record<string, string> | null;
  onOpen: (id: string) => void;
  onNew: () => void;
  /** Opens the history dialog — the sidebar header is where the spec puts it. */
  onHistory: () => void;
  /** A request with something changed — a rename. */
  onSave: (request: RestRequest) => void;
  onDuplicate: (request: RestRequest) => void;
  /** Recent only: keep this request for good. */
  onPin: (id: string) => void;
  onDelete: (id: string) => void;
}

type Group = "saved" | "recent";

interface MenuState {
  request: RestRequest;
  group: Group;
  x: number;
  y: number;
}

/**
 * The request list: **Saved**, which is what someone chose to keep, and **Recent**, which is what
 * pasting left behind.
 *
 * Recent is empty until Phase 2 puts anything in it, and is drawn anyway: the counter is how the
 * ten-request ceiling is visible before it is hit, and an explanation reads better than a group
 * that appears out of nowhere the first time a cURL command is pasted.
 */
function RequestList({
  lists,
  activeId,
  vars,
  onOpen,
  onNew,
  onHistory,
  onSave,
  onDuplicate,
  onPin,
  onDelete,
}: Props) {
  const { t } = useTranslation();
  const [filter, setFilter] = useState("");
  const [openGroups, setOpenGroups] = useState({ saved: true, recent: true });
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [renaming, setRenaming] = useState<RestRequest | null>(null);
  const [deleting, setDeleting] = useState<RestRequest | null>(null);

  /** What a row shows: the name, else the URL cut down, else that it has neither yet. */
  const label = useCallback(
    (request: RestRequest) =>
      request.name !== "" ? request.name : shortUrl(request.url) || t("rest.untitled"),
    [t],
  );

  // No `lang` here: this holds a function, not words. `t` reads the current dictionary each time
  // it is called, and the rows are filtered afresh on every render, a language switch included.
  const match = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    if (needle === "") return () => true;
    return (request: RestRequest) =>
      label(request).toLowerCase().includes(needle) || request.url.toLowerCase().includes(needle);
  }, [filter, label]);

  /** Deleting from Recent asks nothing: the group empties itself anyway, and the row came from a
   *  paste rather than from a decision. Saved is asked about, as before. */
  function remove(request: RestRequest, group: Group) {
    if (group === "recent") onDelete(request.id);
    else setDeleting(request);
  }

  function rows(list: RestRequest[], emptyMessage: string, group: Group) {
    const shown = list.filter(match);
    if (shown.length === 0) return <p className={`${styles.empty} muted`}>{emptyMessage}</p>;
    return shown.map((request) => (
      /* The row is a button, and a button cannot hold another — so the pin sits beside it in a
         wrapper, which is also what carries the hover. */
      <div key={request.id} className={styles.rowWrap}>
        <button
          type="button"
          className={`${styles.row}${request.id === activeId ? ` ${styles.rowActive}` : ""}`}
          onClick={() => onOpen(request.id)}
          /* Delete on the row the keyboard is on. Backspace too: it is what the finger reaches for
             on a laptop, and this row is not a text field where it would mean anything else. The
             same route as the menu's Delete — the key is a shortcut to it, not past it. */
          onKeyDown={(e) => {
            if (e.key !== "Delete" && e.key !== "Backspace") return;
            e.preventDefault();
            remove(request, group);
          }}
          onContextMenu={(e) => {
            e.preventDefault();
            setMenu({ request, group, x: e.clientX, y: e.clientY });
          }}
        >
          <span className={`${styles.method} rest-method rest-method-${request.method}`}>
            {request.method}
          </span>
          <span className={styles.name}>{label(request)}</span>
        </button>
        {/* Only on the row the keyboard is already on, and only while the pointer is there too —
            a reminder for the hand that is on this row, not another button in the list. */}
        <span className={styles.keyHint}>
          <Tooltip text={t("rest.deleteKeyHint")}>
            <BackspaceIcon size="0.95em" />
          </Tooltip>
        </span>
        {group === "recent" && (
          <button
            type="button"
            className={styles.pin}
            aria-label={t("rest.pin")}
            title={t("rest.pinHint")}
            onClick={() => onPin(request.id)}
          >
            <PinIcon size="0.9em" />
          </button>
        )}
      </div>
    ));
  }

  function group(key: Group, heading: string, list: RestRequest[], empty: string) {
    const open = openGroups[key];
    return (
      <>
        <button
          type="button"
          className={styles.groupHead}
          onClick={() => setOpenGroups((prev) => ({ ...prev, [key]: !prev[key] }))}
          aria-expanded={open}
        >
          {open ? <ChevronDownIcon size="0.9em" /> : <ChevronRightIcon size="0.9em" />}
          {heading}
        </button>
        {open && rows(list, empty, key)}
      </>
    );
  }

  return (
    <div className={styles.sidebar}>
      <div className={styles.header}>
        <div className={styles.headerRow}>
          <Button className={styles.newButton} size="small" onClick={onNew}>
            <PlusIcon size="1em" />
            {t("rest.newRequest")}
          </Button>
          <button
            type="button"
            className={styles.historyButton}
            onClick={onHistory}
            title={t("rest.historyOpen")}
            aria-label={t("rest.historyOpen")}
          >
            <HistoryIcon size="1em" />
          </button>
        </div>
        <Input
          size="small"
          value={filter}
          placeholder={t("rest.filterPlaceholder")}
          aria-label={t("rest.filterPlaceholder")}
          onChange={(e) => setFilter(e.target.value)}
        />
      </div>

      <div className={styles.groups}>
        {group("saved", t("rest.saved"), lists.saved, t("rest.noSaved"))}
        {group(
          "recent",
          t("rest.recent", { n: lists.recent.length, max: RECENT_LIMIT }),
          lists.recent,
          t("rest.noRecent"),
        )}
      </div>

      {menu && (
        <ContextMenu x={menu.x} y={menu.y} onClose={() => setMenu(null)}>
          {menu.group === "recent" && (
            <button
              type="button"
              onClick={() => {
                onPin(menu.request.id);
                setMenu(null);
              }}
            >
              {t("rest.pin")}
            </button>
          )}
          <button
            type="button"
            onClick={() => {
              setRenaming(menu.request);
              setMenu(null);
            }}
          >
            {t("rest.rename")}
          </button>
          <button
            type="button"
            onClick={() => {
              onDuplicate(menu.request);
              setMenu(null);
            }}
          >
            {t("rest.duplicate")}
          </button>
          <button
            type="button"
            onClick={() => {
              // Resolved on the way out, secrets and all. This command is going somewhere that has
              // never heard of this workspace — a terminal, a colleague, a bug report — and every
              // `{{name}}` left in it is a place it would fail. Nothing is written back: what is
              // resolved here is a string on the clipboard and the request keeps its variables.
              // A refusal is reported by `copyText`; the sidebar has no banner to put it on, so it
              // is swallowed rather than left as an unhandled rejection — as the tree's copy is.
              void copyText(toCurl(resolveRequest(menu.request, vars).request)).catch(() => {});
              setMenu(null);
            }}
          >
            {t("rest.copyAsCurl")}
          </button>
          <button
            type="button"
            className="context-menu-delete"
            onClick={() => {
              remove(menu.request, menu.group);
              setMenu(null);
            }}
          >
            {t("rest.delete")}
          </button>
        </ContextMenu>
      )}

      {renaming && (
        <NameDialog
          title={t("rest.renameTitle")}
          ariaLabel={t("rest.renameTitle")}
          label={t("rest.nameLabel")}
          initialName={renaming.name !== "" ? renaming.name : label(renaming)}
          emptyError={t("rest.nameEmpty")}
          submitLabel={t("rest.renameSubmit")}
          savingLabel={t("rest.renameSaving")}
          onCancel={() => setRenaming(null)}
          onSubmit={async (name) => {
            onSave({ ...renaming, name });
            setRenaming(null);
          }}
        />
      )}

      {deleting && (
        <ConfirmDialog
          title={t("rest.deleteTitle")}
          message={t("rest.deleteMessage", { name: label(deleting) })}
          confirmLabel={t("rest.delete")}
          danger
          onCancel={() => setDeleting(null)}
          onConfirm={() => {
            onDelete(deleting.id);
            setDeleting(null);
          }}
        />
      )}
    </div>
  );
}

export default RequestList;
