---
status: implemented
date: 2026-10-24
task: T209m
---

# No roadmap task numbers in code (design)

T209b stopped new roadmap task numbers from entering source files, and left the 6,040 already there
to go one at a time as the code around them changed. This design removes all of them in one pass,
then turns the check into a plain rule: a source file that cites a task number fails, and there is
no baseline to lower. The rule itself does not change — it is `docs/standards/rust.md`, *Comments*:
a comment says the reason itself, or links the ADR or the spec, and never points at the roadmap.

Roadmap: [phase 47](../roadmap/phase-47-code-that-is-safe-to-change.md), task T209m.

## What was read

Measured on `master` at `139e02e8`, with `node scripts/check-task-citations.mjs`: 6,040 citations
in 1,024 files, 271 distinct task ids.

| Area | Citations | Files |
| --- | ---: | ---: |
| `crates/mixengine-core` | 1,634 | 191 |
| `crates/mixengine-daemon` | 1,565 | 169 |
| `crates/mixengine-cli` | 671 | 117 |
| `crates/mixengine-proto` | 656 | 53 |
| `crates/mixengine-platform` | 647 | 173 |
| `apps/desktop/src` | 457 | 186 |
| `apps/desktop/src-tauri` | 160 | 55 |
| the rest: elevate 60, testkit 59, shim 55, supervisor 22, `server/*` 29, `apps/desktop/scripts`, `demo`, `public`, `vite.config.ts` 10, `scripts/*` 7, docs 4, trampoline 3 | ~250 | ~70 |

By extension: `.rs` 5,545, `.ts` 285, `.tsx` 158, `.css` 35, `.mjs` 16, `.js` 1.

**The shapes they take.** Nearly all sit in comments, and "roadmap task" appears 2,055 times. The
common forms:

- a trailing pointer: `— roadmap task **T85b**.`, `(T190c)`, `— T117.`;
- a pointer with a decision label from that task's design: `(T204a, D5)`, `the T49a design's D6`,
  `(T178c, C4)`;
- history: `until T121 the desktop …`, `since T23`, `a daemon from before T206d`;
- a measurement: `measured in T33a`, `found exactly the way T9's was`;
- the pre-letter numbering, `T2.7`, `T4.4`, `T4.6–T4.8`, counted once per `T` and number.

78 lines carry one outside a comment:

- **test messages and test data** — `.expect("T193 fixture")`, assertion messages,
  `"the shape every client has sent since T23"`, a `why:` field in a proto test and the assertion
  that reads it back (`why.contains("T78a")`);
- **test names** in the window's suites — `it("reads a daemon from before T206d as present")`,
  `it("reads a site from a daemon older than T98 and T135")`, three more. No Rust test function
  carries one: the pattern is case-sensitive and Rust names are lower case;
- **user-facing text** — the daemon's warning about `[updates] enabled` in
  `mixengine-daemon/src/main.rs` ("… no longer do anything since T187 …"), written to its log;
- **SQL inside Rust and TypeScript strings** — the comments in `server/native/src/db.rs`'s schema
  and `server/worker/src/schema.ts`.

**Text that leaves the file it is written in.**

- Doc comments in `crates/mixengine-proto` are copied into `bindings/` by `ts-rs`: `bindings/` holds
  274 citations today, all from there. `bindings/` is not counted by the check, but goes stale.
