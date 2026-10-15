/*
 * Helpers for recording a screen in the demo before and after a refactor — see SKILL.md.
 *
 * Copy this file to a git-ignored directory under the repository (the demo's Vite only serves the
 * repository root), write a steps file that calls `window.__harness`, and run both in the page:
 *
 *   await (0, eval)(await (await fetch("/@fs/C:/…/harness.js")).text());
 *   await (0, eval)(await (await fetch("/@fs/C:/…/steps.js")).text());
 *
 * The steps file is an async IIFE — `(async () => { … })();` — that ends with
 * `return __harness.save("baseline")` on master and `return __harness.compare("baseline")` after
 * each commit. The baseline lives in sessionStorage, which survives a reload. Load the harness again
 * after every reload: the page forgets it.
 */
(() => {
  const q = (selector, root = document) => [...root.querySelectorAll(selector)];
  const uuid = /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/g;
  const log = [];

  const h = {
    q,
    wait: (ms = 350) => new Promise((resolve) => setTimeout(resolve, ms)),

    /** Answers commands the demo has no fixture for. Give each the shape its fixture would have:
     *  look in demo/fixtures/*.ts before choosing a value. */
    stub(answers) {
      const internals = window.__TAURI_INTERNALS__;
      if (internals.__harnessOriginal === undefined) internals.__harnessOriginal = internals.invoke.bind(internals);
      const original = internals.__harnessOriginal;
      internals.invoke = (cmd, args, options) => {
        const answer = answers[cmd];
        return answer === undefined ? original(cmd, args, options) : Promise.resolve(answer(args));
      };
    },

    /** A controlled input, changed the way a keystroke changes it. */
    setInput(el, value) {
      const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, "value").set.call(el, value);
      el.dispatchEvent(new Event("input", { bubbles: true }));
    },
    /** The input whose label starts with `label`, inside `root`. */
    field: (label, root = document) =>
      q("input, textarea", root).find((e) => e.closest("label")?.textContent?.trim().startsWith(label)),
    button: (text, root = document) => q("button", root).find((b) => b.textContent.trim() === text),
    dblclick: (el) => el.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })),
    contextmenu: (el) => el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 120, clientY: 160 })),

    /** One snapshot. `read` returns what this screen's recording looks at; ids are masked so two
     *  runs compare equal. */
    rec(label, read) {
      const snap = JSON.parse(JSON.stringify({ label, ...read() }).replace(uuid, "<id>"));
      log.push(snap);
    },

    save(key) {
      sessionStorage.setItem(`harness-${key}`, JSON.stringify(log));
      return `${log.length} snapshots saved, unmocked=${JSON.stringify(window.__demo?.unmocked ?? [])}, errors=${JSON.stringify(window.__demo?.errors ?? [])}`;
    },

    compare(key) {
      const before = JSON.parse(sessionStorage.getItem(`harness-${key}`) ?? "[]");
      const diffs = [];
      if (before.length !== log.length) diffs.push(`length ${before.length} vs ${log.length}`);
      before.forEach((b, i) => {
        const a = log[i] ?? {};
        for (const k of Object.keys(b)) {
          const was = JSON.stringify(b[k]);
          const now = JSON.stringify(a[k]);
          if (was !== now) diffs.push(`${b.label} :: ${k}\n  before ${was.slice(0, 300)}\n  after  ${now?.slice(0, 300)}`);
        }
      });
      return `${log.length} snapshots, errors=${JSON.stringify(window.__demo?.errors ?? [])}\n${diffs.length === 0 ? "IDENTICAL" : diffs.join("\n")}`;
    },
  };

  window.__harness = h;
  return "harness ready";
})();
