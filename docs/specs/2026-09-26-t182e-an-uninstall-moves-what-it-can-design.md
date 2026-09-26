---
status: approved
date: 2026-09-26
task: T182e
---

# An uninstall moves what it can past other programs, and names only what it cannot

Follows [T182](2026-09-24-t182-removing-mixlab-is-one-act-design.md) (D4, D6) and
[T182b](2026-09-25-t182b-a-helper-that-keeps-up-and-an-uninstall-that-finishes-design.md). Found on
2026-09-26, over seven uninstalls of one Windows machine: VS Code watched `<home>\bin`, Windows
refused the home's rename for as long as the watch was open, and the uninstall reached the end of
its work — the machine undone, a UAC prompt answered — before it could say so. The fix that shipped
lifts `bin/` out of the home before the rename; this generalises it.

## What is wrong

A complete uninstall renames every directory it removes to a tombstone first, all of them or none
(T182, D6), so a database is never left half deleted. On Windows a rename of a directory is refused
while another program holds anything *inside* it. What the other program holds decides whether
anything can be done about it:

| What another program holds, inside a directory that goes | Can that one thing be renamed and deleted? | Measured with |
| --- | --- | --- |
| a directory it **watches** for changes | **yes** — the watch shares delete, and follows the rename | VS Code on `bin`; a .NET `FileSystemWatcher` |
| a directory **shown in File Explorer** | **yes**, for the same reason | — |
| a file opened **sharing delete** (most logs, `std::fs::File::open`) | **yes** | a test holding a file |
| a directory that is its **working directory** (a terminal) | **no** — neither renamed nor deleted until it lets go | — |
| a file opened **without sharing delete** (SQLite, many editors and database tools) | **no** — neither renamed nor deleted until it lets go | — |

In the first three cases the one held thing can be moved out on its own, the way `bin/` already is,
and the directory above it is then free to go. Asking the person to close VS Code or a window of
File Explorer for that would be asking for something the uninstall can do itself.

In the last two nothing can be done by anybody but the program holding it — other uninstallers
leave such a file behind or schedule it for the next restart. Today MixEngine finds these only at
the very end, after the UAC prompt and after the machine has been undone.

## Goal

1. **Whatever can be moved is moved**, silently: every held directory or file that can be renamed
   goes out on its own before the directories holding it, and comes back with them on a refusal.
2. **Only what cannot be moved is asked about**, and it is asked about **before anything changes**:
   the program holding it is named on the uninstaller's checks page, with Retry.
3. On a machine where nothing is stuck — the common case — the uninstall runs straight through, with
   no Retry box.

**Not** a goal: closing programs for the person. The uninstaller never ends somebody's editor or
terminal.

## Decisions

### D1. Held, movable, stuck

A **held item** is a file or directory strictly inside a directory the uninstall removes, that
another process has a handle to (D2). Not a held item:

- one of those directories itself — renaming a directory that is itself open is allowed;
- anything held only by the asking daemon or a process it started (D3).

A held item is **movable** when it can be opened with `DELETE` access while sharing read, write and
delete — exactly the access a rename or a delete needs, refused with a sharing violation when any
existing handle does not share delete. The probe opens and closes a handle and changes nothing;
directories are opened with `FILE_FLAG_BACKUP_SEMANTICS`. Otherwise it is **stuck**.

The probe is the ground truth; the handle table (D2) only says *where to probe* and *whom to name*.
It is asked again at every step, because a program can open or close something at any moment.

On macOS and Linux nothing held open refuses a rename: there are no held items, and nothing here
changes on those systems.

### D2. Where to look: the system handle table