- Doc comments on `clap` items in `mixengine-cli` (and the daemon's flags) are `mix --help` and
  `mix <command> --help`. `commands/blueprint.rs` has some on fields.
- `mixengine-elevate` and the parts of `mixengine-platform` the helper is built from are pinned by
  `packaging/helper-lock.sh`.
- `.sql` files (the archived migrations) are not counted by the check and are out of scope;
  migrations are never edited once shipped.

**Where the baseline is named.** `scripts/check-task-citations.mjs` and its test,
`scripts/gate.sh`, `.githooks/pre-commit`, `.github/workflows/_lint.yml`, `CLAUDE.md`
(*Common commands*), `docs/standards/rust.md` (*Comments*), and phase 47's T209b entry and M47.
`--shrink` also belongs to `scripts/check-file-sizes.mjs`, which this task does not touch.

## Decisions

**D1. Every citation is rewritten by hand, sentence by sentence.** No `sed`, no regex replacement,
no script that deletes tokens. A script may *list* what is left (the check itself does); every
change is read in its sentence and written so the sentence is grammatical, keeps its meaning, and
fits the file's 100-character line width (rewrapping the paragraph where needed).

**D2. What a citation turns into.** Before rewriting the citations to one task id, its roadmap
entry is read, so the sentence says what the task did and not a guess at it.

- A bare pointer (`— roadmap task **T33c**.`, `(T117)`) is dropped and the reason around it kept.
  Where the sentence then says nothing, the reason is written in its place.
- History (`until T121`, `since T23`, `a daemon from before T206d`) is said by what changed:
  "a daemon from before `devkit` was reported", "the shape every client has sent since the first
  release". Where the date only tells the reader which commit to blame, it goes.
- A measurement says what was measured against: "measured in T33a" → "measured against a real
  server".
- A design's decision label (`D5`, `C4`) is meaningless without its task. It goes with the number,
  and the decision is said in the sentence. Where the reader really needs the longer argument, the
  spec is linked by file name under `docs/specs/` (lower-case `t204a` in a file name is not a
  citation), or the ADR under `docs/decisions/`. Never a file under `docs/plans/`. A link is the
  exception: most comments already say the reason, and a link is added only where the comment
  cannot carry the argument.
- A cross-reference between two code sites that both cite one task (`T182f recorded`) names the
  other site — the function, module or file — instead.

**D3. Behaviour does not change.** Only comments, doc comments, strings and test names. A string a
person reads (the daemon's log warning, any `--help` text) is rewritten with the
`writing-user-facing-text` skill, and every test that asserts on it is changed with it. Each such
string is looked up in `CHANGELOG.md` and `apps/desktop/CHANGELOG.md`; a released entry is never
edited, so a quote there stays and is only reported. A test name with a task number becomes a name
that describes the behaviour (`reads a daemon from before T206d as present` → `reads a daemon that
does not report devkit as present`). Test data with a number (`"running … arrives with roadmap task
T78a"`) gets a sentence without one, and the assertion that reads it back reads the new one.

**D4. SQL text inside strings.** The comments in `server/native/src/db.rs`'s and
`server/worker/src/schema.ts`'s schemas are rewritten like any other comment. Before that, it is
confirmed that neither schema's text is hashed, versioned or compared against a stored copy — a
changed comment must not count as a changed schema. If one is, the comment moves out of the SQL
string into a code comment beside it. A `sqlx::query!` string whose text changes is followed by
`cargo sqlx prepare --workspace -- --all-targets --all-features`.

**D5. One branch, one PR, one commit per area.** Branch `t209m-no-task-numbers-in-code`. Commits,
in this order:

1. `docs(roadmap)`: this spec as `approved`, and the T209m entry.
2. `mixengine-proto`, then `bash packaging/bindings.sh`, with `bindings/` in the same commit.
3. `mixengine-platform`.
4. `mixengine-core`.
5. `mixengine-daemon`.
6. `mixengine-cli`.
7. the window: `apps/desktop/src`, `src-tauri`, `scripts`, `demo`, `public`, `vite.config.ts`.
8. the rest: elevate, testkit, shim, supervisor, trampoline, docs crate, `server/*`, `scripts/*`.
9. the check without a baseline (D6), and the documents that name it.

After each of 2–8, `node scripts/check-task-citations.mjs --shrink` lowers the baseline in the same
commit, and the commands `CLAUDE.md` gives for that area run before the next one starts: `cargo
fmt`, `cargo clippy -D warnings`, rustdoc with `-D warnings`, and `cargo test` for the crates; `npm
run build`, `npm test`, `npm run lint` and the `src-tauri` workspace's `fmt` and `clippy` for the
window; `bash packaging/helper-lock.sh --check` after platform and after the rest (elevate), with
`--bump` if it asks. The commit message's body names any user-facing string changed.

**D6. The check after the baseline is empty.** `scripts/check-task-citations.mjs` stays, at the same
path, run the same way by `scripts/gate.sh`, `.githooks/pre-commit` and CI's `Task citations`
step. It reads the same files (`isChecked`, with `READERS_OF_THE_ROADMAP` still exempt), and
prints one line per occurrence, `file:line: T182f`, then once:
`say the reason itself, or link the ADR or spec (docs/standards/rust.md, *Comments*)`, and exits
non-zero. A clean tree prints `task citations: ok`. `--shrink`, `BASELINE`, `judge`, `shrunk`,
`renderBaseline` and every read or write of the JSON go, and `scripts/task-citation-baseline.json`
is deleted. `check-task-citations.test.mjs` is rewritten for this: `isChecked` as now; the counting
pattern as now (`T32`, `T182f`, `T4` of `T4.4`, no generic or word); a clean text gives no finding;
a text with citations gives one finding per occurrence with its line number; a reader of the
roadmap is exempt.

**D7. The documents that name the baseline.** In the same commit as D6:

- `CLAUDE.md`, *Common commands*: `node scripts/check-task-citations.mjs    # no roadmap task
  numbers in code`.
- `docs/standards/rust.md`, *Comments*: the sentences about citations going "when the code around
  them next changes, not in a bulk rewrite", the baseline and `--shrink` become one: the check
  fails on any citation.
- the comments beside the check in `scripts/gate.sh`, `.githooks/pre-commit` and `_lint.yml`.
- phase 47: T209m ticked, and M47's sentence about the baseline from T209b becomes "no source file
  cites a roadmap task number". T209b's entry is history and stays as written.
- this spec to `implemented`, and `node scripts/check-docs.mjs --write-index`.

Then `git grep` for `task-citation-baseline` finds nothing outside released history, and for
`--shrink` finds only `check-file-sizes.mjs` and the documents about it.

**D8. Verification before the PR.**

- The test count before and after is equal: `cargo test --workspace -- --list` at the root, the
  same in `apps/desktop/src-tauri` and `server/native`, and the test count `npm test` reports in
  `apps/desktop`. A renamed test is one name out and one in.
- `git diff master --stat` touches no file outside comments, strings and test names: the whole diff
  is read once, every rewritten sentence, not a count. `git diff -w master -- '*.rs'` is also
  checked with a script that flags any changed line that is not a comment, a string or a test name,
  as a second look rather than the review.
- `bash scripts/gate.sh` green, then CI requested on the branch (`scripts/ask-ci.sh`), then the PR,
  squash-merged with `(#id)` once CI is green.

## MixLab

No screen changes. The window's code is cleaned like the rest: comments in `apps/desktop/src` and
`src-tauri`, and five test names in its suites. The i18n files' strings carry no task number; only
their comments do. No string a MixLab user reads changes.

## Left to the builder

- The wording of each rewritten sentence, within D2.
- Whether a pre-letter citation (`T2.7`) names the screen or the feature it meant; the roadmap's
  phase files say which.
- `.github/workflows/*.yml` comments are not counted; any found while there are cleaned too.
