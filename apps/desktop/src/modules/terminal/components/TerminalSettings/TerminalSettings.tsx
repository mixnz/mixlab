import { useEffect, useMemo, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import Button from "../../../../components/Button";
import Input from "../../../../components/Input";
import Select from "../../../../components/Select";
import Checkbox from "../../../../components/Checkbox";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import { localShells } from "../../api";
import { installedFonts } from "../../fontProbe";
import { MAX_FONT_SIZE, MIN_FONT_SIZE, stepFontSize } from "../../fontSize";
import { TERMINAL_FONTS, familyOf, fontStack } from "../../fonts";
import {
  MAX_SCROLLBACK,
  MIN_SCROLLBACK,
  clampScrollback,
  type CursorStyle,
} from "../../settings";
import { updateTerminalSettings, useTerminalSettings } from "../../settingsStore";
import { shellLabel } from "../../shells";
import type { LocalShell } from "../../types";
import styles from "./TerminalSettings.module.css";

/** The shell picker's value when nothing is set. `Select` takes `string | number`, not `null` —
 *  the empty string is how `null` is written at that layer, and it matches no shell name. */
const NO_DEFAULT_SHELL = "";

/**
 * The terminal module's pane in the app's Settings dialog.
 *
 * Every field writes straight to `terminal-settings.json` when it changes, so there is no Save
 * button — just like the REST module's pane. The two number fields write on leaving the field or on
 * `Enter` rather than on every key: `clampScrollback` would pull a half-typed number to the limit
 * midway, and a user typing "12000" would see the field jump to 100 after the first digit.
 */
function TerminalSettings() {
  const { t } = useTranslation();
  const settings = useTerminalSettings();
  const [shells, setShells] = useState<LocalShell[]>([]);
  const [fonts, setFonts] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  /** The two number fields while being typed, so a half-finished number is not clamped midway. */
  const [fontSizeText, setFontSizeText] = useState<string | null>(null);
  const [scrollbackText, setScrollbackText] = useState<string | null>(null);

  useEffect(() => {
    localShells()
      .then(setShells)
      // If no shell was detected, the picker only has the "whatever this machine offers first"
      // entry, and that is still a usable answer. The error shows under the field rather than
      // being swallowed.
      .catch((e) => setError(errorMessage(t, e)));
    // Once: a machine's shell list does not change midway.
  }, []);

  /* Also once, and for the same reason — fonts installed on the machine do not sprout while the
     dialog is open. Measured with a canvas, so it can only run once there is a DOM, i.e. in an
     effect. */
  useEffect(() => {
    let live = true;
    installedFonts(TERMINAL_FONTS).then((found) => {
      if (live) setFonts(found);
    });
    return () => {
      live = false;
    };
  }, []);

  const family = familyOf(settings.fontFamily);

  /* The font in use is always present, even when the measurement does not recognise it: a picker
     pointing at nothing is a picker lying about what is running. */
  const fontOptions = useMemo(() => {
    const names = fonts.includes(family) ? fonts : [family, ...fonts];
    return names.map((name) => ({
      value: name,
      label: name,
      // Each entry is drawn in itself: how else would you pick a font without seeing it.
      optionLabel: <span style={{ fontFamily: fontStack(name) }}>{name}</span>,
    }));
  }, [fonts, family]);

  function commitFontSize(text: string) {
    updateTerminalSettings({ fontSize: stepFontSize(Number(text), 0) });
    setFontSizeText(null);
  }

  function commitScrollback(text: string) {
    updateTerminalSettings({ scrollback: clampScrollback(Number(text)) });
    setScrollbackText(null);
  }

  async function browseDirectory() {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === "string") updateTerminalSettings({ defaultCwd: picked });
  }

  return (
    <>
      <div className={styles.group}>
        <span className={styles.groupLabel}>{t("terminal.settingsScreenGroup")}</span>

        <div className={styles.row}>
          <span className={styles.label}>{t("terminal.settingsFontFamily")}</span>
          {/* A picker rather than a text field, and not only because typing font names is tiring:
              a text field writes on every key, so it passes through half-finished values — even
              the empty string. xterm builds `ctx.font` from that value, a broken string makes the
              canvas quietly keep the old cell measurements, and the screen cuts across every line.
              Here every value goes through `fontStack`. */}
          <Select<string>
            size="small"
            className={styles.wide}
            searchable
            searchPlaceholder={t("terminal.settingsFontSearch")}
            value={family}
            ariaLabel={t("terminal.settingsFontFamily")}
            options={fontOptions}
            onChange={(name) => updateTerminalSettings({ fontFamily: fontStack(name) })}
          />
        </div>
        <p className={styles.hint}>{t("terminal.settingsFontFamilyHint")}</p>

        <div className={styles.row}>
          <span className={styles.label}>{t("terminal.settingsFontSize")}</span>
          <Input
            size="small"
            type="number"
            className={styles.number}
            min={MIN_FONT_SIZE}
            max={MAX_FONT_SIZE}
            value={fontSizeText ?? String(settings.fontSize)}
            aria-label={t("terminal.settingsFontSize")}
            onChange={(e) => setFontSizeText(e.target.value)}
            onBlur={(e) => commitFontSize(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitFontSize(e.currentTarget.value);
            }}
          />
        </div>

        <div className={styles.row}>
          <span className={styles.label}>{t("terminal.settingsScrollback")}</span>
          <Input
            size="small"
            type="number"
            className={styles.number}
            min={MIN_SCROLLBACK}
            max={MAX_SCROLLBACK}
            value={scrollbackText ?? String(settings.scrollback)}
            aria-label={t("terminal.settingsScrollback")}
            onChange={(e) => setScrollbackText(e.target.value)}
            onBlur={(e) => commitScrollback(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitScrollback(e.currentTarget.value);
            }}
          />
          <span className={styles.unit}>{t("terminal.settingsScrollbackUnit")}</span>
        </div>

        <div className={styles.row}>
          <span className={styles.label}>{t("terminal.settingsCursorStyle")}</span>
          <Select<CursorStyle>
            size="small"
            value={settings.cursorStyle}
            ariaLabel={t("terminal.settingsCursorStyle")}
            options={[
              { value: "block", label: t("terminal.settingsCursorBlock") },
              { value: "underline", label: t("terminal.settingsCursorUnderline") },
              { value: "bar", label: t("terminal.settingsCursorBar") },
            ]}
            onChange={(cursorStyle) => updateTerminalSettings({ cursorStyle })}
          />
        </div>

        <Checkbox
          className={styles.check}
          label={t("terminal.settingsCursorBlink")}
          checked={settings.cursorBlink}
          onChange={(e) => updateTerminalSettings({ cursorBlink: e.target.checked })}
        />
      </div>

      <div className={styles.group}>
        <span className={styles.groupLabel}>{t("terminal.settingsSessionGroup")}</span>

        <div className={styles.row}>
          <span className={styles.label}>{t("terminal.settingsDefaultShell")}</span>
          <Select<string>
            size="small"
            value={settings.defaultShell ?? NO_DEFAULT_SHELL}
            ariaLabel={t("terminal.settingsDefaultShell")}
            options={[
              { value: NO_DEFAULT_SHELL, label: t("terminal.settingsDefaultShellAuto") },
              ...shells.map((shell) => ({ value: shell.name, label: shellLabel(shell.name) })),
            ]}
            onChange={(name) =>
              updateTerminalSettings({ defaultShell: name === NO_DEFAULT_SHELL ? null : name })
            }
          />
        </div>

        <div className={styles.row}>
          <span className={styles.label}>{t("terminal.settingsDefaultCwd")}</span>
          <Input
            size="small"
            className={styles.wide}
            placeholder={t("terminal.startInPlaceholder")}
            value={settings.defaultCwd ?? ""}
            aria-label={t("terminal.settingsDefaultCwd")}
            onChange={(e) => updateTerminalSettings({ defaultCwd: e.target.value || null })}
          />
          <Button size="small" onClick={browseDirectory}>
            {t("terminal.browse")}
          </Button>
        </div>

        <Checkbox
          className={styles.check}
          label={t("terminal.settingsRightClickPastes")}
          checked={settings.rightClickPastes}
          onChange={(e) => updateTerminalSettings({ rightClickPastes: e.target.checked })}
        />
        <p className={styles.hint}>{t("terminal.settingsRightClickPastesHint")}</p>

        <Checkbox
          className={styles.check}
          label={t("terminal.settingsTitleShowsTargetName")}
          checked={settings.titleShowsTargetName}
          onChange={(e) => updateTerminalSettings({ titleShowsTargetName: e.target.checked })}
        />
        <p className={styles.hint}>{t("terminal.settingsTitleShowsTargetNameHint")}</p>

        {error && <p className={styles.hint}>{error}</p>}
        <p className={styles.hint}>{t("terminal.settingsGlobalHint")}</p>
      </div>
    </>
  );
}

export default TerminalSettings;
