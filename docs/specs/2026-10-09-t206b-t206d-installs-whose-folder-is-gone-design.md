---
status: approved
date: 2026-10-09
task:
  - T206b
  - T206c
  - T206d
---

# T206b–T206d — the devkit's package database, an empty folder, and installs whose folder is gone (design)

Three follow-ups found by hand while `msys2` (T206a) was being tried. T206b is re-measured and
closed with documentation; T206c is a small fix to `package.uninstall` and `runtime.uninstall`; T206d
makes a runtime or a package whose folder was deleted by hand say so everywhere, instead of reading as
installed.

## T206b — re-measured, and not what it looked like

The roadmap entry says a gem declaring `msys2_mingw_dependencies` (Rails 8's `ruby-vips`) ran pacman
against the package database of the devkit's build date, that four packages answered 404, and that
libvips loaded without its heif, jxl and magick modules. Measured again on 2026-10-09 against the
same install (`.mixengine-home/packages/msys2/2026.10.08`):

- **The install succeeded.** `var/log/pacman.log` ends RubyInstaller's
  `pacman -S --needed --noconfirm mingw-w64-ucrt-x86_64-libvips` with `transaction completed`, and
  libvips 8.18.7 and its 70 dependencies are in `var/lib/pacman/local`. A package that answered 404
  on every mirror would have failed the whole transaction. So the 404s were most likely one
  mirror's, and pacman took the next mirror in `mirrorlist`. pacman logs no download errors, so that
  part is inferred, not read off the log.
- **The missing modules are optional dependencies.** libvips' `desc` lists `libheif`, `libjxl`,
  `imagemagick`, `openslide` and `poppler` under `%OPTDEPENDS%`. `pacman -S` never installs those,
  and RubyInstaller asks for nothing more. `ucrt64/lib/vips-modules-8.18/vips-heif.dll` is there, and
  does not load without `libheif`. A database refreshed that morning gives the same result.
- **An old database still downloads.** Asked today, the 2026-10-08 database resolves those five
  optional packages to 27 URLs, and all 27 answer 200 from `mirror.msys2.org` and from
  `repo.msys2.org`. MSYS2 keeps superseded files for a long time: `repo.msys2.org/mingw/ucrt64/`
  still lists `gcc-14.2.0-2` and `libvips-8.16.0-2`.

**Decision: no code.** Refreshing the database before RubyInstaller runs (`pacman -Sy`) would fix
nothing that was measured. It would also be a partial upgrade, which MSYS2 does not support: a
library downloaded against a newer database may need a newer version of one already installed. A
scheduled rebuild of `msys2` would only answer a database old enough to have lost its files, and
nobody has seen one yet.

What changes is what a person is told:

- mixengine-packages, `docs/packages/msys2.md`, a section *What a gem's MSYS2 packages bring*:
  RubyInstaller installs a gem's declared MSYS2 packages and none of their optional dependencies.
  An optional module is added with `pacman -S <name>` from the devkit's `usr/bin` (for libvips:
  `mingw-w64-ucrt-x86_64-libheif`, `-libjxl`, `-imagemagick`). If a database ever grows old enough
  for its files to be gone, the answer is a newer `msys2` from `mix package available`, not a
  `pacman -Sy` inside this one.
- `docs/features/runtime-versions.md`, the Ruby on Windows section: the same two sentences, and a
  link to that page.
- The roadmap entry is rewritten to say what was measured and ticked by the commit that lands this.

## T206c — the package's empty folder

`package.uninstall` removes `packages/<name>/<version>/` and leaves `packages/<name>/` behind once its
last version is gone. `runtime.uninstall` does the same to `runtimes/<kind>/`.

- After `discard`, both call one new core function, `paths::remove_if_empty(parent)`. It calls
  `std::fs::remove_dir` and ignores every failure: a directory that still holds a version or a
  `.staging` directory is refused by the OS, and that is the answer.
- Removing an empty directory is housekeeping, never a failure of the uninstall. An error is logged
  at `debug` and the uninstall answers as before.
- **The one race is accepted.** An install of another version of the same package creates
  `packages/<name>/`, then `.<version>.staging` inside it. A `remove_dir` landing between those two
  makes that install fail with an I/O error naming the staging directory, and asking again succeeds.
  Two jobs on the same package in the same millisecond is not worth a lock across both maps.

## T206d — an install whose folder is gone

Found by hand on 2026-10-08 with `msys2`: the folder was deleted, and `mix package list`,
`mix doctor` and MixLab all kept saying the devkit was there. The shim kept pointing `MSYS2_PATH` at
the missing folder, and MixLab's steps panel unlocked Run over a Ruby that could not build gems. A
runtime has the same gap: `ruby` through the shim fails with an OS error about a program that does
not exist.

### D1 — `missing` is read from the disk, never stored

