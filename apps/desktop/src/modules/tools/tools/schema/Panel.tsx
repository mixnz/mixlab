import { useMemo, useState } from "react";
import Input, { Textarea } from "../../../../components/Input";
import Select, { type SelectOption } from "../../../../components/Select";
import { useTranslation } from "../../../../i18n";
import CopyField from "../../components/CopyField";
import type { SqlDialect } from "../convert/insert";
import { toCreateTable, toGoStruct, toTypeScript } from "./emit";
import { inferSchema } from "./infer";
import styles from "./Panel.module.css";

type Target = "sql" | "ts" | "go";

const TARGETS: SelectOption<Target>[] = [
  { value: "sql", label: "CREATE TABLE" },
  { value: "ts", label: "TypeScript interface" },
  { value: "go", label: "Go struct" },
];

const DIALECTS: SelectOption<SqlDialect>[] = [
  { value: "mysql", label: "MySQL" },
  { value: "postgres", label: "PostgreSQL" },
];

type Outcome = { output: string } | { errorKey: "notJson" | "notObject" };

function SchemaPanel() {
  const { t } = useTranslation();
  const [input, setInput] = useState("");
  const [target, setTarget] = useState<Target>("sql");
  const [table, setTable] = useState("my_table");
  const [dialect, setDialect] = useState<SqlDialect>("mysql");
  const [rootName, setRootName] = useState("Row");

  // Inference is a cheap pure function, so it runs on every keystroke — no button needed like the
  // Convert tool.
  const outcome = useMemo<Outcome | null>(() => {
    if (input.trim() === "") return null;
    let value: unknown;
    try {
      value = JSON.parse(input);
    } catch {
      return { errorKey: "notJson" };
    }
    const fields = inferSchema(value);
    if (!fields) return { errorKey: "notObject" };
    if (target === "sql") return { output: toCreateTable(fields, { table, dialect }) };
    if (target === "ts") return { output: toTypeScript(fields, rootName) };
    return { output: toGoStruct(fields, rootName) };
  }, [input, target, table, dialect, rootName]);

  return (
    <div className={styles.panel}>
      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t("toolbox.input")}</span>
        <Textarea
          value={input}
          onChange={(event) => setInput(event.target.value)}
          placeholder={t("toolbox.schema.placeholder")}
          maxRows={14}
        />
      </label>

      <div className={styles.controls}>
        <Select
          value={target}
          options={TARGETS}
          onChange={setTarget}
          ariaLabel={t("toolbox.schema.target")}
          className={styles.target}
        />
        {target === "sql" ? (
          <>
            <Input
              value={table}
              onChange={(event) => setTable(event.target.value)}
              aria-label={t("toolbox.schema.table")}
              className={styles.name}
            />
            <Select
              value={dialect}
              options={DIALECTS}
              onChange={setDialect}
              ariaLabel={t("toolbox.schema.dialect")}
            />
          </>
        ) : (
          <Input
            value={rootName}
            onChange={(event) => setRootName(event.target.value)}
            aria-label={t("toolbox.schema.rootName")}
            className={styles.name}
          />
        )}
      </div>

      {outcome && "errorKey" in outcome ? (
        <p className={styles.error}>
          {outcome.errorKey === "notJson"
            ? t("toolbox.schema.notJson")
            : t("toolbox.schema.notObject")}
        </p>
      ) : null}

      {outcome && "output" in outcome ? (
        <CopyField label={t("toolbox.output")} value={outcome.output} multiline />
      ) : null}
    </div>
  );
}

export default SchemaPanel;
