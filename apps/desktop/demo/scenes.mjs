import { readFileSync } from "node:fs";

/**
 * The six scenes, and the copy the frame writes under each.
 *
 * Plain JavaScript because the capture script reads it in node. `state` is a module's own session
 * slot — `scenes.test.mjs` runs each one through that module's parser, so a change to how a tab
 * restores fails there first. The MixEngine tab restores nothing — it always opens on Dashboard —
 * so its scenes carry no `state` and reach another screen through `act`, by the sidebar's
 * `data-screen`, which is the screen's id and not its label.
 *
 * `act` runs once the scene is quiet, and the scene waits to be quiet again after it. It may use a
 * shortcut the module registers, text the fixtures own, or an element that is the only one of its
 * kind — never interface copy, never a CSS class.
 */
export const CONSTANTS = JSON.parse(readFileSync(new URL("./constants.json", import.meta.url), "utf8"));

export const SCENES = [
  {
    id: "hero",
    moduleId: "mixengine",
    tabTitle: "MixEngine",
    headline: "Your whole local stack, one window",
    description: "PHP, Node and Python side by side, databases and HTTPS domains — no Docker, no config files.",
  },
  {
    id: "sites",
    moduleId: "mixengine",
    tabTitle: "MixEngine",
    headline: "Sites and runtimes",
    description: "Every project gets a .test domain with trusted HTTPS, on the PHP or Node version it asks for.",
    act: async ({ page }) => {
      await page.locator('nav [data-screen="sites"]').click();
    },
  },
  {
    id: "database",
    moduleId: "db",
    tabTitle: "acme_shop",
    // Not connected: the saved connection is loaded into the form, and the form is the image.
    state: { savedId: CONSTANTS.connectionId, connected: false },
    headline: "Database",
    description: "PostgreSQL, MySQL, SQLite, MongoDB, Redis and more in one client, over an SSH tunnel when you need one.",
  },
  {
    id: "rest",
    moduleId: "rest",
    tabTitle: "REST",
    state: { openIds: [CONSTANTS.requestId], activeId: CONSTANTS.requestId },
    headline: "REST",
    description: "Send requests, keep environments and history, and read the response the way it was meant to be read.",
    // `rest.send` in `modules/rest/shortcuts.ts`.
    act: async ({ page, modifier }) => {
      await page.keyboard.press(`${modifier}+Enter`);
    },
  },
  {
    id: "terminal",
    moduleId: "terminal",
    tabTitle: "Terminal",
    state: { kind: "local", shellName: CONSTANTS.shellName, cwd: CONSTANTS.cwd },
    headline: "Terminal",
    description: "A shell on this machine or on a server over SSH, in a tab beside everything else.",
  },
  {
    id: "tools",
    moduleId: "tools",
    tabTitle: "Tools",
    state: { toolId: "diff" },
    headline: "Tools",
    description: "Diffs, JSON, regex, JWTs, timestamps and more. Offline, next to your code.",
    // The diff fills the width and its colours read at a glance, where the JWT decoder left half the
    // image empty. Its two textareas are the only ones on screen, left then right. The view modes are
    // the last tablist on the page (the tab strip is the first), in `VIEW_MODES` order: unified,
    // then split.
    act: async ({ page, constants }) => {
      const fields = page.locator("textarea");
      await fields.nth(0).fill(constants.diffLeft);
      await fields.nth(1).fill(constants.diffRight);
      await page.locator('[role="tablist"]').last().locator('[role="tab"]').nth(1).click();
    },
  },
];