Windows has no documented call that answers "who holds anything under this directory". The Restart
Manager answers for files only; opening every entry with no sharing (today's `first_held`) says
*whether*, not *who*. The system handle table (`NtQuerySystemInformation`,
`SystemExtendedHandleInformation`) answers for files and directories alike, with the pid — it is
what Process Explorer and `handle.exe` read. Measured on this machine, unelevated, on Windows 11:
1.2–4.5 s for the whole system, and it found VS Code's watch on `bin` that nothing else did.

The walk lives in `mixengine-platform` (`windows/handles.rs`, used by `occupants`):

1. Read the table, growing the buffer on `STATUS_INFO_LENGTH_MISMATCH`.
2. Keep only handles whose object type is `File`, the type's index learned from a file this process
   holds open for the purpose, so pipes, events and the like are never touched.
3. For each, in a process other than the spared ones, duplicate the handle (a process that refuses
   `PROCESS_DUP_HANDLE` — another user's, SYSTEM's — is skipped), check `GetFileType` is
   `FILE_TYPE_DISK`, and name it with `GetFinalPathNameByHandleW`.
4. **Naming can hang** on a synchronous pipe with a read outstanding. The naming runs on a worker
   thread; one that does not answer in **250 ms** is abandoned and a fresh one carries on from the
   next handle; the whole walk has a **5 s** budget, and what was found by then is the answer.

`unsafe` is confined to that file, `#[expect(unsafe_code, reason = …)]` per call. `windows-sys`
carries the function (`Wdk_System_SystemInformation`, a feature, not a crate); the table's layout
is not in it and is declared there as a `#[repr(C)]` struct, with a test that it is 40 bytes on
64-bit Windows. Both are `host`-only, like `occupants`, so the privileged helper does not change.

### D3. Who is spared

The daemon asking, and everything descended from it — `occupants::processes_under`'s existing rule,
for its reason: its own shutdown stops them, in dependency order, before any rename. Today that is
its database, lock file and working directory, and the working directories of `caddy`, `php-cgi`
and the other services, all measured holding the home.

### D4. Before anything changes: only the stuck are rows

`daemon.uninstall_plan`, and `daemon.uninstall` before it touches anything, look for held items over
the directories the choices made will remove — the home, the relocated directories, the window's
folders — and probe each (D1).

- A **movable** item is not mentioned: it will be moved (D5).
- A **stuck** item is a row, one per holding process, as T182's D4 rows already are:
  `id: InUse`, `outcome: Blocked`, `what: "Code.exe (pid 1200)"`, `location:` the first stuck path,
  `by: "Code.exe has C:\…\MixEngine\data open, so it cannot be moved or deleted (and 2 more); close it and run the uninstall again"`.
  A process both running from the home and holding inside it is one row, the running one.

`mix uninstall --dry-run` then exits `3`, as for a running program, and the uninstaller offers Retry
with the list; the act itself refuses with `PreconditionFailed` and changes nothing. The row types
exist; the only wire change is one optional field, `UninstallQuery.skip_holders` (`serde(default)`),
which `mix` sets for `--dry-run --relocated` — the listing `un.onInit` reads while a banner is up,
which only names folders and should not pay for a scan.

### D5. At the rename: move out what can move

`remove_all_or_nothing` already takes `lifted` directories — today only `bin/` — and moves each out
beside the directory holding it before that directory is renamed, putting it back on a refusal.
This widens it:

1. The renames are tried as today, with `bin/` lifted. **On a machine where nothing else is held
   this is the whole of it**, and no scan is paid for.
2. If a rename is refused, everything is put back as today, and the daemon looks for held items
   (D2) under the directories it is removing and probes them (D1).
3. Every movable item joins `lifted`, **deepest first**, so a held file is out of a held directory
   before that directory moves. The renames are tried again, with T182b's 10 s retry.
4. A stuck item, or a holder that appeared after the checks, refuses as today: everything is put
   back, and the message names what is held and, when the table knew, by whom.

A lifted item's tombstone is `<directory>.removing-<pid>-<n>-<name>` beside the directory it came
from, `<n>` its order, so two lifted `logs` cannot collide and `tombstones_beside` finds every one of
them with the directory's own; `bin/` is numbered like the rest (it was `<home>.removing-<pid>-bin`
in d880937d, which no release shipped). Deleting a moved item
succeeds while the other program still holds it — it shared delete, which is what made it movable —
and a tombstone that cannot be deleted is found and removed by the next uninstall, as T182's D6
already provides.

### D6. The uninstaller

*Amended during implementation, at the person's request:* what is in the way gets **a page of its
own**, between the choices and the progress page.

- `mix uninstall --dry-run --blocked` prints only the `Blocked` rows, one program per line —
  `<what>: <location>`, ASCII, since `nsExec` misreads anything else — and succeeds whether or not
  it prints anything, like `--relocated`.
- The page runs it behind a banner, with the choices made. **Nothing listed skips the page**, so the
  ordinary uninstall goes from the choices straight to the progress page. Otherwise it lists them,
  asks for them to be closed, and offers **Check again**, which asks once more and rewrites the list.
  Uninstall stays available either way.
- `un.Checks` still turns exit `3` into a Retry box, for what appeared between the page and the
  click. Its sentence becomes *"Some programs are using MixLab's folders in a way that stops them
  being removed. Close them, then click Retry."*

## What this does not do

- **Close anything.** Restart Manager can ask a program to exit; an uninstall that ends somebody's
  unsaved editor is worse than one that asks.
- **Name another user's or SYSTEM's program.** Its handles are not in the table this process can
  read. What it holds is met by D5's retry and, failing that, the message.
- **Change macOS or Linux** (D1).

## Verification

- `mixengine-platform`, Windows:
  - the handle table finds a file and a directory this test process holds;
  - a watched directory and a file held sharing delete are movable; a file held without sharing
    delete, and a directory a child process stands in, are stuck;
  - an armed directory held itself is not a held item; the spared process and its child are not
    holders;
  - one process holding three stuck files is one holder with a count; a root spelled in upper case
    still matches; `home-dev` beside `home` is not inside it; a zero budget returns at once.
- `tombstone`: several lifted items, one nested in another, move out deepest first and come back on
  a refusal; their tombstones are found by `tombstones_beside`.
- `mixengine-cli` tests, Windows: a child process standing in `<home>\data` makes
  `uninstall --dry-run` exit `3` naming it; a directory this test only watches does not.
- By hand, `packaging/windows/uninstall-check.md`:
  - VS Code open, and File Explorer inside `<home>\etc` — the uninstall finishes with no Retry box;
  - a terminal in `<home>\data` — the checks page names it before any UAC prompt, Retry after
    closing it goes on and finishes.
