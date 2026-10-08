# Phase 44 — Ruby on Windows builds native gems

*Goal: a Windows Ruby says it cannot build gems with C extensions wherever it is chosen, and one
install of a MixEngine-managed devkit makes every installed Ruby build them.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-10-08-t206-ruby-on-windows-builds-native-gems-design.md](../specs/2026-10-08-t206-ruby-on-windows-builds-native-gems-design.md).

---

- [x] **T206** The index carries an artifact's `lacks` (mixengine-packages' `mkindex.py` and index
      schemas); `index::format::Artifact` and `RuntimeRelease` read it; `mix runtime available`
      draws a `LACKS` column and `mix runtime install ruby` names the devkit; MixLab's Languages
      screen marks a Ruby that cannot build native gems.
- [x] **T206a** **(P)** `msys2`, a package built on CI for Windows x86_64 (UCRT gcc) and Windows
      ARM64 (clang), signed and smoke-tested like the others; installed into the home with no
      service; the shim sets `MSYS2_PATH` for every Ruby command, leaving a session's own value;
      MixLab's **Install devkit** button; `rails.toml`'s notes name it (on T205's branch).
- [ ] **T206b** **(P)** A gem that declares `msys2_mingw_dependencies` makes RubyInstaller run
      pacman inside the installed `msys2`, against the package database of its build date: measured
      with Rails 8's `ruby-vips`, four packages MSYS2 had since replaced answered 404 and libvips
      loaded without its heif, jxl and magick modules. Keep that database current, by rebuilding on
      a schedule or by refreshing it before RubyInstaller installs.
- [ ] **T206c** `mix package uninstall` leaves the package's empty directory (`packages/msys2/`)
      behind once its last version is gone.
- [ ] **T206d** **(P)** A package whose folder was deleted by hand still reads as installed: found
      by hand on 2026-10-08 with `msys2`, where `mix package list`, `mix doctor` and MixLab all kept
      saying the devkit was there, the shim kept pointing `MSYS2_PATH` at the missing folder, and
      MixLab's steps panel unlocked Run over a Ruby that could not build gems. Only
      `mix package uninstall` repairs it (it already treats an absent folder as removed), and
      MixLab has no control for a toolchain. Needs a short spec: the daemon marks such a row
      `missing` and stops counting it installed, without deleting it (a folder can be out of reach,
      not gone); `mix package list` and `mix doctor` say so and name the uninstall; the shim skips
      a devkit whose folder is absent; MixLab's Languages tab offers *Reinstall* for a missing
      devkit and *Remove devkit* for one that is there.
- [x] **T206e** **(P)** A home with `msys2` installed could not start its daemon in any reasonable
      time on Windows: every start restricted the home's root with `icacls /reset` and
      `/inheritance:r`, which rewrites the inherited permissions of every file under it, and
      `msys2` is 54,487 files. Found by hand on 2026-10-08, when MixLab could not start MixEngine
      and three daemons sat in `icacls` with no line logged. `Paths::bootstrap` now asks
      `is_restricted_to_owner` (one listing of the directory itself) and restricts only a
      directory where the restriction is not in force, so a restored or loosened home is still
      repaired.

**M44** On Windows x86_64 and on a Windows 11 ARM64 machine, `mix package install msys2 <version>` and then the
`rails` blueprint's steps end with `rails server` answering, and uninstalling the package brings
RubyInstaller's own "MSYS2 could not be found" back.
