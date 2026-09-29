import { useState } from "react";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import ContextMenu from "../../../../components/ContextMenu";
import { GlobeIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import { ShellIcon } from "../../icons";
import { shellLabel } from "../../shells";
import type { SavedTarget } from "../../types";
import styles from "./SavedTargetList.module.css";

interface Props {
  targets: SavedTarget[];
  /** The target currently loaded in the form, to highlight the right row. */
  selectedId: string | null;
  onSelect: (target: SavedTarget) => void;
  /** Double-click: loads the target *and* opens the session right away. For a place used often
   *  every field in the form is already right, so making people load it and then press Connect is
   *  making them click twice for one intention. */
  onOpen: (target: SavedTarget) => void;
  onDelete: (id: string) => void;
  /** Clears the form — an action on the list, not on any of its rows, so it sits at the top of
   *  the column rather than in a row's menu. */
  onNew: () => void;
}

/** Where the menu is open, and on which target. */
interface MenuState {
  target: SavedTarget;
  x: number;
  y: number;
}

/** The secondary line under the name: enough to tell two entries with the same name apart, no
 *  longer than that. */
function subtitle(target: SavedTarget): string {
  if (target.kind === "ssh") return `${target.config.username}@${target.config.host}`;
  const label = shellLabel(target.shellName);
  return target.cwd ? `${label} · ${target.cwd}` : label;
}

/**
 * The saved targets column — both shells on this machine and SSH servers, mixed in one list.
 *
 * Mixed because this column is *the places I open often*, not a server list: it still shows while
 * the form is on "This machine", and it showed like that before the local branch existed. What
 * tells the two kinds apart is the leading marker plus the secondary line, not two groups with
 * their own headings — with five or seven entries, two headings take up more room than they have
 * to say.
 *
 * Drawn by hand rather than with `ItemList`: that one reports back by name, and two entries with
 * the same name — common with "prod" — cannot be told apart. This list goes by `id`.
 */
function SavedTargetList({ targets, selectedId, onSelect, onOpen, onDelete, onNew }: Props) {
  const { t } = useTranslation();
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [confirming, setConfirming] = useState<SavedTarget | null>(null);

  return (
    <aside className={styles.list}>
      <div className={styles.header}>
        <h3>{t("terminal.savedTargets")}</h3>
        <button type="button" className={styles.new} onClick={onNew} title={t("terminal.newTarget")}>
          +<span className="visually-hidden">{t("terminal.newTarget")}</span>
        </button>
      </div>

      {targets.length === 0 ? (
        <p className={styles.empty}>{t("terminal.noTargets")}</p>
      ) : (
        <ul>
          {targets.map((target) => (
            <li key={target.id}>
              <button
                type="button"
                className={`${styles.item}${target.id === selectedId ? ` ${styles.itemActive}` : ""}`}
                onClick={() => onSelect(target)}
                onDoubleClick={() => onOpen(target)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenu({ target, x: e.clientX, y: e.clientY });
                }}
              >
                <span className={styles.title}>
                  {/* The shell's logo for this machine; a globe for SSH, because what it says is
                      "somewhere else on the network" rather than "a terminal" — both rows are
                      terminals. */}
                  {target.kind === "local" ? (
                    <ShellIcon name={target.shellName} className={styles.mark} />
                  ) : (
                    <GlobeIcon size="1em" className={styles.mark} />
                  )}
                  <strong>{target.name}</strong>
                </span>
                <span className={styles.endpoint}>{subtitle(target)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}

      {menu && (
        <ContextMenu x={menu.x} y={menu.y} onClose={() => setMenu(null)}>
          <button
            type="button"
            onClick={() => {
              setConfirming(menu.target);
              setMenu(null);
            }}
          >
            {t("terminal.deleteTarget")}
          </button>
        </ContextMenu>
      )}

      {confirming && (
        <ConfirmDialog
          title={t("terminal.deleteTargetTitle")}
          /* Only a server has anything in the credential store to lose, so only it says that
             sentence — promising to delete the password of a shell on this machine is promising
             something that is not real. */
          message={
            confirming.kind === "ssh"
              ? t("terminal.deleteTargetMessageSsh", { name: confirming.name })
              : t("terminal.deleteTargetMessage", { name: confirming.name })
          }
          confirmLabel={t("terminal.deleteTarget")}
          danger
          onConfirm={() => {
            onDelete(confirming.id);
            setConfirming(null);
          }}
          onCancel={() => setConfirming(null)}
        />
      )}
    </aside>
  );
}

export default SavedTargetList;
