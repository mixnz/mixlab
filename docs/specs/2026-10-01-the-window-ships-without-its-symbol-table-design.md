---
status: implemented
date: 2026-10-01
---

# The window ships without its symbol table

## The problem

`apps/desktop/src-tauri` got a release profile built for size in #242 — thin LTO, one codegen unit,
`opt-level = "s"` — and macOS stopped exporting every external symbol in the commit after it. What
those two shrank is code. What neither touched is the symbol table, which `strip = "debuginfo"`
keeps on purpose, and on Linux and macOS that table now outweighs a third of the binary:

| Target | Window binary | Of which symbol names and entries | Loaded code and data |
| --- | --- | --- | --- |
| Linux x86_64 | 52.6 MB | ≈ 21 MB (`.symtab` + `.strtab`) | 31.7 MB |
| macOS, one slice | ≈ 45.7 MB | ≈ 18.6 MB (`LC_SYMTAB`: 1.7 MB entries, 16.9 MB names) | ≈ 26.5 MB |
| Windows x86_64 | 33.0 MB | 0 — the linker writes symbols to a `.pdb` | 33.0 MB |

Measured on `release-exact` runs 36819090962 and 36829166545. The macOS figure is paid twice,
because the universal binary carries both slices.

Windows never carried them: MSVC's linker writes symbols to `mixlab.pdb` beside the executable,
and nothing in `packaging/` or `.github/workflows/` keeps that file. Every Windows release so far has
shipped without symbols and without a copy of them anywhere.

## What the symbols are read by

Nothing in MixLab. The window installs no panic hook and captures no backtrace; a panic prints a
message without one unless `RUST_BACKTRACE` is set, to a stderr no GUI launch shows. The one
reader is outside the application: macOS's crash reporter names the frames of an `.ips` report from
the binary's own symbol table, and without it a frame is an address and an offset that `atos`
resolves only against a copy of the unstripped binary.

`mixengined` is the opposite case and is not part of this: "copy diagnostics" (T66) exists to produce
a report a human can act on, and the root's `[profile.release]` keeps symbols for it.

## D1. `strip = "symbols"` in the window's profile

`apps/desktop/src-tauri/Cargo.toml`'s `[profile.release]` changes `strip = "debuginfo"` to
`strip = "symbols"`. One line, in the profile that already belongs to the window alone, so no
binary of MixEngine's is touched. rustc runs the platform's strip after the link, and on macOS that
is `/usr/bin/strip` rewriting a binary the linker has already ad-hoc signed. No binary this
repository ships to macOS has been through that path yet — `mixengine-trampoline`, the one with
`strip = "symbols"`, is Windows-only — so whether the signature survives is a thing to check,
not to assume (see Verification).

Expected: Linux x86_64 ≈ 32 MB, macOS universal ≈ 53 MB, Windows unchanged.

## D2. Whether a copy of the symbols is kept

**Decided: D2a.** The two shapes that were weighed:

- **D2a — nothing is kept (chosen).** The same as Windows today. A macOS crash report from a
  user names no functions; a crash anyone can reproduce is reproduced on a development build, which
  keeps everything.
- **D2b — the unstripped binary is kept per target.** `packaging/desktop.sh` builds with
  `strip = "debuginfo"` as now, copies the binary out as the symbol file, then strips it itself:
  `objcopy --only-keep-debug` / `--strip-all` / `--add-gnu-debuglink` on Linux, `strip -x` and an
  ad-hoc re-sign on macOS, and the `.pdb` on Windows. CI uploads them as
  `window-symbols-<os>` beside `window-<os>`. Retention is the hard part: a run's artifacts are
  kept 14 days, and a symbol file is useful exactly as long as its release is installed, so a
  tag run would have to attach them to the GitHub release. That is a second copy of
  `desktop.sh`'s per-OS branching, a re-signing step the bundle does not otherwise have, and a
  release asset nobody downloads but the maintainer.

D2a, because nothing reads the symbols today and Windows has never had them. D2b
can follow as its own spec the day a crash report arrives that the address alone cannot explain.

## Verification

- A `release-exact` run of the branch, `build` group, then measuring each `window-<os>` artifact
  against the table above.
- `MixLab.app` from that run opened on an Apple-silicon Mac: the strip runs after the link and the
  signature has to survive it. CI's macOS probe reads `codesign -dv` on `mix`, not on the window,
  so this one is a person's.
- A full run (`all`) before the branch merges.

## MixLab

No screen. This changes how the window's executable is linked and what file a user downloads; it
adds nothing to the window and takes nothing away from it. Settings → Updates downloads a smaller
payload, and that is the only place a person could notice.
