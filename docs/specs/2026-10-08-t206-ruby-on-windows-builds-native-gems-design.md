---
status: implemented
date: 2026-10-08
task:
  - T206
  - T206a
---

# T206 — Ruby on Windows builds native gems (design)

MixEngine's Ruby on Windows cannot install a gem with a C extension, and nothing in MixEngine says
so. This design makes the gap visible wherever a Ruby is chosen (T206), then closes it with a devkit
MixEngine installs and removes like any other package (T206a).

## What this closes

Measured on 2026-10-08 with Ruby 3.4.11 installed by `mix runtime install ruby 3.4.11` (mix 0.0.16,
Windows 11 x64, `x64-mingw-ucrt`):

- `gem install rails --no-document` exits 1 with *"MSYS2 could not be found. Please run 'ridk
  install'"*. It stops at `websocket-driver`, the first gem with a C extension. `railties` is already
  in by then, so `rails` resolves through T131's globals shim and looks installed.
- `rails new . --database=postgresql` exits 0, but the `bundle install` inside it fails the same way
  (`puma`, `bootsnap`, `nio4r`, `msgpack`, `bcrypt`…), so `ruby bin/rails server` stops on
  `Bundler::GemNotFound`. The `rails` gallery blueprint (T205) cannot end at a working site on
  Windows.

