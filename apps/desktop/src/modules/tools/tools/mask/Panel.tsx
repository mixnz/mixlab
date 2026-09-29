import { useState } from "react";
import Button from "../../../../components/Button";
import { Textarea } from "../../../../components/Input";
import Select, { type SelectOption } from "../../../../components/Select";
import { useTranslation, type TranslationKey } from "../../../../i18n";
import CopyField from "../../components/CopyField";
import Checkbox from "../../../../components/Checkbox";
import { parseCsvRows, rowsToObjects, toCsv } from "../shared/csv";
import {
  MASK_KINDS,
  detectFieldSpecs,
  maskRows,
  parseFlatRows,
  type FieldMaskSpec,
  type MaskKind,
} from "./mask";
import styles from "./Panel.module.css";

type Format = "json" | "csv";

const FORMATS: SelectOption<Format>[] = [
  { value: "json", label: "JSON" },
  { value: "csv", label: "CSV" },
];

const DELIMITERS: SelectOption<string>[] = [
  { value: ",", label: "," },
  { value: ";", label: ";" },
  { value: "\t", label: "Tab" },
  { value: "|", label: "|" },
];

const KIND_LABEL: Record<MaskKind, TranslationKey> = {
  none: "toolbox.mask.kindNone",
  redact: "toolbox.mask.kindRedact",
  partial: "toolbox.mask.kindPartial",
  hash: "toolbox.mask.kindHash",
};

function MaskPanel() {
  const { t } = useTranslation();
  const [format, setFormat] = useState<Format>("json");
  const [delimiter, setDelimiter] = useState(",");
  const [header, setHeader] = useState(true);
  const [input, setInput] = useState("");
  const [rows, setRows] = useState<Record<string, unknown>[] | null>(null);
  const [specs, setSpecs] = useState<FieldMaskSpec[]>([]);
  const [readError, setReadError] = useState<string | null>(null);
  const [output, setOutput] = useState("");

  const kindOptions: SelectOption<MaskKind>[] = MASK_KINDS.map((kind) => ({
    value: kind,
    label: t(KIND_LABEL[kind]),
  }));

  // Rereading always replaces the whole field list with a new guess — earlier manual edits are
  // lost, the same way the Fake data tool handles "Infer fields from sample".
  const readData = () => {
    setOutput("");
    if (format === "json") {
      let parsed: unknown;
      try {
        parsed = JSON.parse(input);
      } catch {
        setRows(null);
        setReadError(t("toolbox.mask.notJson"));
        return;
      }
      const flat = parseFlatRows(parsed);
      if (flat === null) {
        setRows(null);
        setReadError(t("toolbox.mask.needsFlatRows"));
        return;
      }
      setRows(flat);
      setSpecs(detectFieldSpecs(flat));
      setReadError(null);
      return;
    }

    const csvRows = rowsToObjects(parseCsvRows(input, delimiter));
    if (csvRows.length === 0) {
      setRows(null);
      setReadError(t("toolbox.mask.empty"));
      return;
    }
    setRows(csvRows);
    setSpecs(detectFieldSpecs(csvRows));
    setReadError(null);
  };

  const updateKind = (index: number, kind: MaskKind) => {
    setSpecs((prev) => prev.map((spec, i) => (i === index ? { ...spec, kind } : spec)));
  };

  const run = () => {
    if (rows === null) return;
    const masked = maskRows(rows, specs);
    setOutput(format === "json" ? JSON.stringify(masked, null, 2) : toCsv(masked, delimiter, header));
  };

  return (
    <div className={styles.panel}>
      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t("toolbox.input")}</span>
        <Textarea
          value={input}
          onChange={(event) => setInput(event.target.value)}
          placeholder={t("toolbox.mask.placeholder")}
          maxRows={12}
        />
      </label>

      <div className={styles.controls}>
        <Select
          value={format}
          options={FORMATS}
          onChange={setFormat}
          ariaLabel={t("toolbox.mask.format")}
          className={styles.format}
        />
        {format === "csv" ? (
          <>
            <Select
              value={delimiter}
              options={DELIMITERS}
              onChange={setDelimiter}
              ariaLabel={t("toolbox.mask.delimiter")}
            />
            <Checkbox
              className={styles.check}
              label={t("toolbox.mask.header")}
              checked={header}
              onChange={(event) => setHeader(event.target.checked)}
            />
          </>
        ) : null}
        <Button variant="primary" onClick={readData} disabled={input.trim() === ""}>
          {t("toolbox.mask.read")}
        </Button>
      </div>

      {readError ? <p className={styles.error}>{readError}</p> : null}

      {specs.length > 0 ? (
        <div className={styles.fields}>
          {specs.map((spec, index) => (
            <div key={spec.name} className={styles.fieldRow}>
              <span className={styles.name}>{spec.name}</span>
              <Select
                value={spec.kind}
                options={kindOptions}
                onChange={(kind) => updateKind(index, kind)}
                ariaLabel={t("toolbox.mask.fieldKind")}
                className={styles.kind}
              />
            </div>
          ))}
          <Button variant="primary" onClick={run}>
            {t("toolbox.mask.run")}
          </Button>
        </div>
      ) : null}

      {output !== "" ? (
        <>
          <CopyField label={t("toolbox.output")} value={output} multiline />
          <p className={styles.note}>{t("toolbox.mask.note")}</p>
        </>
      ) : null}
    </div>
  );
}

export default MaskPanel;