- `runtimes::records` and `packages::records` check each row's `install_path` with
  `std::fs::symlink_metadata`. **Only `NotFound` makes a row missing.** Any other error (no access, a
  drive not mounted) leaves the row as installed, because a folder out of reach is not a folder
  gone, and nothing here may act on a guess.
- No migration, no column, no background scan. The answer is current every time it is asked, and
  costs one `stat` per row, against tables of a few dozen rows.
- **Nothing deletes the row.** Only `mix runtime uninstall` and `mix package uninstall` remove it, and
  both already treat an absent folder as removed (`runtimes::discard`).
- It is stamped wherever core builds a summary from a row: `records`, `record`, `remember` and
  `restore` (D3). Because of that, every answer that carries a summary (`runtime.list`, a finished
  install's job, `runtime.set_default`, an uninstall's `removed`) says it, and no daemon call site
  has to remember to.
- The wire types carry it as `RuntimeSummary::missing` and `PackageSummary::missing`, both
  `Option<bool>` under ADR 0019: `None` is a daemon from before this task, never "could not tell".
  `is_missing()` on each type reads `Some(true)`. Each type's older-peer test decodes it as `None`.
  `bindings/` is regenerated.

### D2 — a missing install is not counted as installed

One rule, applied to each reader of the two tables. **A reader that decides what runs, or what is
offered as installed, skips a missing row. Every other reader keeps it.**

| Reader | What it does with a missing row |
|---|---|
| `resolve::runtime` (shim, pools, `mix runtime which`) | Skips it when choosing among versions that match a constraint. When only missing versions match, or the default is missing, refuses with the new `Error::RuntimeMissing { kind, version, path }`: *"ruby 3.4.11 is recorded but its folder is gone"*, hint *"`mix runtime install ruby 3.4.11` puts it back; `mix runtime uninstall ruby 3.4.11` forgets it"*. **A default is never replaced silently** by another version. |
| shim's `installed_devkit` | Skips it. A Ruby is pointed at the newest `msys2` that is there, or at none. |
| `runtime.list_available`, `package.list_available` | `installed: false` for that version, so it is offered again. It is never the base of an offered update (T193). |
| blueprint plan (`blueprints/plan.rs`) | Unchanged. A plan is a function of this home's tables and reads no disk (T77, D9). An apply over a missing runtime stops at its first step that runs it, on `resolve::runtime`'s `RuntimeMissing`. |
| `certs/jdks`, doctor's Go and Java checks | Unchanged. `jdks` already reports a `keytool` it cannot run, and the doctor checks only ask whether any version is recorded. |
| adopt walk, `bin_watch`, `upgrade`, uninstall | Unchanged. They need the row, not the folder. |

### D3 — installing the same version again restores it in place

- `runtime.install` and `package.install` refuse a version that is already recorded
  (`AlreadyRecorded`, `PackageAlreadyRecorded`). **A recorded version that is missing is no longer
  refused.** The install downloads, verifies, smoke-tests and renames into place as any other
  install does.
- The row is then **updated, not inserted**: new core functions `runtimes::restore` and
  `packages::restore` write the path, size, URL, hash, `provides` (and, for a runtime, channel,
  `extension_dir` and `extensions_json`) over the existing row. The row's id stays, so do
  `is_default` and every service that is an instance of the package (`ON DELETE RESTRICT`).
- If the folder came back between the check and the rename (a person restored it from a backup),
  the install meets `AlreadyInstalled`. A restore does not hand that to the adopt path, which would
  try to insert a second row. It answers `already_exists`, as for a version that is there, and the
  row is untouched.
- No new method. `mix runtime install ruby 3.4.11` and `mix package install msys2 2026.10.08` are
  the repair, and are what every message about a missing install names.

### D4 — `mix` says so

- `mix runtime list` and `mix package list`: the `INSTALLED` column reads `missing` instead of a
  date. Under a table with any missing row, one line names both commands for the first missing row.
- `mix doctor` gains one check, *installs recorded but not on disk*, reported as a `Problem` with
  the new `ProblemId::InstallMissing`. It lists each missing runtime and package with its path.
  `daemon.doctor_repair` maps it to `Planned::Untouched`, with a sentence naming the install and
  uninstall commands: the repair is a download a person asks for, not something a repair may start
  (the precedent is `DomainUnreachable` and `DnsServerUnavailable` in `repair.rs`).
