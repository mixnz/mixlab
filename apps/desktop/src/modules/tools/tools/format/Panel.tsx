import { useState } from "react";
import { format as formatSqlText } from "sql-formatter";
import Button from "../../../../components/Button";
import { Textarea } from "../../../../components/Input";
import Select, { type SelectOption } from "../../../../components/Select";
import { useTranslation, type TranslationKey } from "../../../../i18n";
import CopyField from "../../components/CopyField";
import { detectFormat, type FormatKind } from "./detect";
import { formatJson, minifyJson } from "./json";
import { minifySql } from "./sql";
import { formatXml, minifyXml } from "./xml";
import styles from "./Panel.module.css";

type KindChoice = FormatKind | "auto";

/** `sql-formatter`'s dialect names. Different from `convert/insert.ts`'s `SqlDialect`, deliberately
 *  not sharing the name. */
type FormatterLanguage = "mysql" | "postgresql" | "sqlite" | "sql";

/** The label is the very thing it produces or the dialect's own name, so it is not translated. */
const INDENTS: SelectOption<string>[] = [
  { value: "  ", label: "2 spaces" },
  { value: "    ", label: "4 spaces" },
  { value: "\t", label: "Tab" },
];

const LANGUAGES: SelectOption<FormatterLanguage>[] = [
  { value: "mysql", label: "MySQL" },
  { value: "postgresql", label: "PostgreSQL" },
  { value: "sqlite", label: "SQLite" },
  { value: "sql", label: "Standard SQL" },
];

const GUESS_KEY: Record<FormatKind, TranslationKey> = {
  json: "toolbox.format.guessedJson",
  xml: "toolbox.format.guessedXml",
  sql: "toolbox.format.guessedSql",
};

interface Outcome {
  output: string;
  error: string | null;
}

function FormatPanel() {
  const { t } = useTranslation();

  // Built inside the component because the "auto-detect" entry is a string to translate; the other
  // three are format names.
  const kinds: SelectOption<KindChoice>[] = [
    { value: "auto", label: t("toolbox.format.auto") },
    { value: "json", label: "JSON" },
    { value: "xml", label: "XML" },
    { value: "sql", label: "SQL" },
  ];

  const [input, setInput] = useState("");
  const [choice, setChoice] = useState<KindChoice>("auto");
  const [indent, setIndent] = useState("  ");
  const [language, setLanguage] = useState<FormatterLanguage>("mysql");
  const [outcome, setOutcome] = useState<Outcome | null>(null);

  const guessed = detectFormat(input);
  const kind: FormatKind | null = choice === "auto" ? guessed : choice;

  const run = (minify: boolean): void => {
    if (kind === null) return;
    if (kind === "json") {
      const result = minify ? minifyJson(input) : formatJson(input, indent);
      setOutcome(
        result.ok
          ? { output: result.output, error: null }
          : { output: "", error: t("toolbox.format.errorAt", { ...result.error }) },
      );
      return;
    }
    if (kind === "xml") {
      const result = minify ? minifyXml(input) : formatXml(input, indent);
      setOutcome(
        result.ok
          ? { output: result.output, error: null }
          : { output: "", error: t("toolbox.format.errorAt", { ...result.error }) },
      );
      return;
    }
    // `sql-formatter` throws when the statement cannot be read; its message already points at the
    // right place.
    try {
      const output = minify
        ? minifySql(input)
        : formatSqlText(input, {
            language,
            tabWidth: indent.length,
            useTabs: indent === "\t",
          });
      setOutcome({ output, error: null });
    } catch (error) {
      setOutcome({ output: "", error: error instanceof Error ? error.message : String(error) });
    }
  };

  return (
    <div className={styles.panel}>
      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t("toolbox.input")}</span>
        <Textarea
          value={input}
          onChange={(event) => setInput(event.target.value)}
          placeholder={t("toolbox.format.placeholder")}
          maxRows={14}
        />
      </label>

      {/* State the guess. A silent guess that is wrong leaves the user no way of knowing. */}
      {choice === "auto" && guessed ? <p className={styles.guess}>{t(GUESS_KEY[guessed])}</p> : null}

      <div className={styles.controls}>
        <Select
          value={choice}
          options={kinds}
          onChange={setChoice}
          ariaLabel={t("toolbox.format.kind")}
          className={styles.kind}
        />
        <Select
          value={indent}
          options={INDENTS}
          onChange={setIndent}
          ariaLabel={t("toolbox.format.indent")}
          className={styles.indent}
        />
        {kind === "sql" ? (
          <Select
            value={language}
            options={LANGUAGES}
            onChange={setLanguage}
            ariaLabel={t("toolbox.format.dialect")}
            className={styles.dialect}
          />
        ) : null}
        <Button variant="primary" onClick={() => run(false)} disabled={kind === null}>
          {t("toolbox.format.formatAction")}
        </Button>
        <Button onClick={() => run(true)} disabled={kind === null}>
          {t("toolbox.format.minifyAction")}
        </Button>
      </div>

      {outcome?.error ? <p className={styles.error}>{outcome.error}</p> : null}
      {outcome && !outcome.error ? (
        <CopyField label={t("toolbox.output")} value={outcome.output} multiline />
      ) : null}
    </div>
  );
}

export default FormatPanel;
