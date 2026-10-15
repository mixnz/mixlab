import { useEffect, useImperativeHandle, useMemo, useRef, useState, type Ref } from "react";
import Button from "../../../../components/Button";
import Input from "../../../../components/Input";
import Select from "../../../../components/Select";
import Checkbox from "../../../../components/Checkbox";
import { MinusIcon, PlusIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import {
  arityLookup,
  createFilterRow,
  type FilterOperatorSpec,
  type FilterRow,
} from "../../workspace/filters";
import styles from "./FilterBar.module.css";

/** What the pane above can ask the bar to do — see {@link Props.ref}. */
export interface FilterBarHandle {
  /**
   * Puts the caret in the first value box that takes one, with whatever is in it selected so the
   * next keystroke replaces it.
   *
   * A bar with nowhere to type — no rows at all, or none whose operator reads a value — grows a row
   * first. The shortcut behind this means "write a condition", and a key that does nothing at all
   * on an empty bar is one the user has to learn the exception to.
   */
  focusValue: () => void;
}

interface Props<Op extends string> {
  /** The fields a row may point at — a SQL table's columns, or the properties the documents on a
   * Mongo collection's first page were found to carry. */
  fields: string[];
  operators: readonly FilterOperatorSpec[];
  /** The operator a new row starts on. */
  defaultOperator: Op;
  /** Names an operator in the current language. Kept a callback rather than a key prefix so each
   * database can label the same id after its own query language. */
  operatorLabel: (operator: Op) => string;
  rows: FilterRow<Op>[];
  onChange: (rows: FilterRow<Op>[]) => void;
  /** Runs the rows against the table. The bar edits freely until this is called. */
  onApply: () => void;
  /** Blocks applying while the grid is busy with something else; the rows stay editable. */
  applyDisabled?: boolean;
  /** How the pane above reaches {@link FilterBarHandle} — the grid's `Ctrl+F`, which has to land in
   * a value box from wherever the focus happens to be. */
  ref?: Ref<FilterBarHandle>;
}

/** The row of conditions above the grid. Every row is ANDed with the others, and none of them
 * touch the query until Apply (or Enter in a value box) is pressed — so a half-typed condition
 * never costs a round trip. */
function FilterBar<Op extends string>({
  fields,
  operators,
  defaultOperator,
  operatorLabel,
  rows,
  onChange,
  onApply,
  applyDisabled,
  ref,
}: Props<Op>) {
  const { t } = useTranslation();
  const valueRefs = useRef(new Map<number, HTMLInputElement>());
  /** The row whose value box is owed the focus, and whether the text already in it is to be
   * selected with it. Cleared as soon as the box has been given both. */
  const [pendingFocus, setPendingFocus] = useState<{ id: number; select: boolean } | null>(null);

  // Focusing from an effect rather than straight out of the select handlers, for two reasons:
  // the box is still disabled until the render that a new operator brings, and the select
  // hands focus back to its own trigger on the way out of the handler. The shortcut below adds a
  // third: the row it focuses may be one it has just asked for, and which does not exist yet.
  useEffect(() => {
    if (pendingFocus === null) return;
    const box = valueRefs.current.get(pendingFocus.id);
    box?.focus();
    // Only for the shortcut, which is the start of writing a condition rather than an edit of the
    // one that is there. A field or operator changed keeps its value on purpose, and selecting it
    // would put the next keystroke through it.
    if (pendingFocus.select) box?.select();
    setPendingFocus(null);
  }, [pendingFocus]);

  const operatorArity = useMemo(() => arityLookup<Op>(operators), [operators]);

  useImperativeHandle(
    ref,
    () => ({
      focusValue() {
        const typeable = rows.find((row) => operatorArity(row.operator) !== "none");
        if (typeable) {
          setPendingFocus({ id: typeable.id, select: true });
          return;
        }
        // Nothing here to type into: an empty bar, or one holding only `IS NULL` and its kind. A
        // bar with no fields to point a row at is the grid before its first read has landed.
        if (fields.length === 0) return;
        const row = createFilterRow<Op>(fields, defaultOperator);
        onChange([...rows, row]);
        setPendingFocus({ id: row.id, select: true });
      },
    }),
    [rows, fields, defaultOperator, onChange, operatorArity],
  );
  const fieldOptions = fields.map((c) => ({ value: c, label: c }));
  const operatorOptions = operators.map((op) => ({
    value: op.id as Op,
    label: operatorLabel(op.id as Op),
  }));

  function updateRow(id: number, patch: Partial<FilterRow<Op>>) {
    onChange(rows.map((row) => (row.id === id ? { ...row, ...patch } : row)));
  }

  function changeField(row: FilterRow<Op>, column: string) {
    updateRow(row.id, { column });
    // Same reasoning as the operator below: the field alone is not a condition, so the caret
    // moves on to the value — unless the operator already on the row takes none.
    if (operatorArity(row.operator) !== "none") setPendingFocus({ id: row.id, select: false });
  }

  function changeOperator(row: FilterRow<Op>, operator: Op) {
    // An operator that takes no value leaves the box disabled, and text left sitting in a
    // disabled box reads as if it were still part of the condition.
    const clears = operatorArity(operator) === "none";
    updateRow(row.id, { operator, ...(clears ? { value: "" } : null) });
    // Picking an operator is only ever half of writing a condition, so the value is where the
    // typing goes next — unless the operator is one of the value-less ones.
    if (!clears) setPendingFocus({ id: row.id, select: false });
  }

  function valuePlaceholder(operator: Op): string {
    switch (operatorArity(operator)) {
      case "none":
        return "";
      case "list":
        return t("filterBar.listPlaceholder");
      case "pair":
        return t("filterBar.pairPlaceholder");
      default:
        return t("filterBar.valuePlaceholder");
    }
  }

  return (
    <div className={styles.bar}>
      {rows.length > 0 && (
        <div className={styles.rows}>
          {rows.map((row) => {
            const takesValue = operatorArity(row.operator) !== "none";
            return (
              <div key={row.id} className={styles.row}>
                <Checkbox
                  size="normal"
                  className={styles.toggle}
                  checked={row.enabled}
                  aria-label={t("filterBar.enableFilter")}
                  title={t("filterBar.enableFilter")}
                  onChange={(e) => updateRow(row.id, { enabled: e.target.checked })}
                />
                <Select
                  size="normal"
                  className={styles.field}
                  value={row.column}
                  options={fieldOptions}
                  ariaLabel={t("filterBar.field")}
                  searchable
                  searchPlaceholder={t("filterBar.fieldSearch")}
                  onChange={(column) => changeField(row, column)}
                />
                <Select
                  size="normal"
                  className={styles.operator}
                  value={row.operator}
                  options={operatorOptions}
                  ariaLabel={t("filterBar.operator")}
                  searchable
                  searchPlaceholder={t("filterBar.operatorSearch")}
                  onChange={(operator) => changeOperator(row, operator)}
                />
                <Input
                  ref={(el) => {
                    if (el) valueRefs.current.set(row.id, el);
                    else valueRefs.current.delete(row.id);
                  }}
                  size="normal"
                  className={styles.value}
                  value={row.value}
                  disabled={!takesValue}
                  placeholder={valuePlaceholder(row.operator)}
                  aria-label={t("filterBar.value")}
                  onChange={(e) => updateRow(row.id, { value: e.target.value })}
                  // Enter is the shortcut for the Apply button beneath — a filter is typed and
                  // run far more often than it is typed and left.
                  onKeyDown={(e) => {
                    if (e.key !== "Enter" || applyDisabled) return;
                    e.preventDefault();
                    onApply();
                  }}
                />
                <button
                  type="button"
                  className={styles.iconButton}
                  aria-label={t("filterBar.removeFilter")}
                  title={t("filterBar.removeFilter")}
                  onClick={() => onChange(rows.filter((r) => r.id !== row.id))}
                >
                  <MinusIcon size={14} />
                </button>
              </div>
            );
          })}
        </div>
      )}
      <div className={styles.actions}>
        <button
          type="button"
          className={styles.iconButton}
          aria-label={t("filterBar.addFilter")}
          title={t("filterBar.addFilter")}
          disabled={fields.length === 0}
          onClick={() => onChange([...rows, createFilterRow(fields, defaultOperator)])}
        >
          <PlusIcon size={14} />
        </button>
        <Button size="normal" variant="primary" disabled={applyDisabled} onClick={onApply}>
          {t("filterBar.apply")}
        </Button>
      </div>
    </div>
  );
}

export default FilterBar;
