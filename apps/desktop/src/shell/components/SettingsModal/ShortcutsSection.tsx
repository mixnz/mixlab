import { keyLabel, shortcutLabel } from "../../../core/platform";
import type { Chord, ShortcutDef, ShortcutGroup } from "../../../core/shortcuts";
import { useTranslation } from "../../../i18n";
import styles from "./SettingsModal.module.css";

interface Props {
  /** The catalogue the dispatcher was handed, passed rather than imported — it is a function of
   *  the visible modules, and a constant beside a catalogue that changes is exactly the
   *  disagreement this table exists not to have. Not read back from `currentCatalogue()` either:
   *  the dispatcher writes that in an effect, one render later than this wants it. */
  shortcuts: ShortcutGroup[];
}

/** The chord as this platform spells it — `⌘A` on a Mac, `Ctrl+A` elsewhere. The same function
 *  names the reload button, so the table and the tooltips cannot come to disagree about a key. */
function chordLabel(chord: Chord): string {
  return shortcutLabel(keyLabel(chord.key), {
    shift: chord.shift,
    alt: chord.alt,
    ctrl: chord.ctrl,
  });
}

/**
 * Every Ctrl/Cmd shortcut the app has, read straight out of the catalogue the dispatcher resolves
 * against — so the table cannot say one thing while the app does another. A module's chords appear
 * because the module contributed them, not because this file knows about it.
 *
 * Read-only. The keys are not remappable yet, and a control that does nothing is worse than none.
 */
function ShortcutsSection({ shortcuts }: Props) {
  const { t } = useTranslation();

  /** The row's words. A chord may name its label's blanks after other strings — a module's own name
   *  is the case there is — and here is where there is a `t` to fill them in with. */
  function defLabel(def: ShortcutDef): string {
    if (!def.labelVars) return t(def.labelKey);
    const vars = Object.entries(def.labelVars).map(([name, key]) => [name, t(key)]);
    return t(def.labelKey, Object.fromEntries(vars));
  }

  return (
    <>
      {shortcuts.map((group) => (
        <div key={group.scope} className={styles.section}>
          <span className={styles.sectionLabel}>{t(group.labelKey)}</span>
          {group.defs.map((def) => (
            <div key={def.id} className={styles.shortcutRow}>
              <span>{defLabel(def)}</span>
              <kbd className={styles.shortcutKey}>{chordLabel(def.chord)}</kbd>
            </div>
          ))}
        </div>
      ))}
    </>
  );
}

export default ShortcutsSection;
