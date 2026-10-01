---
status: implemented
date: 2026-10-01
task: T91a
---

# A crash report names its frames after the fact

## The problem

A `CrashReport` (T91, [ADR 0022](../decisions/0022-a-crash-report-is-recorded-by-default-and-sent-by-nothing.md),
[the T91 design](2026-09-05-t91-crash-reporting-design.md)) carries the panicking thread's backtrace
as symbol names, resolved **inside the panic hook** from the running binary's own symbols
(`std::backtrace::Backtrace::force_capture().to_string()`, filtered by `frames` in
`crates/mixengine-daemon/src/crash.rs`). That has two costs, one of which nobody knew about.

**On Windows, every release report is already nameless.** MSVC's linker writes the symbols to
`mixengined.pdb`, and nothing in `packaging/` or `.github/workflows/` ships or keeps that file:
`binaries-hand-on.sh` tars the executables alone. A program built with the root's release profile
(thin LTO, one codegen unit, `strip = "debuginfo"`) that asks for its own backtrace prints this on
Windows with its `.pdb` beside it:

```text
   0: std::backtrace_rs::backtrace::win64::trace
   1: std::backtrace_rs::backtrace::trace_unsynchronized
   2: std::backtrace::Backtrace::create
   3: std::backtrace::Backtrace::force_capture
```

and this with the `.pdb` moved away, which is how every installed `mixengined.exe` runs:

```text
   0: <unknown>
   1: <unknown>
   …
```

Measured on Windows x86_64 on 2026-10-01. The release profile's comment, *"a daemon crash report
is worthless without function names"*, has described every Windows report written so far.

**On Linux and macOS the names are paid for in every download.** `mixengined` is the one binary
the root workspace still ships with its symbol table, and only for this hook. The table is 5.5 MB
of 32.5 MB on Linux x86_64 (17%) and 4.6 MB of 26.1 MB on a macOS arm64 slice (18%), measured in
[the binaries spec](2026-10-01-mixengine-binaries-built-for-size-design.md). That spec also found
the table to be what keeps `opt-level = "s"` from shrinking the daemon at all on those systems.

The fix for both is the one native crash reporting has always used. The report records **where**
each frame is, as an offset into the executable. The release keeps the symbols **beside** the
download instead of inside it. A maintainer turns offsets back into names when a report arrives.

## D1. A frame is an offset into the executable

`CRASH_FORMAT` goes to **2**. `frames` changes from a list of names to a list of:

```rust
pub struct CrashFrame {
    /// The frame's return address minus the executable's base (below), as `0x…` hex. The same on
    /// every machine that ran this build, whatever ASLR chose. Absent for a frame outside the
    /// executable: `libc`, `ntdll`, a system framework.
    pub offset: Option<String>,
}
```

and `CrashReport` gains two fields:

```rust
/// Symbol names as this build resolved them in the hook (D3): what `frames` held in format 1, and
/// empty in a release build. A list of its own rather than a name on each frame, because the names
/// come from `std`'s own capture, whose frames are not ours one for one.
pub symbols: Vec<String>,

/// The executable's build identifier: the GNU build-id note on Linux, `LC_UUID` on macOS, the
/// CodeView GUID and age on Windows. A symbol file is matched against it, so a report is never
/// symbolised against the wrong build.
pub build_id: Option<String>,
```

**The base** is what each format's symbolisers count from. On Linux it is the load bias
(`dlpi_addr` of the main object), so an offset is a link-time virtual address. On macOS it is the
dyld slide, so an offset is a link-time address including `__TEXT`'s `0x100000000`. On Windows it
is the module's base, so an offset is an RVA. The script in D5 knows which is which. It subtracts
one from every offset before it asks for a name, because a return address points at the
instruction **after** the call.

**Why the new fields are clean, by ADR 0022's own test.** `symbols` is the format-1 field under a
new name. That ADR makes recording safe
through the field list: every field is a compile-time constant of the build, a literal from `std`
or `tokio`, or a symbol name. An offset into the executable and a build identifier are both
constants of the build. They are the same number on every machine that runs it and carry nothing
from the home. A frame outside the executable keeps no offset, and nothing names the library it
was in, because a module's name is a path on the machine (`/usr/lib/…`, `C:\Windows\…`). It records
only that a frame was there, so the reader can still count the depth.

