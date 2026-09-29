import { useState } from "react";
import Button from "../../../../components/Button";
import Input, { Textarea } from "../../../../components/Input";
import Select, { type SelectOption } from "../../../../components/Select";
import { useTranslation, type TranslationKey } from "../../../../i18n";
import CopyField from "../../components/CopyField";
import Checkbox from "../../../../components/Checkbox";
import {
  FIELD_KINDS,
  LOCALES,
  LOCALE_KINDS,
  generate,
  inferFields,
  type FieldKind,
  type FieldSpec,
  type Locale,
} from "./fake";
import styles from "./Panel.module.css";

const MAX_COUNT = 1000;

const KIND_LABEL: Record<FieldKind, TranslationKey> = {
  fullName: "toolbox.fake.kindFullName",
  firstName: "toolbox.fake.kindFirstName",
  middleName: "toolbox.fake.kindMiddleName",
  lastName: "toolbox.fake.kindLastName",
  email: "toolbox.fake.kindEmail",
  phone: "toolbox.fake.kindPhone",
  address: "toolbox.fake.kindAddress",
  city: "toolbox.fake.kindCity",
  company: "toolbox.fake.kindCompany",
  uuid: "toolbox.fake.kindUuid",
  integer: "toolbox.fake.kindInteger",
  float: "toolbox.fake.kindFloat",
  boolean: "toolbox.fake.kindBoolean",
  date: "toolbox.fake.kindDate",
  word: "toolbox.fake.kindWord",
  sentence: "toolbox.fake.kindSentence",
  constant: "toolbox.fake.kindConstant",
};

let nextFieldId = 0;

function blankField(): FieldSpec {
  nextFieldId += 1;
  return { name: `field${nextFieldId}`, kind: "fullName" };
}

/** `crypto.getRandomValues` wrapped as `() => number` in [0, 1) — the shape `generate()` needs,
 *  and also the shape a test passes a deterministic LCG in as instead. */
function cryptoRnd(): () => number {
  const buf = new Uint32Array(1);
  return () => {
    crypto.getRandomValues(buf);
    return buf[0]! / 4294967296;
  };
}

function numberOrUndefined(text: string): number | undefined {
  return text === "" ? undefined : Number(text);
}

