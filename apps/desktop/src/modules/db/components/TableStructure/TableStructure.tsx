import LoadingOverlay from "../../../../components/LoadingOverlay";
import { useReloadShortcut } from "../../../../core/reload";
import { useTranslation } from "../../../../i18n";
import { useSqlDialect } from "../../sql/context";
import ColumnsPanel from "./ColumnsPanel";
import IndexesPanel from "./IndexesPanel";
import SkipIndexesPanel from "./SkipIndexesPanel";
import StructureDialogs from "./StructureDialogs";
import styles from "./TableStructure.module.css";
import { useStructureEdits } from "./useStructureEdits";
import { useTableShape, type StructureCache } from "./useTableShape";

interface Props {
  /** Whether this is what the user is actually looking at — the Structure tab, in the connection
   *  tab the tab bar is showing. This stays mounted behind both, so it is what says when the shape
   *  is worth reading, when a read the user cannot see would be wasted, and which of the panes
   *  mounted at once `Ctrl+R` belongs to. */
  active: boolean;
  connectionId: string;
  selectedDb: string;
  selectedTable: string;
  onError: (message: string) => void;
  /** Where the shape read for each table is kept between visits — see {@link StructureCache}. */
  structureCache: StructureCache;
  /** Which shape of this database the cache is allowed to speak for. Moved by the workspace
   *  whenever the app changes that shape — a table created, renamed or dropped, a column altered, a
   *  dump restored — so that columns read from the shape before are read again rather than shown.
   *  A name is not a promise: a table dropped and made again under the same name would otherwise
   *  open on the columns of the one it replaced. */
  schemaToken: number;
  /** Told that an `ALTER TABLE` has just landed. The panel's own cache is not the only thing now
   *  out of date — the Data tab's rows are about columns that have moved, and the Query tab
   *  completes from a copy of the shape — so this is the workspace's to answer, not this panel's. */
  onSchemaChanged: () => void;
  /** The saved connection is marked as one nothing is written to. The columns and indexes are
   *  still read and shown; every `ALTER TABLE` this panel can send is what goes. */
  readOnly?: boolean;
}

/**
 * The table's shape: its columns above, its indexes below, each with its own add/edit/drop.
 *
 * Every change is one `ALTER TABLE` sent by itself and followed by a reload, rather than a batch
 * of pending edits applied together — a rejected ALTER then names the one thing that was wrong
 * with it, and what the grid shows afterwards is what the server actually has.
 *
 * Read once and kept: the workspace leaves this mounted while another tab is up, so moving between
 * the tabs shows the shape already read rather than asking for it again. It is only re-read when
 * the table changes under it, when an `ALTER` this panel sent means the server no longer matches,
 * or when the reload asks; a table selected while the tab was hidden waits until it is looked at
 * again before costing anything.
 */
function TableStructure({
  active,
  connectionId,
  selectedDb,
  selectedTable,
  onError,
  structureCache,
  schemaToken,
  onSchemaChanged,
  readOnly = false,
}: Props) {
  const { t } = useTranslation();
  const dialect = useSqlDialect();
  const offers = dialect.editing;
  const { tableKey, loading, collations, columns, indexes, skipIndexes, engine, reload } = useTableShape({
    active,
    connectionId,
    selectedDb,
    selectedTable,
    schemaToken,
    structureCache,
    onError,
  });
  const edits = useStructureEdits({ connectionId, selectedDb, selectedTable, onError, onSchemaChanged });
  const { saving, columnDialog, indexDialog, pendingDrop } = edits;

  const busy = loading || saving;

  /** What every button that would send an `ALTER TABLE` is gated on. The reload beside them is
   *  not: reading is the one thing a read-only connection is for. */
  const noWrites = busy || readOnly;
  /** Why they are greyed out, when it is not simply that the panel is mid-request. */
  const noWritesHint = readOnly ? t("common.readOnlyConnection") : undefined;

  // Gated on the same state the reload button (`ColumnsPanel`) is: a re-read asked for while one is
  // already out, or over an `ALTER` still running, is one the button would refuse. Not from behind
  // the column or index form either — that is a half-filled definition the keyboard belongs to —
  // nor from behind the drop confirmation, which is a question about the very thing a reload would
  // replace.
  useReloadShortcut(
    active && columnDialog === null && indexDialog === null && pendingDrop === null,
    () => {
      if (busy) return;
      reload();
    }
  );

  return (
    <div className={styles.structure}>
      <ColumnsPanel
        columns={columns}
        tableKey={tableKey}
        loading={loading}
        busy={busy}
        noWrites={noWrites}
        noWritesHint={noWritesHint}
        offers={offers}
        onReload={reload}
        onAdd={() => edits.setColumnDialog({})}
        onEdit={(column) => edits.setColumnDialog({ column })}
        onDrop={(name) => edits.setPendingDrop({ kind: "column", name })}
      />

      <IndexesPanel
        indexes={indexes}
        columnCount={columns.length}
        loading={loading}
        noWrites={noWrites}
        noWritesHint={noWritesHint}
        offers={offers}
        dialectKind={dialect.kind}
        engine={engine}
        onAdd={() => edits.setIndexDialog({})}
        onEdit={(index) => edits.setIndexDialog({ index })}
        onEditSortingKey={() => edits.setOrderByDialogOpen(true)}
        onDrop={(name) => edits.setPendingDrop({ kind: "index", name })}
      />

      {dialect.kind === "clickhouse" && (
        <SkipIndexesPanel
          skipIndexes={skipIndexes}
          loading={loading}
          noWrites={noWrites}
          noWritesHint={noWritesHint}
          onAdd={() => edits.setSkipIndexDialog({})}
          onEdit={(index) => edits.setSkipIndexDialog({ index })}
          onDrop={(name) => edits.setPendingDrop({ kind: "skipIndex", name })}
        />
      )}

      {(loading || saving) && (
        <LoadingOverlay label={saving ? t("structure.saving") : t("structure.loading")} />
      )}

      <StructureDialogs
        table={selectedTable}
        columns={columns}
        indexes={indexes}
        collations={collations}
        edits={edits}
      />
    </div>
  );
}

export default TableStructure;
export type { RememberedStructure, StructureCache } from "./useTableShape";
