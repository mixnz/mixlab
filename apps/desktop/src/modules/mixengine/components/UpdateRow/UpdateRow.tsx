import Button from "../../../../components/Button";
import { useTranslation } from "../../../../i18n";
import type { UpdateRowState } from "../../updateRow";
import styles from "./UpdateRow.module.css";

interface Props<U extends { to: string }> {
  state: UpdateRowState<U>;
  /** How many columns the installed table has, so the row spans all of them. */
  columns: number;
  onUpdate: (update: U) => void;
}

/**
 * The row under an installed version that has a newer patch in its line — T193.
 *
 * **A row of its own, so an update never widens a column.** The button and the progress used to sit
 * in the version cell, where every progress message changed the cell's width and the table
 * re-laid every row. Here the bar has a fixed width, the message is cut to what fits, and the
 * content is kept out of the table's width calculation altogether.
 */
export default function UpdateRow<U extends { to: string }>({ state, columns, onUpdate }: Props<U>) {
  const { t } = useTranslation();

  if (state.kind === "none") return null;

  return (
    <tr className={styles.row}>
      <td colSpan={columns}>
        <div className={styles.body}>
          {state.kind === "offer" ? (
            <Button size="small" variant="soft" onClick={() => onUpdate(state.update)}>
              {t("mixengine.upgrade.available", { to: state.update.to })}
            </Button>
          ) : (
            <>
              <progress className={styles.bar} value={state.percent} max={100} />
              <span className={styles.message} title={state.message}>
                {state.message}
              </span>
            </>
          )}
        </div>
      </td>
    </tr>
  );
}