Because this widens the list ADR 0022 calls the guarantee, the change ships with **ADR 0060**
rather than an edit to an accepted one. Its subject is that a crash report carries offsets and the
release keeps the symbols. It adds "an offset into the executable" and "its build identifier" as
the fourth and fifth kinds of safe field, and says the rest of ADR 0022 stands.

## D2. What the hook reads, and from where

Two things, both in **`mixengine-platform`**, behind one module (`crash_image`, or whatever the
plan names it), because both are OS calls and this repository allows those nowhere else.

**The return addresses** of the panicking thread come from the unwinder the program already links:

- on Linux and macOS, `_Unwind_Backtrace` and `_Unwind_GetIP` (`libgcc_s` and `libunwind`, which
  `std`'s own unwinding uses);
- on Windows, `RtlCaptureStackBackTrace`, through `windows-sys`, which is already in the tree.

**Not the `backtrace` crate**, though it is what `std` vendors. On the Linux and macOS side it
brings `addr2line`, `gimli`, `object` and `rustc-demangle` into the tree for a resolver that a
release build never calls (D3). Its current `Cargo.toml` asks for `miniz_oxide` 0.9 where the tree
has 0.8.9, which `multiple-versions = "deny"` in `deny.toml` refuses without a `skip` entry
([docs/standards/rust.md](../standards/rust.md)). Two OS functions per family are less than either
of those.

**The executable's base, its address range and its build identifier** are read **once**, when
`Reports::install` sets the hook, and kept in `Reports`. Each OS supplies them differently:

- Linux: `dl_iterate_phdr` for the main object, with its program headers and `PT_NOTE`;
- macOS: `_dyld_get_image_header(0)` and `_dyld_get_image_vmaddr_slide(0)`, with the load
  commands and `LC_UUID`;
- Windows: `GetModuleHandleW(NULL)`, with the PE headers and the debug directory.

The hook itself then makes no OS call beyond the unwind.

A platform that cannot answer, through a failure to read the headers or a target nobody wrote yet,
gives `None`. The report then carries frames with no offsets, `symbols` where the build resolved
them, and no `build_id`. That is ADR 0022's rule already: an absent fact is absent, not a failure.

**The build identifier has to be there to be read.** On Linux, `mixengine-daemon`'s build script
passes `-Wl,--build-id` explicitly. The release binaries carry one today
(`BuildID[sha1]=2bc129f7…` in run 36841381812), but only because the toolchain defaults to it.
`ld64` always writes `LC_UUID`, and MSVC's linker always writes the CodeView record, because rustc
always asks it for a `.pdb`.

## D3. A release build stops resolving names in the hook

In a release build the hook records offsets and **does not resolve symbols**. In a debug build,
which keeps every symbol, it does both, so a developer's own crash still reads as names with no tool
in between. The names come from the same `std::backtrace::Backtrace` render as today, filtered by
the same `frames` function, into `symbols`. They are not paired with `frames`: `std`'s capture starts
inside `std` and the platform's starts inside the hook, so position *n* in one is not position *n*
in the other.

There are two reasons beyond there being nothing left to resolve.

- Symbolising inside a panic hook is the heaviest work the hook does. It walks a symbol table,
  allocates, and on Windows takes dbghelp's global lock while the panicking thread still holds every
  lock it held. ADR 0022 accepts that deadlock as a named hazard. Taking symbolisation out of release
  builds leaves the hook an unwind and one file write.
- dbghelp, asked for a `.pdb` that is not there, searches the configured symbol path, which on a
  developer's machine may be a network symbol server.

## D4. The release keeps the daemon's symbols, and ships without them

**In packaging, not in Cargo.** The daemon keeps `strip = "debuginfo"`, so the binary cargo writes
still has its symbol table. Cargo's `strip = "symbols"` cannot be used for it, because it strips at
link time and the unstripped binary a symbol file is made from would never exist.

`split-debuginfo` was considered and is not the route. Packed split debuginfo on Linux leaves a
skeleton in the binary that `strip = "symbols"` removes, which would orphan the split file. Whether
the two can be combined has not been measured, and the packaging route has no such question.

The pipeline today: `binaries` runs `packaging/stage.sh`, which builds, and hands cargo's output on
through `binaries-hand-on.sh`. `build` unpacks it and runs every packaging script, each of which
calls `stage.sh` again with `MIX_PREBUILT=1` to copy rather than compile. So:

- **`binaries-hand-on.sh` also carries `mixengined.pdb`** on Windows. The unstripped Linux and macOS
  executables are what it carries already.
- **`stage.sh`, per target,** copies the daemon's symbols aside: the unstripped executable on Linux
  and macOS, the `.pdb` and the executable it belongs to on Windows (the executable's sections place
  the `.pdb`'s symbols, and its CodeView record is the build identifier). It writes them as
  `$MIX_OUT/dist/mixengined-<version>-<target>.sym.tar.gz`, then strips the copy it stages:
  `objcopy --strip-all` on Linux, `strip` on macOS. Each macOS slice is stripped and keeps its own
  symbol file before `packaging/macos/build.sh` joins the two with `lipo`, because each slice has
  its own `LC_UUID`. Windows has nothing to strip.
- **Nothing in CI changes beyond that.** `build` already uploads all of `target/packaging/dist/` as
  `mixengine-<os>`. `release` already gathers those, `packaging/sign.sh` already signs everything in
  `dist`, and `release-verify.sh` already checks every signature it finds. A symbol archive reaches
  the GitHub release, signed, by the path every archive takes. It is not in the update feed, and
  nothing installs it. A release asset is the one place a file lives as long as the version it
  belongs to. **The name is chosen to stay out of the feed:** `packaging/feed.sh` picks its payloads
  and helpers by `mixlab-<version>-…` and `mixengine-elevate-<helper version>-…`, and
  `mixengined-<version>-<target>.sym.tar.gz` matches neither. A rename that started with either
  prefix would put a symbol file in front of every updater.

**A Windows release is built with line tables** (`-C debuginfo=line-tables-only`, beside T150's
`crt-static`, both from `mix_release_rustflags` in `packaging/common.sh`). Found while building D5,
on 2026-10-01: a release `.pdb` without them holds only the public symbols, which after LTO are a
minority of the functions, and a release daemon's report read through them put most frames in the
wrong function (`crash::probe::raise` read as `RawVec::grow_one`). With them the `.pdb` holds every
function's name, address and size. On a small release-profile program the executable is
byte-for-byte the same size with and without them, because MSVC keeps debug information in the
`.pdb` alone; the daemon's `.pdb` grows from 6.8 MB to 78 MB, and travels only in the symbol archive. Linux and macOS need nothing of the kind: their
symbol tables already list every function.

**Checks the strip must pass**, in `stage.sh` itself, which already opens what it made: the build
identifier is the same in the stripped binary and in its symbol file (`objcopy --strip-all` keeps
`.note.gnu.build-id`, an allocated section), and on macOS `codesign --verify` passes on each
stripped slice. For the window (#243), the linker's ad-hoc signature was verified on an
Apple-silicon Mac to survive Apple's `strip`, but there it was rustc that ran it.

## D5. `scripts/symbolize.mjs`, for whoever reads the report

A maintainer-only script, with no `mix` command and no daemon method. It reads one report, or a
bundle's `crashes.json`:

1. It downloads the matching `.sym.tar.gz` from the release named by `daemon.version`, for
   `os`/`arch`, through `gh`, and checks its minisign signature against `packaging/updates.pub`.
2. It refuses to go on when the symbol file's build identifier is not the report's `build_id`. A
   report from a self-built binary therefore says so, instead of printing confident wrong names.
3. It reads the symbols **itself**, and looks each frame up at `offset − 1`:
   - `.symtab` on Linux;
   - `LC_SYMTAB` on macOS;
   - on Windows, the procedures each module records in the `.pdb`, placed by the executable's
     sections, with the public symbols as a fallback.

   It undoes Rust's v0 and legacy mangling and prints the frames as names. The design first named
   `llvm-symbolizer` from rustup's `llvm-tools`, but that component does not ship it, which was
   checked on 2026-10-01. A system LLVM would be one more thing every reader and every CI leg had
   to install. Reading the three formats takes Node and nothing else.

Nothing it prints is written back into the report, the bundle or anywhere else.

## D6. Reports already on disk, and the bundle

A home upgraded to this build has format-1 reports in `logs/crashes/`. `Reports::load` reads both
formats. A format-1 report keeps `format: 1`, its names move to `symbols`, and its `frames` is
empty, so a reader of the bundle can see which shape each report was written in. A downgraded
daemon meets a format-2 file it cannot parse and names it as unreadable, which `load` already does
for any report it cannot read.

`crashes.json` is a member of the bundle archive and travels on no RPC, so neither
`PROTOCOL_VERSION` nor `MANIFEST_FORMAT` moves. ADR 0022's point 5 concerns an added `Part`, and
nothing here adds one. `bindings/` gains `CrashFrame` and is regenerated with
`packaging/bindings.sh`.

## D7. What a person is told

[docs/guide/en/troubleshooting.md](../guide/en/troubleshooting.md) and its `vi` twin tell a user
that a crash file holds *"the function names around it"*. From this change it holds the positions
of those functions in the program, which MixEngine's maintainers turn back into names. Both pages
change in the same commit as the format (skill `writing-user-facing-text`). What they promise is
**not** in it (paths, names, passwords) stays word for word.

## D8. Then the daemon's symbols leave the download

With D1–D7 in place, the staged `mixengined` has no symbol table on Linux and macOS: −17% and −18%
on the figures above. The root `Cargo.toml` comment that keeps the daemon's symbols changes in the
same commit, to say where they went.

This spec does **not** take up `opt-level = "s"` again. With the daemon's symbol table out of the
file, the binaries spec's second objection to it is gone. The first, the shim's resolution, is not,
so `"s"` stays a spec of its own.

## Verification

- `crash.rs` unit tests:
  - an offset is the address minus the base;
  - a frame outside the range carries no offset;
  - a release-shaped capture resolves no symbol;
  - a format-1 file loads and keeps `format: 1`;
  - *"nothing a panic said reaches the file"* still holds with the new fields.
- `mixengine-platform`, on each OS, against its own test binary: a base, a range that contains a
  function of the test, a build identifier, and return addresses inside the range. That the
  identifier is the one a symbol file carries is proved end to end below, where the script reads it
  from the file.
- **End to end, on all three OSes, in CI.** A panic in a release-profile daemon, stripped by
  `stage.sh`, gives a report that `scripts/symbolize.mjs` turns back into `mixengined::` names
  using the symbol file from the same run. **A Cargo feature, `crash-probe`, is how it panics.**
  With it, `main` panics right after `Reports::install` when `MIXENGINE_CRASH_PROBE` is set; without
  it, which is every build `stage.sh` makes, the code is not compiled. A test-only binary linking the
  hook is not possible: `mixengine-daemon` is a binary crate with no library to link. The cost is
  named rather than hidden: the binary under test is the shipped one plus one feature. It runs in
  the `bench` job, the one job that already builds a release daemon from `Cargo.toml`'s profile on
  all three systems, so the feature build recompiles only the daemon's own crate.
- Sizes from a `release-exact` run against the figures above.

## Roadmap

**T91a**, under T91 in [phase 9](../roadmap/phase-9-ship.md), pointing here.

## Outcome

Built as designed, with the three corrections D4 and D5 record: the `.pdb` and its executable
travel together, a Windows release is built with line tables, and the script reads the symbols
itself because rustup's `llvm-tools` has no `llvm-symbolizer`.

`mixengined` as it ships, from `release-exact` run 36915070507 against 36841381812:

| Target | Before | After | Change |
| --- | --- | --- | --- |
| Linux x86_64 | 32.55 MB | 27.12 MB | −16.7% |
| Linux aarch64 | 29.95 MB | 23.11 MB | −22.8% |
| macOS universal | 55.21 MB | 46.02 MB | −16.7% |
| Windows x86_64 | 29.27 MB | 29.31 MB | +0.12%, the new code; the `.pdb` does not ship |

Verified:

- The `bench` job's end-to-end check (run 36906744850) passed on Linux, Windows and macOS. In each,
  a stripped release daemon panicked and its report read back as `mixengined::crash::probe::raise`.
- On an Apple-silicon Mac (2026-10-02), with the headless `.pkg` of run 36915070507:
  - the arm64 slice's ad-hoc signature verifies after the strip;
  - the stripped daemon serves `mix status`;
  - each symbol archive's `LC_UUID` equals its slice's;
  - a local probe build symbolised the same way.
- The x86_64 slice was never signed by the linker, before this change or after it.
  `packaging/symbols.sh` verifies a signature only where the unstripped slice had one, which
  the first `build` run (36910692417) showed was needed.

## MixLab

No screen. MixLab's Settings → Diagnostics asks the daemon for the same bundle `mix doctor --bundle`
writes (`daemon.bundle`, `DiagnosticsSection.tsx`). Its `crashes.json` changes shape with
`CRASH_FORMAT`, and the window neither reads nor shows what is inside. `mix doctor`'s crash-report
note, which the window's Doctor section shows, is unchanged: it counts files and reads none.
