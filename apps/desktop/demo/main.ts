/* Order is the point: the mock must own `window.__TAURI_INTERNALS__` before the first module of
   the app evaluates, because `Channel` and every plugin read it at call time from there on. */
import "./ipc/install";
import { installBrowserOverlay } from "./browser/overlay";
import "../src/main.tsx";

installBrowserOverlay(window);
