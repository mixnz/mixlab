import { Store } from "@tauri-apps/plugin-store";
import { createStore, useStore } from "../../core/jsonStore";
import { stepFontSize } from "./fontSize";
import {
  DEFAULT_SETTINGS,
  LEGACY_FONT_SIZE_KEY,
  sanitizeSettings,
  withLegacyFontSize,
  type TerminalSettings,
} from "./settings";

/**
 * The display settings in use, shared by every terminal tab and remembered between launches.
 *
 * The mechanism lives in `core/jsonStore.ts`. Only this store's own part is left here: the read
 * *writes back right away*, because it has just lifted the font size out of `localStorage` and
 * deleted the old key — without writing, one app exit would lose it.
 */

const FILE = "terminal-settings.json";
const KEY = "settings";

let storePromise: Promise<Store> | null = null;

function getStore(): Promise<Store> {
  if (!storePromise) storePromise = Store.load(FILE);
  return storePromise;
}

/** The font size the previous round left behind, cleared away at the same time. Read inside a
 *  `try` because `localStorage` throws in webviews that forbid per-site storage — and a font size
 *  that cannot be read makes the default the right answer, not an error to report. */
function takeLegacyFontSize(): string | null {
  try {
    const value = localStorage.getItem(LEGACY_FONT_SIZE_KEY);
    localStorage.removeItem(LEGACY_FONT_SIZE_KEY);
    return value;
  } catch {
    return null;
  }
}

const shared = createStore<TerminalSettings>({
  defaults: DEFAULT_SETTINGS,
  load: async () => {
    const store = await getStore();
    const settings = withLegacyFontSize(await store.get(KEY), takeLegacyFontSize());
    /* Write back right after loading rather than waiting for the first edit: the `localStorage` key
       has just been deleted, so the old font size now only exists here. Without writing, one app
       exit would lose it. */
    await store.set(KEY, settings);
    await store.save();
    return settings;
  },
  persist: async (next) => {
    const store = await getStore();
    await store.set(KEY, next);
    await store.save();
  },
});

/** On screen right away, to disk afterwards. Nothing here is worth raising an error in front of
 *  the user: a font size that did not reach the disk is a font size back at the default on the
 *  next launch. */
function write(next: TerminalSettings): void {
  void shared.save(next).catch(() => {});
}

/** The shared settings, kept in sync across every place that calls it. */
export function useTerminalSettings(): TerminalSettings {
  // A failed read leaves `loaded` at `false` and the screen runs on the defaults; there is nowhere
  // here to report an error, and a terminal drawn at the default font size is still a usable
  // terminal.
  return useStore(shared);
}

/** What the store holds right now, for callers that are not components — a right-click handler
 *  reads its switch at the moment of the click, not when the handler was built. */
export function currentTerminalSettings(): TerminalSettings {
  return shared.get();
}

/**
 * The settings, once the file has finished loading.
 *
 * Differs from {@link currentTerminalSettings} exactly where it is worth differing: a screen
 * opening right at app start that asked `snapshot` directly would get the defaults, because the
 * file read has not finished. `TargetForm` needs the *right* default shell rather than needing it
 * *now*, so it waits.
 *
 * A failed read still returns a set of settings — the defaults — rather than throwing: the form
 * still has to open.
 */
export function loadTerminalSettings(): Promise<TerminalSettings> {
  return shared.ready().then(
    () => shared.get(),
    () => shared.get(),
  );
}

/** One settings change. One door rather than a setter per field: the Settings pane changes one
 *  field at a time and no field needs anything another field does not. */
/** One settings change, resolved once it is on disk and rejected when it is not — for sync, which
 *  agrees only on what the disk holds (T178a, L5). Sanitized on the way in, as every write is. */
export function saveTerminalSettings(patch: Partial<TerminalSettings>): Promise<void> {
  return shared.save(sanitizeSettings({ ...shared.get(), ...patch }));
}

export function updateTerminalSettings(patch: Partial<TerminalSettings>): void {
  /* Sanitized on write too, not only when reading the file. A broken field here does not stay where
     it was written wrong: an empty `fontFamily` goes straight into `term.options.fontFamily`, xterm
     builds `ctx.font` from it, that string cannot be parsed, the canvas ignores the assignment and
     keeps the old cell measurements — the text grows while the lines stay put. Exactly one line
     here means no write door can open that path again. */
  void saveTerminalSettings(patch).catch(() => {});
}

/** One step bigger (positive `delta`) or smaller (negative `delta`). Hitting an end of the range
 *  writes nothing and notifies nobody — nothing changed, so there is nothing to redraw. */
export function zoomTerminal(delta: number): void {
  const settings = shared.get();
  const fontSize = stepFontSize(settings.fontSize, delta);
  if (fontSize === settings.fontSize) return;
  write({ ...settings, fontSize });
}
