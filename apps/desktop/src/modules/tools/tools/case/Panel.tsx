import { useMemo, useState } from "react";
import { Textarea } from "../../../../components/Input";
import Select, { type SelectOption } from "../../../../components/Select";
import { useTranslation } from "../../../../i18n";
import CopyField from "../../components/CopyField";
import { CASE_STYLES, convert, type CaseStyle } from "./caseConvert";
import styles from "./Panel.module.css";

const STYLE_LABEL: Record<CaseStyle, string> = {
  camel: "camelCase",
  snake: "snake_case",
  kebab: "kebab-case",
  pascal: "PascalCase",
  constant: "CONSTANT_CASE",
  dot: "dot.case",
  title: "Title Case",
};

/* The label is the very syntax it produces, so it is not translated: `snake_case` is called
   snake_case in every language, and a translation only makes the reader guess backwards. */
const STYLE_OPTIONS: SelectOption<CaseStyle>[] = CASE_STYLES.map((style) => ({
  value: style,
  label: STYLE_LABEL[style],
}));

function CasePanel() {
  const { t } = useTranslation();
  const [input, setInput] = useState("");
  const [style, setStyle] = useState<CaseStyle>("snake");

  // Line by line: the real use is copying a whole column list from the db tab and pasting it here.
  const output = useMemo(
    () =>
      input
        .split("\n")
        .map((line) => convert(line, style))
        .join("\n"),
    [input, style],
  );

  return (
    <div className={styles.panel}>
      <div className={styles.controls}>
        <Select
          value={style}
          options={STYLE_OPTIONS}
          onChange={setStyle}
          ariaLabel={t("toolbox.case.style")}
          className={styles.style}
        />
      </div>

      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t("toolbox.input")}</span>
        <Textarea
          value={input}
          onChange={(event) => setInput(event.target.value)}
          placeholder={t("toolbox.case.placeholder")}
          maxRows={14}
        />
      </label>

      <CopyField label={t("toolbox.output")} value={output} multiline />
    </div>
  );
}

export default CasePanel;
