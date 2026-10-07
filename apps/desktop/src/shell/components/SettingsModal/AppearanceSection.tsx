import SegmentedControl from "../../../components/SegmentedControl";
import type { ColorTheme, ThemeMode } from "../../theme";
import { COLOR_THEMES } from "../../theme";
import type { Language, TranslationKey } from "../../../i18n";
import { useTranslation } from "../../../i18n";
import styles from "./SettingsModal.module.css";

interface Props {
  theme: ThemeMode;
  onThemeChange: (theme: ThemeMode) => void;
  colorTheme: ColorTheme;
  onColorThemeChange: (colorTheme: ColorTheme) => void;
}

/** `dim` -> `settings.colorThemeDim`, the name under each swatch. */
function colorThemeLabelKey(colorTheme: ColorTheme): TranslationKey {
  return `settings.colorTheme${colorTheme.charAt(0).toUpperCase()}${colorTheme.slice(1)}` as TranslationKey;
}

/**
 * Everything about how the app looks and reads: light, dark or a colour theme, and which language.
 *
 * They sit together because they are the settings a user changes on a whim and sees the result of
 * immediately — unlike the tools and the updater, which are errands.
 */
function AppearanceSection({ theme, onThemeChange, colorTheme, onColorThemeChange }: Props) {
  const { t, lang, setLang } = useTranslation();
  const colorMode = theme === "color";

  return (
    <>
      <div className={styles.section}>
        <span className={styles.sectionLabel} id="settings-theme-label">
          {t("settings.theme")}
        </span>
        {/* System first: it is the default, and the choice that asks least of the user. */}
        <SegmentedControl<ThemeMode>
          aria-label={t("settings.theme")}
          block
          value={theme}
          onChange={onThemeChange}
          segments={[
            { value: "system", label: t("settings.themeSystem") },
            { value: "light", label: t("settings.themeLight") },
            { value: "dark", label: t("settings.themeDark") },
            { value: "color", label: t("settings.themeColor") },
          ]}
        />
      </div>

      <div className={styles.section}>
        <span className={styles.sectionLabel} id="settings-color-theme-label">
          {t("settings.colorTheme")}
        </span>
        {/* Shown under every mode, so it is there to be found, but only Colour lets it be used:
            Light, Dark and System each have their own ground and accent. */}
        {!colorMode && <p className={styles.hint}>{t("settings.colorThemeHint")}</p>}
        <div
          className={styles.colorThemes}
          role="group"
          aria-labelledby="settings-color-theme-label"
          aria-disabled={!colorMode}
        >
          {COLOR_THEMES.map((opt) => {
            const label = t(colorThemeLabelKey(opt));
            const active = colorMode && opt === colorTheme;
            return (
              <button
                key={opt}
                type="button"
                className={active ? `${styles.colorTheme} ${styles.colorThemeActive}` : styles.colorTheme}
                onClick={() => onColorThemeChange(opt)}
                disabled={!colorMode}
                aria-pressed={active}
                title={label}
              >
                {/* The preview takes the theme's own tokens (App.css), so it shows that theme's
                    ground, surface and accent whatever is in force around it. */}
                <span className={styles.colorThemePreview} data-palette-swatch={opt} aria-hidden="true">
                  <span className={styles.colorThemeSurface}>
                    <span className={styles.colorThemeLine} />
                    <span className={styles.colorThemeAccent} />
                  </span>
                </span>
                <span className={styles.colorThemeName}>{label}</span>
              </button>
            );
          })}
        </div>
      </div>

      <div className={styles.section}>
        <span className={styles.sectionLabel}>{t("settings.language")}</span>
        <SegmentedControl<Language>
          aria-label={t("settings.language")}
          block
          value={lang}
          onChange={setLang}
          segments={[
            { value: "en", label: t("settings.languageEnglish") },
            { value: "vi", label: t("settings.languageVietnamese") },
          ]}
        />
      </div>
    </>
  );
}

export default AppearanceSection;
