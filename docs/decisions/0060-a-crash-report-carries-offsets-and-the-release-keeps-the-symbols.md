# 0060. A crash report carries offsets, and the release keeps the symbols

**Status**: Accepted. It widens point 3 of
[0022](0022-a-crash-report-is-recorded-by-default-and-sent-by-nothing.md), the list of what a crash
report may contain, by two kinds of field, and changes nothing else in it.
**Date**: 2026-10-01

## Context

A crash report (T91) names the frames of the panicking thread by resolving them **inside the panic
hook**, from the running binary's own symbols. Measured on 2026-10-01:

- **On Windows every release report so far is nameless.** MSVC writes the symbols to
  `mixengined.pdb`, and nothing ships or keeps that file. A release-profile program asking for its
  own backtrace without its `.pdb` prints `<unknown>` for every frame.
- **On Linux and macOS the names cost a sixth of the daemon.** `mixengined` is the one binary
  that ships its symbol table, for this hook alone: 5.5 MB of 32.5 MB on Linux x86_64 and 4.6 MB of
  26.1 MB on a macOS arm64 slice.

Resolving inside the hook is also its heaviest work. It walks a symbol table and allocates, and on
Windows it takes dbghelp's lock while the panicking thread holds every lock it held, which is
the deadlock 0022 accepted as a named hazard.

## Decision

1. **A frame is an offset into the executable**, and the report carries the executable's **build
   identifier**: the GNU build-id note, `LC_UUID`, or the CodeView GUID and age. Both join 0022's
   list as its fourth and fifth kinds of safe field. Each is a constant of the build, the same on
   every machine that runs it, and carries nothing from the home. A frame outside the executable
   keeps no offset and no module name, because a module's name is a path on the machine.
2. **A release build resolves nothing in the hook.** A debug build still records names, in a list
   of their own.
3. **The release keeps the daemon's symbols beside the download.** Packaging strips the shipped
   `mixengined` and writes its symbols as `mixengined-<version>-<target>.sym.tar.gz`, which the
   release signs and publishes like every other asset, outside the update feed.
4. **Names are restored by whoever reads the report**, with `scripts/symbolize.mjs`, which refuses
   a symbol file whose build identifier is not the report's.

The rest of 0022 stands: nothing is transmitted, recording is on by default, the panic message
stays out of the report, and `mixengine-elevate`, `mix` and the shim install no hook.

## Consequences

**Easy:**

- A Windows report names its frames for the first time, once its symbol file is fetched.
- `mixengined` loses its symbol table on Linux and macOS, about a sixth of the file.
- The hook in a release build is an unwind and a file write.

**Hard, and accepted:**

- **A report alone no longer reads as names.** Someone has to run the script, with `gh` and
  rustup's `llvm-tools`. Before, the reader of a Linux or macOS report needed nothing; now every
  reader needs the script. Windows reports were unreadable either way.
- **A self-built release cannot be symbolised from a published asset.** Its build identifier
  matches nothing published, and the script says so rather than guess.
- **Every release carries one more asset per target**, of the daemon's unstripped size, kept as
  long as the release is.

## Alternatives considered

**Keep the symbols in the binary.** It is the status quo, and it leaves Windows reports nameless
anyway.

**Ship the `.pdb` beside `mixengined.exe`.** That would fix Windows alone, at the cost of a
download that grows instead of shrinking. Linux and macOS would still carry their tables.

**The `backtrace` crate for the addresses.** It is what `std` vendors, but it brings `addr2line`,
`gimli` and `object` for a resolver no release calls. Its current manifest also asks for a
`miniz_oxide` the tree has at another version, which `deny.toml` refuses. The unwinder's own two
functions per platform, in `mixengine-platform`, are less.

**`split-debuginfo` instead of stripping in packaging.** On Linux it leaves a skeleton in the binary
that `strip = "symbols"` removes, which would orphan the split file, and the combination has not
been measured. Stripping a copy in packaging has no such question.