function FakePanel() {
  const { t } = useTranslation();
  const [fields, setFields] = useState<FieldSpec[]>(() => [blankField()]);
  const [count, setCount] = useState(10);
  const [sample, setSample] = useState("");
  const [sampleError, setSampleError] = useState<string | null>(null);
  const [output, setOutput] = useState("");

  const kindOptions: SelectOption<FieldKind>[] = FIELD_KINDS.map((kind) => ({
    value: kind,
    label: t(KIND_LABEL[kind]),
  }));

  const localeOptions: SelectOption<Locale>[] = LOCALES.map((locale) => ({
    value: locale,
    label: t(locale === "vi" ? "toolbox.fake.localeVi" : "toolbox.fake.localeEn"),
  }));

  const updateField = (index: number, patch: Partial<FieldSpec>) => {
    setFields((prev) => prev.map((field, i) => (i === index ? { ...field, ...patch } : field)));
  };

  const removeField = (index: number) => setFields((prev) => prev.filter((_, i) => i !== index));

  const addField = () => setFields((prev) => [...prev, blankField()]);

  const inferFromSample = () => {
    try {
      const parsed: unknown = JSON.parse(sample);
      const inferred = inferFields(parsed);
      if (inferred === null || inferred.length === 0) {
        setSampleError(t("toolbox.fake.notObject"));
        return;
      }
      setFields(inferred);
      setSampleError(null);
    } catch {
      setSampleError(t("toolbox.fake.notJson"));
    }
  };

  const trimmedNames = fields.map((field) => field.name.trim());
  const hasBlankName = trimmedNames.some((name) => name === "");
  const hasDuplicateName = trimmedNames.some(
    (name, i) => name !== "" && trimmedNames.indexOf(name) !== i,
  );
  const canGenerate = fields.length > 0 && !hasBlankName && !hasDuplicateName;

  const runGenerate = () => {
    setOutput(JSON.stringify(generate(fields, count, cryptoRnd()), null, 2));
  };

  return (
    <div className={styles.panel}>
      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t("toolbox.fake.sample")}</span>
        <Textarea
          value={sample}
          onChange={(event) => setSample(event.target.value)}
          placeholder={t("toolbox.fake.samplePlaceholder")}
          maxRows={6}
        />
      </label>

      <div className={styles.controls}>
        <Button onClick={inferFromSample} disabled={sample.trim() === ""}>
          {t("toolbox.fake.infer")}
        </Button>
      </div>
      {sampleError ? <p className={styles.empty}>{sampleError}</p> : null}

      <div className={styles.rows}>
        {fields.length === 0 ? <p className={styles.empty}>{t("toolbox.fake.noFields")}</p> : null}
        {fields.map((field, index) => (
          <div key={index} className={styles.row}>
            <Input
              value={field.name}
              onChange={(event) => updateField(index, { name: event.target.value })}
              aria-label={t("toolbox.fake.fieldName")}
              className={styles.name}
            />
            <Select
              value={field.kind}
              options={kindOptions}
              onChange={(kind) => updateField(index, { kind })}
              ariaLabel={t("toolbox.fake.fieldKind")}
              className={styles.kind}
              searchable
              searchPlaceholder={t("toolbox.fake.searchKind")}
            />
            {LOCALE_KINDS.has(field.kind) ? (
              <Select
                value={field.locale ?? "vi"}
                options={localeOptions}
                onChange={(locale) => updateField(index, { locale })}
                ariaLabel={t("toolbox.fake.locale")}
                className={styles.locale}
              />
            ) : null}
            {field.kind === "fullName" ? (
              <Checkbox
                className={styles.check}
                label={t("toolbox.fake.includeMiddle")}
                checked={field.includeMiddle ?? false}
                onChange={(event) => updateField(index, { includeMiddle: event.target.checked })}
              />
            ) : null}
            {field.kind === "integer" || field.kind === "float" ? (
              <>
                <Input
                  type="number"
                  value={field.min ?? ""}
                  placeholder={t("toolbox.fake.min")}
                  onChange={(event) => updateField(index, { min: numberOrUndefined(event.target.value) })}
                  aria-label={t("toolbox.fake.min")}
                  className={styles.num}
                />
                <Input
                  type="number"
                  value={field.max ?? ""}
                  placeholder={t("toolbox.fake.max")}
                  onChange={(event) => updateField(index, { max: numberOrUndefined(event.target.value) })}
                  aria-label={t("toolbox.fake.max")}
                  className={styles.num}
                />
              </>
            ) : null}
            {field.kind === "float" ? (
              <Input
                type="number"
                value={field.decimals ?? ""}
                placeholder={t("toolbox.fake.decimals")}
                onChange={(event) =>
                  updateField(index, { decimals: numberOrUndefined(event.target.value) })
                }
                aria-label={t("toolbox.fake.decimals")}
                className={styles.num}
              />
            ) : null}
            {field.kind === "date" ? (
              <>
                <Input
                  value={field.from ?? ""}
                  placeholder={t("toolbox.fake.from")}
                  onChange={(event) => updateField(index, { from: event.target.value })}
                  aria-label={t("toolbox.fake.from")}
                  className={styles.value}
                />
                <Input
                  value={field.to ?? ""}
                  placeholder={t("toolbox.fake.to")}
                  onChange={(event) => updateField(index, { to: event.target.value })}
                  aria-label={t("toolbox.fake.to")}
                  className={styles.value}
                />
              </>
            ) : null}
            {field.kind === "constant" ? (
              <Input
                value={field.value ?? ""}
                placeholder={t("toolbox.fake.constantValue")}
                onChange={(event) => updateField(index, { value: event.target.value })}
                aria-label={t("toolbox.fake.constantValue")}
                className={styles.value}
              />
            ) : null}
            <Button size="small" className={styles.remove} onClick={() => removeField(index)}>
              {t("toolbox.fake.remove")}
            </Button>
          </div>
        ))}
      </div>
      {hasDuplicateName ? <p className={styles.empty}>{t("toolbox.fake.duplicateNames")}</p> : null}

      <div className={styles.controls}>
        <Button onClick={addField}>{t("toolbox.fake.addField")}</Button>
        <Input
          type="number"
          min={1}
          max={MAX_COUNT}
          value={count}
          onChange={(event) =>
            setCount(Math.min(MAX_COUNT, Math.max(1, Math.floor(Number(event.target.value)) || 1)))
          }
          aria-label={t("toolbox.fake.count")}
          className={styles.count}
        />
        <Button variant="primary" onClick={runGenerate} disabled={!canGenerate}>
          {t("toolbox.fake.generate")}
        </Button>
      </div>

      <CopyField label={t("toolbox.output")} value={output} multiline />
    </div>
  );
}

export default FakePanel;
