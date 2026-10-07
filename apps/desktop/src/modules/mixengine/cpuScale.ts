import { useSyncExternalStore } from "react";
import type { CpuScale } from "./metricsState";

/**
 * Which scale CPU is shown on — Settings → MixEngine. A display preference of this window, kept in
 * `localStorage` beside the theme; the tray window hears a change through the `storage` event.
 */
export const CPU_SCALE_KEY = "mixlab-mixengine-cpu-scale";

const listeners = new Set<() => void>();

/** The stored scale, or the whole machine for anything else. `localStorage` can throw in a webview
 *  that forbids it, and the default is then the right answer. */
export function readCpuScale(): CpuScale {
  try {
    return localStorage.getItem(CPU_SCALE_KEY) === "core" ? "core" : "machine";
  } catch {
    return "machine";
  }
}

export function setCpuScale(scale: CpuScale): void {
  try {
    if (scale === "machine") localStorage.removeItem(CPU_SCALE_KEY);
    else localStorage.setItem(CPU_SCALE_KEY, scale);
  } catch {
    // Not remembered; this window still follows below.
  }
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  const onStorage = (e: StorageEvent) => {
    if (e.key === null || e.key === CPU_SCALE_KEY) listener();
  };
  window.addEventListener("storage", onStorage);
  return () => {
    listeners.delete(listener);
    window.removeEventListener("storage", onStorage);
  };
}

export function useCpuScale(): CpuScale {
  return useSyncExternalStore(subscribe, readCpuScale, () => "machine");
}
