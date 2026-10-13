import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";

/**
 * What only a linter can say.
 *
 * Deliberately not the recommended sets. `tsc` already runs `strict`, `noUnusedLocals` and
 * `noUnusedParameters`, and it runs on every push; a second opinion about the same things would be
 * noise. What is here is what the compiler cannot see:
 *
 * - **Hook dependencies.** A `useCallback` that closes over `t` and does not list it keeps the
 *   dictionary it was built with, so an error raised after the user switches language comes out in
 *   the old one. That compiles perfectly.
 * - **The module boundary.** `src/components`, `src/core`, `src/icons`, `src/shell` and `src/i18n`
 *   know nothing about `modules/db`, `modules/rest` or `modules/terminal`. A `Button` importing
 *   from a module type-checks; this is what says no, and it says it in the editor rather than in
 *   CI ten minutes later.
 *
 * `docs/standards/desktop/adding-a-module.md` is where the boundary rule is written down. The two
 * exceptions below are the point of it: `shell/registry.ts` joins a module to the tab bar and
 * `i18n/dicts.ts` joins its strings to the dictionary, one line each per module.
 */

/** Everything the boundary protects. `src/modules/` and `src/main.tsx` are deliberately absent. */
const GUARDED = ["src/components/**", "src/core/**", "src/icons/**", "src/shell/**", "src/i18n/**"];

/** The two places a module is joined to the app. */
const JOINS = ["src/shell/registry.ts", "src/i18n/dicts.ts"];

export default tseslint.config(
  { ignores: ["dist/**", "src-tauri/**", "node_modules/**", "coverage/**"] },

  {
    files: ["**/*.{ts,tsx}"],
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    plugins: { "react-hooks": reactHooks },
    rules: {
      // Off, all of them: `tsc` has already had this argument, with better information.
      ...Object.fromEntries(
        Object.keys({ ...js.configs.recommended.rules }).map((rule) => [rule, "off"]),
      ),
      "@typescript-eslint/no-unused-vars": "off",
      "@typescript-eslint/no-explicit-any": "off",
      "@typescript-eslint/no-empty-object-type": "off",
      "@typescript-eslint/no-unused-expressions": "off",
      "no-empty": "off",

      "react-hooks/rules-of-hooks": "error",

      /* An error now that the backlog is gone. It was a warning under a ceiling that could only
         fall while there were fifty of them; they were worked off by what each one was: a value
         that never changes identity (`t`, the workspace's caches and error setter) listed, a
         parent's callback or a value an effect must not re-run for reached through
         `useEffectEvent`, a helper called during render made a `useCallback`. A disable comment
         is for a dependency left out or added on purpose, and says why after its `--`. */
      "react-hooks/exhaustive-deps": "error",
    },
  },

  {
    files: GUARDED,
    ignores: JOINS,
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["**/modules/*", "**/modules/*/**"],
              message:
                "The shared layer knows no module. See docs/standards/desktop/adding-a-module.md — only shell/registry.ts and i18n/dicts.ts may name one.",
            },
          ],
        },
      ],
    },
  },
);