The gap was already known, on the other side. `mixengine-packages` borrows Ruby for Windows from
RubyInstaller and writes into each Windows artifact a top-level `lacks` field: `"native gems"` (no
compiler; the toolchain is RubyInstaller's separate ~1 GB MSYS2) and `"yjit"`. Its schema explains
why the field exists: *"a blueprint asking for a native gem can fail where it is written rather than
on somebody's machine"*. **That never happens, because `tools/mkindex.py` copies only `requires`,
`extension_dir` and `extensions` into the index.** `lacks` stops at the artifact, and MixEngine reads
nothing of it.

## Goal

- **T206.** Wherever a person chooses a Ruby (`mix runtime available`, `mix runtime install`,
  MixLab's Languages screen), a Windows Ruby says it cannot build native gems, and says what fixes
  it.
- **T206a.** `mix package install msys2 <version>`, or one button in MixLab, installs a devkit into the home.
  After that, `gem install` and `bundle install` build C extensions with every installed Ruby, on
  Windows x86_64 and Windows ARM64. The `rails` blueprint's steps run to a page from
  `rails server`.

## Not in scope

- **YJIT on Windows.** CRuby does not build it for `x64-mingw-ucrt`, and no package changes that.
  T206 shows it as the second `lacks` entry, and that is all.
- **A blueprint plan that adds the devkit by itself.** A plan reads this home's tables and never the
  index (T77, D9), and `lacks` lives in the index. The `rails` blueprint's note names the package
  instead.
- **MSYS2 for anything but building gems.** The package is a toolchain for Ruby. It puts nothing on
  the person's `PATH` and no shim for `bash`, `pacman` or `gcc`.
- **Keeping the devkit's packages current between releases.** Each release of the package is a whole
  new tree built on CI; `pacman -Syu` inside an installed one is the person's business.

## Is there an ADR here? No

- Reading a field the index carries is the same decision T148 took for `requires`.
- A package with no service is what `package.install` already allows: *"a package whose recipe names
  none is installed without one"* (`daemon/src/packages.rs`).
- A shim setting one variable for one language follows `java_home` (T27e) and `toolchain` (T27d).

No cross-cutting rule changes.

## Decisions

### D1 — The index carries `lacks` (mixengine-packages)

- `tools/mkindex.py` copies `lacks` beside `requires`, `extension_dir` and `extensions`.
- `schema/index.schema.json` and `schema/index-v2-kind.schema.json` allow it on an artifact, with the
  same shape as `artifact.schema.json` (an object of strings, at least one property).
- Older MixEngine builds ignore it: `index::format::Artifact` denies no unknown field. So this ships
  before any MixEngine release reads it.

### D2 — MixEngine reads `lacks` and puts it on the wire

- `core::index::format::Artifact` gains
  `#[serde(default, skip_serializing_if = "BTreeMap::is_empty")] lacks: BTreeMap<String, String>`.
- `mixengine_proto::RuntimeRelease` (`runtime.list_available`) gains the same member, optional under
  ADR 0019, for the artifact this machine would install. Packages get nothing: no package lacks
  anything today.
- **Words, not codes.** The keys are the publisher's (`"native gems"`, `"yjit"`) and the values are
  its reasons. A client prints the key, and the reason where it has room. MixEngine judges none of
  them: a capability is missing or it is not, and the machine does not change that (unlike
  `requires`, which T148 judges against the machine).

### D3 — `mix` says what a Ruby lacks where it is chosen

- `mix runtime available` gains a `LACKS` column, drawn only when some row lacks something: T151's
  rule for `NEEDS`.
- `mix runtime install ruby <v>`: when the release it installed lacks `native gems`, the report ends
  with one line, *"This Ruby cannot build gems with C extensions. `mix package install msys2
  2026.10.08` adds the toolchain."*, naming the newest version offered, because `mix package
  install` takes one. The line appears only where a `msys2` package is offered for this machine;
  on a cell with no devkit it names the gap and nothing else.
- `mix runtime list` (installed) does not repeat it. What an installed release lacks is read from
  the index, and `list` deliberately does not reach the network.

### D4 — `msys2`, a package built and signed like the others (mixengine-packages)

- A recipe `tools/msys2.py` and a workflow `build-msys2.yml` with two cells:

  | Cell | Runner | Base | Toolchain (RubyInstaller's `ridk install 3` set) |
  |---|---|---|---|
  | `windows-x86_64` | `windows-2022` | `msys2-base-x86_64-latest.sfx.exe` (self-extracting: Python's `tarfile` reads no zstd) | `base-devel`, `mingw-w64-ucrt-x86_64-toolchain` |
  | `windows-aarch64` | `windows-11-arm` | the same x86_64 base (MSYS2 publishes no ARM64 `usr/` at the time of writing; it runs emulated on Windows 11 ARM, and the recipe checks that before relying on it) | `base-devel`, `mingw-w64-clang-aarch64-toolchain` |

- **Built, not fetched at install time.** The recipe unpacks the base, initialises it
  (`bash -lc exit`, which writes the keyring), runs `pacman -Syu --noconfirm`, then installs the
  toolchain with `pacman -S --needed --noconfirm`. It clears `var/cache/pacman/pkg` and packs the
  tree. A person's machine then needs no MSYS2 mirror, no keyring and no `pacman` run, and the bytes
  are the ones this pipeline signed.
- **Version is the build date**, `2026.10.08`, so a newer build is the newer release in MixEngine's
  lines-and-updates model (T193).
- **Smoke test after relocation**, as every artifact is: from the moved tree, compile and run a
  one-line C program with the cell's compiler (`ucrt64/bin/gcc.exe` on x86_64,
  `clangarm64/bin/clang.exe` on ARM64). The ARM64 cell's smoke test is why it needs the ARM runner.
- `provides` names `bash` and the cell's compiler as `cc`, because both index schemas require at
  least one entry, and one name on both cells because the publisher refuses a version whose cells
  name different commands. They are what the smoke test ran. **MixEngine adds none of them to `bin/`**: package
  commands come from a recipe's `clients()`, and a toolchain has no recipe (D5).
- `keeps` names each directory holding a static or import library (`ucrt64/lib` or
  `clangarm64/lib`, and `usr/lib`): the repository's rule throws those out of a runtime, and a
  toolchain's compiler links against them.
- `docs/packages/msys2.md` records the size, which `lacks` text it answers, and that the x86_64 base
  under ARM64 is emulated by design.
- The Windows Ruby's `lacks` entry stays as it is. It describes the Ruby artifact, which still ships
  no compiler.

### D5 — MixEngine installs it as a package with no service

- **The catalogue learns a second kind of package, a toolchain.** Today `package.install` and
  `package.list_available` accept only a name with a recipe (`daemon/src/packages.rs`, `runnable`:
  *"a package with no recipe would unpack into a directory nothing could start"*), and a `Recipe`
  must describe a service (`spec()`). So `Catalogue` gains `toolchains()`: names it installs, lists
  and removes but never runs. `installable()` is packages and toolchains together, and `msys2` is
  the one toolchain.
- With that, `package.install msys2` takes the existing path: it downloads, verifies, unpacks into
  `<home>/packages/msys2/<version>`, records the row, and counts it in disk usage. `package.uninstall`
  removes it.
- **No recipe and no service.** Nothing in it is supervised. `service.create msys2` is refused with
  a sentence saying the package is a toolchain, not a server, ahead of the generic "cannot run"
  refusal, whose list would not name it.
- On macOS and Linux the index offers no `msys2` artifact, so the install is refused by the same
  "no release for this machine" rule as any package without a cell there. No `#[cfg]`.

### D6 — The shim points Ruby at the devkit

- `mixengine-shim`'s `surroundings` gains `devkit(kind, store-answer, environment)`, beside
  `java_home` and `toolchain`. For `RuntimeKind::Ruby`, and when a `msys2` package is installed, it
  sets **`MSYS2_PATH`** to that install's directory (the newest, when there are several).
- **Why this variable:** RubyInstaller looks for MSYS2 in a fixed order (`ruby_installer/runtime/
  msys2_installation.rb`, `iterate_msys_paths`): `MSYS2_PATH`, `<ruby>/msys64`, the directory next to
  the Ruby, `C:\msys64`, the registry, `PATH`, then scoop. `MSYS2_PATH` is first, so one devkit serves
  every installed Ruby version, and no Ruby directory is written to.
- **The session's value wins.** A person who exported `MSYS2_PATH` for an MSYS2 of their own meant
  it. That is `toolchain`'s rule, and the opposite of `java_home`'s, for the reason `toolchain` gives:
  the variable is set by the person, not by an installer.
- It reaches every Ruby command a shim starts: `ruby`, `gem`, `bundle`, `ridk`, and the globals of
  T131 (`rails`, `rake`…), because all of them become a Ruby program through the same function.
  RubyInstaller then enables the devkit itself (`ridk enable` behaviour) when a gem needs a compiler.
- Nothing is set for any other language, or when no `msys2` package is installed.

### D7 — The gallery says what to install

`rails.toml`'s notes, written by T205 after the measurement above, change to:

- `gem install rails`: *"Installs into the runtime this project pins, which its other projects
  share. On Windows, install the devkit first: the Install devkit button on the Ruby row of the
  Languages tab, or mix package install msys2 with a version from mix package available."*
- `rails new . --database=postgresql`: *"It runs bundle install, which on Windows needs the same
  devkit."*

The commands stay as they are: a step is the same on every system (T205, D4).

**Landed on T205's branch, not this one.** T206 is built first, so that T205 can be tested end to
end on Windows. `rails.toml`'s `[[next_steps]]` exist only on T205's branch, so the note change is
made there once T206 is on `master`.

## MixLab

- **Packages → Languages** (`screens/Packages/Languages.tsx`): a Ruby release whose `lacks` is not
  empty shows each key as a quiet mark ("can't build native gems", "no YJIT"), with the publisher's
  reason as the tooltip. Where a `msys2` release is offered for this machine and none is installed,
  the `native gems` mark carries an **Install devkit** button that starts `package.install` for
  `msys2`, with its size, through the same job flow the Packages screen already uses. Once a `msys2`
  package is installed, the mark reads "builds native gems with the devkit".
- **Packages list:** `msys2` appears like any other package, in `packageCategories.ts`'s catch-all
  `"other"` (it names no new category), and is uninstalled from there.
- **No new daemon method.** `runtime.list_available` gains an optional member, and the button calls
  `package.install`. `check-client-surface` has nothing to add, and `bindings/` is regenerated.
- **With MixEngine off,** nothing here runs: it is all inside the `mixengine` module.
- Strings go through `t()` in `en` and `vi`.

## Testing

- **packages:** `mkindex` writes `lacks` for a Ruby Windows artifact and nothing for one without it.
  Both index schemas accept it. The `msys2` smoke test compiles and runs C from the relocated tree on
  both runners.
- **core:** an index entry with `lacks` reads into `Artifact.lacks`; one without it reads as empty.
  `runtime.list_available` carries it for the artifact this machine would take.
- **shim** (unit, through `surroundings`): Ruby with a `msys2` package installed gets `MSYS2_PATH`
  naming its directory. Without the package it gets none. With `MSYS2_PATH` already in the session,
  that value stands. PHP or Node with the package installed get nothing. Two installed `msys2`
  versions give the newest.
- **cli:** the `LACKS` column appears only with a row that lacks something; the install line appears
  for `native gems` and names `msys2` only where one is offered.
- **daemon:** `service.create msys2` is refused, naming the package as a toolchain.
- **By hand, on Windows x86_64 and on a Windows 11 ARM64 machine:** `mix package install msys2 <version>`, then
  the `rails` blueprint's steps from a MixLab Terminal tab, ending with `rails server` answering at
  `https://<slug>.test`. Then `mix package uninstall msys2`, and `gem install bcrypt` fails again
  with RubyInstaller's own sentence.

## Documentation, when it lands

- `docs/features/runtime-versions.md`: Ruby on Windows, `lacks`, and the devkit.
- `docs/features/services.md` (or wherever packages without services are listed): `msys2`.
- A roadmap phase for T206 and T206a; root `CHANGELOG.md` under `[Unreleased]`.
- mixengine-packages: `docs/packages/msys2.md`, and `docs/packages/ruby.md` says the gap is closed by
  the `msys2` package.
