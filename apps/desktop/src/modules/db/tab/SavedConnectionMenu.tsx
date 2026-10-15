import ContextMenu from "../../../components/ContextMenu";
import { useTranslation } from "../../../i18n";
import type { SavedConnection } from "../types";
import type { SavedList } from "./useSavedList";

interface Props {
  list: SavedList;
  savedConnections: SavedConnection[];
  onDelete: (id: string) => void;
}

/** A saved connection's right-click menu: pin, read-only, duplicate, delete. */
export default function SavedConnectionMenu({ list, savedConnections, onDelete }: Props) {
  const { t } = useTranslation();
  const { contextMenu, closeContextMenu, togglePinned, toggleReadOnly, duplicate: duplicateSavedConnection } = list;
  if (!contextMenu) return null;
  return (
    <ContextMenu x={contextMenu.x} y={contextMenu.y} onClose={closeContextMenu}>
      <button
        type="button"
        onClick={() => {
          togglePinned(contextMenu.id);
          closeContextMenu();
        }}
      >
        {savedConnections.find((c) => c.id === contextMenu.id)?.pinned
          ? t("connection.unpin")
          : t("connection.pin")}
      </button>
      {/* Offered whatever the kind: each workspace closes off everything of its own that
          would write, so the flag means the same thing on a Mongo or Redis connection as it
          does on a MySQL one. */}
      <button
        type="button"
        onClick={() => {
          toggleReadOnly(contextMenu.id);
          closeContextMenu();
        }}
      >
        {savedConnections.find((c) => c.id === contextMenu.id)?.readOnly
          ? t("connection.allowWrites")
          : t("connection.markReadOnly")}
      </button>
      <button
        type="button"
        onClick={() => {
          duplicateSavedConnection(contextMenu.id);
          closeContextMenu();
        }}
      >
        {t("common.duplicate")}
      </button>
      <button
        type="button"
        className="context-menu-delete"
        onClick={() => {
          onDelete(contextMenu.id);
          closeContextMenu();
        }}
      >
        {t("common.delete")}
      </button>
    </ContextMenu>
  );
}
