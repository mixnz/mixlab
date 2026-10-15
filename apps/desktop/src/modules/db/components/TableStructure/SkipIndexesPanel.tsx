import ActionBar from "../../../../components/ActionBar";
import { useTranslation } from "../../../../i18n";
import { PencilIcon, PlusIcon, TrashIcon } from "../../../../icons";
import type { SqlSkipIndex } from "../../types";
import styles from "./TableStructure.module.css";

interface Props {
  skipIndexes: SqlSkipIndex[];
  loading: boolean;
  noWrites: boolean;
  noWritesHint: string | undefined;
  onAdd: () => void;
  onEdit: (index: SqlSkipIndex) => void;
  onDrop: (name: string) => void;
}

/** ClickHouse's data-skipping indexes, which no other engine has. */
export default function SkipIndexesPanel({ skipIndexes, loading, noWrites, noWritesHint, onAdd, onEdit, onDrop }: Props) {
  const { t } = useTranslation();
  return (
    <section className={styles.panel}>
      <header className={styles.panelHeader}>
        <h4 className={styles.panelTitle}>{t("structure.skipIndexesTitle")}</h4>
        <ActionBar
          actions={[
            {
              key: "add",
              icon: PlusIcon,
              label: t("structure.addSkipIndex"),
              disabled: noWrites,
              disabledHint: noWritesHint,
              onClick: () => onAdd(),
            },
          ]}
        />
      </header>
      <div className={styles.gridWrap}>
        <table className={styles.grid}>
          <thead>
            <tr>
              <th>{t("structure.skipIndexName")}</th>
              <th>{t("structure.skipIndexExpr")}</th>
              <th>{t("structure.skipIndexType")}</th>
              <th>{t("structure.skipIndexGranularity")}</th>
              <th className={styles.rowActions} />
            </tr>
          </thead>
          <tbody>
            {skipIndexes.map((index) => (
              <tr key={index.name}>
                <td>
                  <span className={styles.name}>{index.name}</span>
                </td>
                <td className={styles.mono}>{index.expr}</td>
                <td className={styles.mono}>
                  {index.args.length === 0
                    ? index.indexType
                    : `${index.indexType}(${index.args.join(", ")})`}
                </td>
                <td className={styles.muted}>{index.granularity}</td>
                <td className={styles.rowActions}>
                  <div className={styles.rowButtons}>
                    <button
                      type="button"
                      className={styles.iconButton}
                      disabled={noWrites}
                      title={noWritesHint ?? t("structure.editIndex")}
                      aria-label={t("structure.editIndex")}
                      onClick={() => onEdit(index)}
                    >
                      <PencilIcon size={14} />
                    </button>
                    <button
                      type="button"
                      className={`${styles.iconButton} ${styles.danger}`}
                      disabled={noWrites}
                      title={noWritesHint ?? t("structure.dropIndex")}
                      aria-label={t("structure.dropIndex")}
                      onClick={() =>
                        onDrop(index.name)
                      }
                    >
                      <TrashIcon size={14} />
                    </button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {!loading && skipIndexes.length === 0 && (
          <p className="muted">{t("structure.noSkipIndexes")}</p>
        )}
      </div>
    </section>
  );
}
