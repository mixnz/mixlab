import { useMemo, useState } from "react";
import Button from "../../../../components/Button";
import Input from "../../../../components/Input";
import Select, { type SelectOption } from "../../../../components/Select";
import { useTranslation } from "../../../../i18n";
import CopyField from "../../components/CopyField";
import { setTimeZone, useToolsWorkspace } from "../../workspace";
import { detectUnit, toInstant, toOutputs } from "./time";
import { allZones, canonicalZone, preferredZone, zoneOffset } from "./zones";
import styles from "./Panel.module.css";

/* The machine's zone, corrected according to the user's country — see `preferredZone`. Windows
   reports `Asia/Bangkok` for a machine set to Vietnamese, and +07:00 is identical so nobody
   notices.

   Three sources rather than one: `resolvedOptions().locale` in the webview follows the webview's
   **display language** — usually `en-US` — rather than the Windows region as in Node.
   `navigator.languages` is where the user's real language shows. */
const resolved = Intl.DateTimeFormat().resolvedOptions();
const LOCALE_SOURCES = [
  resolved.locale,
  ...(typeof navigator === "undefined" ? [] : (navigator.languages ?? [])),
  typeof navigator === "undefined" ? "" : navigator.language,
];
const LOCAL_ZONE = preferredZone(resolved.timeZone, LOCALE_SOURCES, Date.now());

/* Computed once at module level: more than four hundred entries, and the list does not change
   during a run. */
const ZONE_NAMES = allZones();

function TimestampPanel() {
  const { t } = useTranslation();
  const workspace = useToolsWorkspace();
  const [input, setInput] = useState(() => String(Date.now()));
  // Freeze "now" instead of letting it tick every second: a "3 days ago" line changing by itself
  // while people are reading it is more annoying than useful. The "Now" button refreshes both.
  const [now, setNow] = useState(() => Date.now());

  /* The saved name is normalised again on the way out: a file written by the previous version may
     still hold `Asia/Saigon`. A zone the runtime no longer knows falls back to the machine's zone,
     rather than leaving the picker blank. */
  const stored = workspace.timeZone === null ? null : canonicalZone(workspace.timeZone);
  const zone = stored !== null && ZONE_NAMES.includes(stored) ? stored : LOCAL_ZONE;

  /* Each zone's offset depends on the moment — half the world changes its clocks by season — so the
     list is rebuilt when `now` changes, rather than once at module level like the zone names. */
  const zoneOptions: SelectOption<string>[] = useMemo(
    () =>
      ZONE_NAMES.map((name) => {
        const offset = zoneOffset(name, now);
        const label = offset ? `${name} (UTC${offset})` : name;
        // Typing "saigon", "+07" or "ho chi minh" must all find the same zone.
        return { value: name, label, searchText: `${label} ${name.replace(/[_/]/g, " ")}` };
      }),
    [now],
  );

  const unit = detectUnit(input);
  const instant = toInstant(input);
  const outputs = useMemo(
    () => (instant === null ? null : toOutputs(instant, zone, now)),
    [instant, zone, now],
  );

  const guess =
    unit === "seconds"
      ? t("toolbox.timestamp.guessedSeconds")
      : unit === "millis"
        ? t("toolbox.timestamp.guessedMillis")
        : unit === "micros"
          ? t("toolbox.timestamp.guessedMicros")
          : instant !== null
            ? t("toolbox.timestamp.guessedIso")
            : null;

  return (
    <div className={styles.panel}>
      <div className={styles.controls}>
        <Input
          value={input}
          onChange={(event) => setInput(event.target.value)}
          aria-label={t("toolbox.timestamp.value")}
          placeholder={t("toolbox.timestamp.placeholder")}
          className={styles.input}
        />
        <Button
          onClick={() => {
            const stamp = Date.now();
            setNow(stamp);
            setInput(String(stamp));
          }}
        >
          {t("toolbox.timestamp.now")}
        </Button>
        <Select
          value={zone}
          options={zoneOptions}
          onChange={setTimeZone}
          searchable
          searchPlaceholder={t("toolbox.timestamp.searchZone")}
          ariaLabel={t("toolbox.timestamp.timeZone")}
          className={styles.zone}
        />
      </div>

      {guess ? <p className={styles.guess}>{guess}</p> : null}

      {outputs ? (
        <div className={styles.results}>
          <CopyField label={t("toolbox.timestamp.isoUtc")} value={outputs.isoUtc} />
          <CopyField label={t("toolbox.timestamp.isoLocal")} value={outputs.isoLocal} />
          <CopyField label={t("toolbox.timestamp.unixSeconds")} value={outputs.unixSeconds} />
          <CopyField label={t("toolbox.timestamp.unixMillis")} value={outputs.unixMillis} />
          <CopyField label={t("toolbox.timestamp.relative")} value={outputs.relative} mono={false} />
        </div>
      ) : (
        <p className={styles.unreadable}>{t("toolbox.timestamp.unreadable")}</p>
      )}
    </div>
  );
}

export default TimestampPanel;
