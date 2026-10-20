import { useEffect, useEffectEvent } from "react";
import { isBlockedReload } from "../../core/reload";
import { configureTray } from "../../core/window";
import { logError } from "../../core/log";
import { useLanguageT, useTranslation } from "../../i18n";
import { onUninstallRequest, uninstallMenuLabel } from "../uninstall";

interface WindowChromeOptions {
  /** Whether a visible module lends the tray panel a section. */
  hasTraySection: boolean;
  /** MixLab ▸ Remove MixLab from this Mac… was picked. */
  onRemoveRequested: () => void;
}

/**
 * What the window tells the OS and asks of it: the tray's menu, the uninstall menu item, and the
 * reload keys it refuses.
 *
 * Called last in `Workspace`, after the shortcut dispatcher, so the refused-reload listener is still
 * added to the window after the dispatcher's: the dispatcher skips an event already prevented.
 */
export function useWindowChrome({ hasTraySection, onRemoveRequested }: WindowChromeOptions): void {
  const { t } = useTranslation();
  /* The tray icon is MixLab's and goes up whatever modules are visible (ADR 0058); what this window
     tells it is whether a visible module lends the panel a section, and the words for its menu —
     again on a language switch, which is what `useLanguageT` gives a new function for. */
  const tl = useLanguageT();

  useEffect(() => {
    void configureTray(hasTraySection, {
      openPanel: tl("tray.openPanel"),
      openMain: tl("tray.openMain"),
      quit: tl("tray.quit"),
    }).catch((e: unknown) => void logError("tray", e));
  }, [hasTraySection, tl]);

  /* MixLab ▸ Remove MixLab from this Mac…: T182a. The menu item is Rust's, built before any string
     was loaded, so its text is sent from here; where there is no item the command does nothing. */
  const removeRequested = useEffectEvent(() => onRemoveRequested());
  useEffect(() => {
    const unlisten = onUninstallRequest(() => removeRequested());
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);
  useEffect(() => {
    void uninstallMenuLabel(t("remove.menuItem")).catch(() => {});
  }, [t]);

  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      // Reloading the webview takes every open connection down with it, so no keystroke is left
      // able to ask for one. The plain `Ctrl+R` is not refused here: it is `pane.reload` on the
      // registry, which presses the reload button of the pane on screen or swallows the key —
      // see `useReloadShortcut`.
      //
      // The rest — `F5`, the hard reload — are not commands: nothing is being asked for here,
      // only refused. See `isBlockedReload` for what differs between builds.
      if (isBlockedReload(e)) e.preventDefault();
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);
}
