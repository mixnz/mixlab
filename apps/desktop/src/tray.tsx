import React, { Suspense } from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import ErrorBoundary from "./components/ErrorBoundary";
import { I18nProvider } from "./i18n";
import { blockNativeContextMenu } from "./core/nativeContextMenu";
import { logError } from "./core/log";
import { IS_MAC, IS_WINDOWS } from "./core/platform";
import { readEnabledModules, visibleModules } from "./shell/profiles";
import TrayFrame from "./shell/tray/TrayFrame";
import { traySections } from "./shell/tray/sections";
/* The tokens, the ground and the theme the main window draws with. `shell/theme` applies the stored
   theme and accent as it is imported. */
import "./shell/theme";
import "./shell/App.css";

/**
 * The tray panel's window — T168, T192, `src-tauri/src/tray.rs`.
 *
 * The shell's frame around the sections the visible modules lend (ADR 0058), and nothing when none
 * does — the backend never shows the window then, because the main window said there is no panel.
 */

blockNativeContextMenu();

/* On macOS and Windows the window is transparent and the page draws a card inside it — but
   `App.css` paints `:root` with the page colour, which would fill the whole window and sit there,
   already in place, while the card slid in over it. On Linux the window is an ordinary one and
   keeps its ground. */
if (IS_MAC || IS_WINDOWS) {
  document.documentElement.style.backgroundColor = "transparent";
}

window.addEventListener("error", (e) => void logError("tray", e.error ?? e.message));
window.addEventListener("unhandledrejection", (e) => void logError("tray", e.reason));

/* The main window writes the theme, the accent, the language and the module set into the storage
   both windows share, and the `storage` event is how this one hears. Reloading is the whole answer:
   the panel reads everything again whenever it is shown anyway, and it is hidden when this runs. */
const FOLLOWED = new Set(["mixdb-theme", "mixdb-accent", "mixdb-lang", "mixdb-modules"]);
window.addEventListener("storage", (e) => {
  if (e.key === null || FOLLOWED.has(e.key)) window.location.reload();
});

const enabled = readEnabledModules();
const sections = enabled === null ? [] : traySections(visibleModules(enabled));

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <I18nProvider>
      <ErrorBoundary variant="app">
        <Suspense fallback={null}>{sections.length > 0 && <TrayFrame sections={sections} />}</Suspense>
      </ErrorBoundary>
    </I18nProvider>
  </React.StrictMode>,
);
