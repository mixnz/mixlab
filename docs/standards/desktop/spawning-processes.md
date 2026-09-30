# Spawning a child process

**Every `std::process::Command` in shipped code goes through `crate::platform::hide_console`.**
No exceptions, no matter how short the program runs.

```rust
use crate::platform::hide_console;

let mut command = Command::new("netstat");
command.args(["-ano"]);
let out = hide_console(&mut command).output()?;
```

Never this:

```rust
let out = Command::new("netstat").args(["-ano"]).output()?;   // black window on Windows
```

## Why

A release build has no console of its own — `windows_subsystem = "windows"` — so Windows hands
every child in the console subsystem a brand new one. `netstat`, `tasklist`, `lsof`, `mysqldump`,
`wsl.exe`, a `--version` probe: each one flashes a real black window over the app and vanishes.

This is not cosmetic. A window that appears without being asked for and disappears before it can be
read is what malware looks like to a user, and the standalone client has been reported for exactly that before. A
tool that runs for forty milliseconds is the worst case, not the mildest one — the flash is all the
user sees.

`hide_console` sets `CREATE_NO_WINDOW` and is a no-op everywhere else, so callers need no `cfg` of
their own. It is the only place in the crate that names that flag; do not set creation flags
anywhere else.

## Scope

- **Applies to** anything that reaches `std::process::Command` in code that ships.
- **Does not apply to** `portable_pty::CommandBuilder` in `modules/terminal/local.rs`: a pty child
  is attached to a ConPTY, not to a console of its own.
- **Tests are exempt.** The test binary is a console app already, so nothing flashes. `hide_console`
  there is harmless but pointless — see the `--version` probe in `db/drivers/tools.rs`.

## Checking

`grep -rn "Command::new" src-tauri/src` — every hit outside `platform.rs` and outside a
`#[cfg(test)]` block must have a `hide_console` next to it. Nothing in the build says so, and the
symptom only shows on Windows in a release build, so this grep is the check.
