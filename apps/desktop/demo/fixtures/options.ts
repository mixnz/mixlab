/** What a clip asks of the fixtures. `demo/rig.mjs` sets it before any script of the page runs. */
export interface DemoFixtureOptions {
  sitesWithout?: string[];
  /** No project and no site yet, so the Dashboard offers Quick start. */
  fresh?: boolean;
  /** What the folder picker answers; without it the picker answers nothing, as before. */
  folder?: string;
  /** Draw a browser window when MixLab opens a URL (`demo/browser/overlay.ts`). */
  browser?: boolean;
  /** Two projects, `blog` and `legacy`, no site, PHP 8.4.26 (the default) and 8.1.34 installed and
   *  nothing pinned: the machine the pin-runtime clip pins `legacy` on. */
  runtimePins?: boolean;
}

declare global {
  interface Window {
    __demoFixtures?: DemoFixtureOptions;
  }
}

export function demoOptions(): DemoFixtureOptions {
  return (typeof window === "undefined" ? undefined : window.__demoFixtures) ?? {};
}
