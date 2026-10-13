---
name: before-pushing
description: Use before pushing a branch or asking CI for a run, and before cutting a branch from master — especially after touching crates/mixengine-platform/src/linux or macos, a #[cfg], anything mixengine-elevate compiles, or a step in .github/workflows/.
---

# What `scripts/gate.sh` cannot see

`bash scripts/gate.sh` is the floor: rustfmt, clippy, rustdoc with `-D warnings`, the docs and
client-surface checks, and the desktop build. Run it first. It answers for **this operating
system, a debug build and the default feature set** — and every CI run that went red after a green
gate went red in one of the gaps below. A CI round costs fifteen to twenty minutes; each check here
costs seconds to a few minutes.

## Before cutting a branch

```bash
git status --short                       # a clean tree
git log --oneline origin/master..master  # must be empty
```

A clean tree is not a pushed one. A branch cut from a master holding an unpushed commit carries that
commit into its PR, and the squash merge folds it into a commit about something else — the content
survives, its commit boundary does not. Push master first.

## The release build and the narrow features

```bash
cargo check --workspace --release --all-targets
cargo test -p mixengine-elevate --test system --no-run
```

- `#[cfg(debug_assertions)]` code is absent from release, so the `bench` and `system` legs go red
  while every `test` leg is green.
- `mixengine-elevate` builds `mixengine-platform` with `default-features = false,
  features = ["elevated"]`. A `mod` that lost its feature gate breaks only there.

## Another operating system's files

Clippy on Windows compiles `windows/` only; `linux/` and `macos/` are not in the build at all.

**Linux**, after touching `crates/mixengine-platform/src/linux/`, a field of `sys::Host`, or the
`Host` trait — through WSL, with a target directory of its own so it does not fight the Windows one:

```bash
wsl -- bash -lc "cd <repo in /mnt/c/...> && CARGO_TARGET_DIR=/tmp/mixtarget cargo check -p mixengine-platform --all-targets"
```

**macOS**, after touching `macos/`, a `#[cfg]` in `trust/`, or anything `mixengine-elevate`
compiles — with `rustup target add aarch64-apple-darwin` once:

```bash
cargo clippy -p mixengine-elevate --target aarch64-apple-darwin -- -D warnings
cargo clippy -p mixengine-platform --target aarch64-apple-darwin --features host -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p mixengine-platform --no-deps --document-private-items --all-features --target aarch64-apple-darwin
```

Exactly these two crates, library only: anything wider pulls a build script that needs a C compiler
for macOS. The third command is not covered by the first two — a broken intra-doc link that only
one OS compiles is seen only by that OS's rustdoc. Two items gated to **different** operating
systems never link to each other in docs; name the other in backticks.

## A new or changed CI step

Re-running the step's command here proves nothing until the job's environment is set too. Every
reusable workflow sets `CARGO_TERM_COLOR: always`, `CARGO_INCREMENTAL: 0` and `RUST_BACKTRACE: 1`:

```bash
CARGO_TERM_COLOR=always CARGO_INCREMENTAL=0 RUST_BACKTRACE=1 bash -c '<the step>'
```

Cargo turns colour off when its output goes into a pipe, so a step that parses cargo's output is
green here and red on CI, where ANSI codes break the `sed`. Give any parsed cargo command
`--color never`.

## Two edits that compile clean and break later

- **Inserting before an anchor steals its attributes.** The line above an item is usually its
  `#[cfg]`, `#[derive]` or `///`. A new `mod` inserted above `mod fakeservice;` takes its
  `#[cfg(debug_assertions)]`, and rustfmt then sorts the two apart. Read the line above the anchor,
  or insert after the whole item.
- **`clippy::result_large_err` fires only on private functions.** `mixengine_core::Error` is over
  128 bytes, and exported functions are exempt — so a new `examples/` or `[[bin]]` returning
  `mixengine_core::Result` goes red while the library stays green. Box the error at the boundary,
  as `mixengine-daemon/src/certs.rs` does. Do not add `#[allow]`.
