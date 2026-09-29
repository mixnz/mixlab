import { useState } from "react";
import Button from "../../../../components/Button";
import Input, { Textarea } from "../../../../components/Input";
import { useTranslation } from "../../../../i18n";
import CopyField from "../../components/CopyField";
import Checkbox from "../../../../components/Checkbox";
import type { RegexMatch, RegexRun } from "./match";
import { runInWorker } from "./run";
import styles from "./Panel.module.css";

const FLAGS = ["g", "i", "m", "s", "u"] as const;

/** Cuts the text into matching and non-matching pieces, to highlight the right places. */
function segments(subject: string, matches: RegexMatch[]): { text: string; hit: boolean }[] {
  const parts: { text: string; hit: boolean }[] = [];
  let at = 0;
  for (const match of matches) {
    if (match.index > at) parts.push({ text: subject.slice(at, match.index), hit: false });
    if (match.text !== "") parts.push({ text: match.text, hit: true });
    at = Math.max(at, match.index + match.text.length);
  }
  if (at < subject.length) parts.push({ text: subject.slice(at), hit: false });
  return parts;
}

function RegexPanel() {
  const { t } = useTranslation();
  const [pattern, setPattern] = useState("");
  const [flags, setFlags] = useState<string[]>(["g"]);
  const [subject, setSubject] = useState("");
  const [replacement, setReplacement] = useState("");
  const [result, setResult] = useState<RegexRun | "timeout" | null>(null);
  const [busy, setBusy] = useState(false);

  const toggle = (flag: string): void => {
    setFlags((current) =>
      current.includes(flag) ? current.filter((one) => one !== flag) : [...current, flag],
    );
  };

  const run = (): void => {
    setBusy(true);
    void runInWorker({ pattern, flags: flags.join(""), subject, replacement })
      .then(setResult)
      .finally(() => setBusy(false));
  };

  return (
    <div className={styles.panel}>
      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t("toolbox.regex.pattern")}</span>
        <Input value={pattern} onChange={(event) => setPattern(event.target.value)} />
      </label>

      <div className={styles.controls}>
        {FLAGS.map((flag) => (
          <Checkbox
            key={flag}
            className={styles.flag}
            label={flag}
            checked={flags.includes(flag)}
            onChange={() => toggle(flag)}
          />
        ))}
        <Button variant="primary" onClick={run} disabled={busy || pattern === ""}>
          {busy ? t("common.loading") : t("toolbox.regex.run")}
        </Button>
      </div>

      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t("toolbox.regex.subject")}</span>
        <Textarea
          value={subject}
          onChange={(event) => setSubject(event.target.value)}
          maxRows={10}
        />
      </label>

      <label className={styles.field}>
        <span className={styles.fieldLabel}>{t("toolbox.regex.replacement")}</span>
        <Input value={replacement} onChange={(event) => setReplacement(event.target.value)} />
      </label>

      {result === "timeout" ? <p className={styles.error}>{t("toolbox.regex.timeout")}</p> : null}
      {result !== null && result !== "timeout" && !result.ok ? (
        <p className={styles.error}>{result.message}</p>
      ) : null}

      {result !== null && result !== "timeout" && result.ok ? (
        <>
          <p className={styles.count}>
            {result.matches.length === 0
              ? t("toolbox.regex.noMatch")
              : t("toolbox.regex.matches", { count: result.matches.length })}
          </p>
          {result.truncated ? <p className={styles.note}>{t("toolbox.regex.truncated")}</p> : null}

          {result.matches.length > 0 ? (
            <div className={styles.highlight}>
              {segments(subject, result.matches).map((part, index) => (
                <span key={index} className={part.hit ? styles.hit : undefined}>
                  {part.text}
                </span>
              ))}
            </div>
          ) : null}

          {result.matches[0] && result.matches[0].groups.length > 0 ? (
            <>
              <span className={styles.fieldLabel}>{t("toolbox.regex.groups")}</span>
              <ul className={styles.groups}>
                {result.matches[0].groups.map((group, index) => (
                  <li key={index}>
                    {group.name ?? `#${group.index}`}: {group.text ?? "—"}
                  </li>
                ))}
              </ul>
            </>
          ) : null}

          {result.replaced !== null ? (
            <CopyField label={t("toolbox.regex.replaced")} value={result.replaced} multiline />
          ) : null}
        </>
      ) : null}
    </div>
  );
}

export default RegexPanel;
