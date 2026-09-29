import { useState } from "react";
import Button from "../../../../components/Button";
import Input, { Textarea } from "../../../../components/Input";
import Select, { type SelectOption } from "../../../../components/Select";
import { useTranslation } from "../../../../i18n";
import CopyField from "../../components/CopyField";
import Checkbox from "../../../../components/Checkbox";
import type { SqlDialect } from "./insert";
import { convertData, type ConvertResult, type ReadFormat, type WriteFormat } from "./pivot";
import styles from "./Panel.module.css";

const FROM: SelectOption<ReadFormat>[] = [
  { value: "json", label: "JSON" },
  { value: "yaml", label: "YAML" },
  { value: "csv", label: "CSV" },
];

const TO: SelectOption<WriteFormat>[] = [...FROM, { value: "insert", label: "SQL INSERT" }];

const DELIMITERS: SelectOption<string>[] = [
  { value: ",", label: "," },
  { value: ";", label: ";" },
  { value: "\t", label: "Tab" },
  { value: "|", label: "|" },
];

const DIALECTS: SelectOption<SqlDialect>[] = [
  { value: "mysql", label: "MySQL" },
  { value: "postgres", label: "PostgreSQL" },
];

function ConvertPanel() {
  const { t } = useTranslation();
  const [input, setInput] = useState("");
  const [from, setFrom] = useState<ReadFormat>("json");
  const [to, setTo] = useState<WriteFormat>("yaml");
  const [delimiter, setDelimiter] = useState(",");
  const [header, setHeader] = useState(true);
  const [table, setTable] = useState("my_table");
  const [dialect, setDialect] = useState<SqlDialect>("mysql");
  const [multiRow, setMultiRow] = useState(false);
  const [result, setResult] = useState<ConvertResult | null>(null);
  // The first click still has to download the js-yaml chunk, so the button has to say what it is
  // doing.
  const [busy, setBusy] = useState(false);

  const usesCsv = from === "csv" || to === "csv";
  const usesInsert = to === "insert";

  const run = (): void => {
    setBusy(true);
    void convertData(input, from, to, { delimiter, header, table, dialect, multiRow })
      .then(setResult)
      .finally(() => setBusy(false));
  };

  const failure = result && !result.ok ? result.failure : null;
  const message =
    failure === null
      ? null
      : failure.reason === "parse"
        ? t("toolbox.convert.failedParse", { detail: failure.detail })
        : failure.reason === "empty"
          ? t("toolbox.convert.failedEmpty")
          : failure.reason === "same"
            ? t("toolbox.convert.failedSame")
            : t("toolbox.convert.failedNeedsRows");

  return (
    <div className={styles.panel}>
      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t("toolbox.input")}</span>
        <Textarea
          value={input}
          onChange={(event) => setInput(event.target.value)}
          placeholder={t("toolbox.convert.placeholder")}
          maxRows={14}
        />
      </label>

      <div className={styles.controls}>
        <Select
          value={from}
          options={FROM}
          onChange={setFrom}
          ariaLabel={t("toolbox.convert.from")}
          className={styles.format}
        />
        <Select
          value={to}
          options={TO}
          onChange={setTo}
          ariaLabel={t("toolbox.convert.to")}
          className={styles.format}
        />
        {usesCsv ? (
          <>
            <Select
              value={delimiter}
              options={DELIMITERS}
              onChange={setDelimiter}
              ariaLabel={t("toolbox.convert.delimiter")}
            />
            <Checkbox
              className={styles.check}
              label={t("toolbox.convert.header")}
              checked={header}
              onChange={(event) => setHeader(event.target.checked)}
            />
          </>
        ) : null}
        {usesInsert ? (
          <>
            <Input
              value={table}
              onChange={(event) => setTable(event.target.value)}
              aria-label={t("toolbox.convert.table")}
              className={styles.table}
            />
            <Select
              value={dialect}
              options={DIALECTS}
              onChange={setDialect}
              ariaLabel={t("toolbox.convert.dialect")}
            />
            <Checkbox
              className={styles.check}
              label={t("toolbox.convert.multiRow")}
              checked={multiRow}
              onChange={(event) => setMultiRow(event.target.checked)}
            />
          </>
        ) : null}
        <Button variant="primary" onClick={run} disabled={busy || input.trim() === ""}>
          {busy ? t("common.loading") : t("toolbox.convert.run")}
        </Button>
      </div>

      {message ? <p className={styles.error}>{message}</p> : null}

      {result?.ok ? (
        <>
          <CopyField label={t("toolbox.output")} value={result.output} multiline />
          {result.warnings.includes("precision") ? (
            <p className={styles.warning}>{t("toolbox.convert.warningPrecision")}</p>
          ) : null}
          {to === "insert" ? <p className={styles.note}>{t("toolbox.convert.insertNote")}</p> : null}
        </>
      ) : null}
    </div>
  );
}

export default ConvertPanel;