- **Out of reach is a `Note`, not a problem.** A row whose folder cannot be checked (D1's "any other
  error") is listed in the same check as a `Note` when nothing is missing, naming the error.

## MixLab

All of it is inside the `mixengine` module; with MixEngine off, nothing here runs. No new daemon
method, so `check-client-surface` has nothing to add. Strings go through `t()` in `en` and `vi`.

- **Packages → Languages** (`screens/Packages/Languages.tsx`): an installed runtime whose `missing`
  is `true` shows a warning mark *"Folder is gone"* with its path as the tooltip, and two buttons:
  **Reinstall**, which starts `runtime.install` for that version through the existing job flow, and
  **Uninstall**, the existing confirm dialog.
- **The devkit notice** reads `package.list` for `msys2` rather than `devkitOffer`'s `installed` flag
  from `package.list_available`. Reading the catalogue alone, a missing devkit would show **Install
  devkit** and install the newest release, leaving the missing row behind. The notice has three
  states:
  - none recorded: **Install devkit** with its size, as today;
  - recorded and missing: *"The devkit's folder is gone"*, with **Reinstall** (that version,
    through `package.install`) and **Remove** (`package.uninstall`);
  - recorded and there: *"Ruby builds native gems with the devkit"*, with **Remove devkit**
    (`package.uninstall`, behind a confirm dialog).
- The Ruby rows' `native gems` mark counts only a devkit that is there.
- **Packages list** (`PackageList.tsx`): a missing package shows the same mark and a **Reinstall**
  button beside **Uninstall**.
- `devkit.ts` gains `devkitState(packages: PackageSummary[])`, answering `{ state: "absent" }`,
  `{ state: "missing", version }` or `{ state: "present", version }`. It answers present for the
  newest `msys2` that is there, else missing for the newest recorded one. It is unit-tested beside
  `devkitOffer`.
- **The steps panel** (`NextStepsPanel.tsx`) and the Apply dialog keep reading
  `package.list_available`. With D2 a missing devkit reads as not installed there, so Run is
  locked again over a Ruby that cannot build gems: the bug found by hand.
- **A Reinstall a catalogue cannot serve** (the index no longer offers that version, or the index
  could not be read) is shown disabled, with a tooltip saying so. Uninstall still works.

## Not in scope

- Repairing a folder that is there but damaged (files deleted inside it). `missing` is about the
  folder, and the smoke test at install time is the only check of the contents.
- Extension rows of a restored PHP runtime: they live in `etc/` and are regenerated from state as
  they are after any install.
- An empty `etc/<kind>/` after a runtime's generated ini set is removed.

## Is there an ADR here? No

- `missing` follows ADR 0019 as written.
- A `ProblemId` that a repair leaves untouched has precedents (`DomainUnreachable`,
  `DnsServerUnavailable`). Like every `ProblemId` added since protocol 1, an older `mix` cannot
  decode a report that carries it. That cost is already paid by every phase that added one, and this
  spec does not change it.
- Restoring a row in place keeps the install-then-record ordering of `crate::runtimes`: the folder is
  renamed into place first, and the row is written after it.

## Testing

- **core:** `records` gives `missing` for a deleted folder and not for one that is there. The
  decision itself is a pure function of the `io::Result` (`gone(&io::Result<Metadata>) -> bool`),
  tested with every `ErrorKind` that matters, because no path fails with "permission denied" the same
  way on all three systems. `restore` keeps the id, `is_default`
  and a service's `package_id`. `remove_if_empty` removes an empty folder, leaves one holding a
  version or a `.staging` directory, and does not fail on one that is gone. `resolve::runtime`
  skips a missing version under a constraint, and refuses with `RuntimeMissing` for a missing
  default and for a constraint only missing versions match.
- **daemon:** `package.install` of a recorded, missing version succeeds and keeps the row's id and
  services. A recorded version that is there is still refused. `package.uninstall` of the last
  version removes `packages/<name>/`, and of one of two keeps it. `runtime.uninstall` likewise.
  Doctor reports `InstallMissing` for a deleted folder.
- **shim** (unit, through `surroundings`): a missing `msys2` gives no `MSYS2_PATH`, and with one
  missing and one present, the present one is chosen.
- **cli:** the `INSTALLED` cell reads `missing` and the line under the table names both commands.
- **proto:** the floor fixtures of `RuntimeSummary` and `PackageSummary` decode `missing` as `None`.
- **MixLab:** `devkitState` for each state; Languages shows Reinstall and Uninstall for a missing
  runtime and the three devkit notices.
- **By hand on Windows:** delete `packages/msys2/<v>` while the daemon runs. `mix package list` says
  `missing`, `mix doctor` reports it, `ruby -e 'p ENV["MSYS2_PATH"]'` through the shim prints `nil`,
  MixLab offers Reinstall, and Reinstall brings `gem install bcrypt` back. Then `mix package
  uninstall msys2 <v>` leaves no `packages/msys2/`.

## Documentation, when it lands

- `docs/features/runtime-versions.md`: the T206b note, and what a runtime whose folder is gone does.
- `docs/features/services.md` (packages): `missing`, and that installing the same version restores it.
- `docs/features/client-surface.md`: the `missing` member of both summaries.
- `docs/roadmap/phase-44-ruby-on-windows-builds-native-gems.md`: T206b rewritten to the measurement;
  T206b, T206c and T206d ticked; `todo.md`'s count. Root `CHANGELOG.md` under `[Unreleased]`.
- mixengine-packages: `docs/packages/msys2.md`, *What a gem's MSYS2 packages bring*.
