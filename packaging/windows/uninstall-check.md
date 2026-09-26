# Checking the Windows uninstaller by hand

Roadmap task T182, design
[docs/specs/2026-09-24-t182-removing-mixlab-is-one-act-design.md](../../docs/specs/2026-09-24-t182-removing-mixlab-is-one-act-design.md).

The uninstaller undoes real machine state: the hosts block, the certificate authority, the NRPT
rule, the port grant. Nothing in CI can click a UAC prompt, so these cases are walked by a person,
on a machine or VM whose MixLab setup they are prepared to lose.

Build the setup with `bash packaging/windows/build.sh`, install it, and set up at least one `.test`
site so there is something outside the home to undo. Then:

1. **The icon.** Installed apps shows MixLab with the MixLab icon, and so do the setup and
   `uninstall.exe`.
2. **The window is open.** Start Uninstall with MixLab running. The choices page offers *Also delete
   MixLab's data*, unticked. Clicking Uninstall asks to close MixLab; OK closes it and continues,
   Cancel leaves everything as it was.
3. **UAC declined.** Decline the prompt. The uninstaller says MixLab is still installed.
   `mix uninstall --dry-run` still lists every machine row as `would`, and
   `%LOCALAPPDATA%\Programs\MixEngine` is intact.
4. **A program in the way.** Start `php -S 127.0.0.1:8099` through the shim in a terminal, then
   Uninstall with the data box ticked. The page names `php` and stays open; nothing changed. Close
   it and click Uninstall again: it goes through.
5. **A finished run.** Nothing is left under `%LOCALAPPDATA%\Programs\MixEngine`; no `MixLab.lnk`
   in the Start menu; no `HKCU\Software\Classes\mixlab`; `%LOCALAPPDATA%\MixEngine` is gone when the
   box was ticked and there when it was not; no `MixEngine.removing-*` beside it.
6. **A relocated folder.** With `[paths]` `logs = "D:\\mixlogs"` in `config.toml` before the first
   start, the second box appears and lists `D:\mixlogs`. Each of the four combinations removes
   exactly what the design's D2 table says.
7. **Silent.** `uninstall.exe /S` with MixLab open closes it, keeps both kinds of data, and exits 0.
8. **Credentials** (T182d). Sign in to sync and save a database connection with a password.
   Uninstall with the data box **unticked**: `cmdkey /list | findstr /i "MixLab mixengine"` still
   lists them, and a reinstall is still signed in. Uninstall again with it **ticked**: the same
   command lists nothing of this home's, and a reinstall's Sync screen is signed out.
8. **An update over a running copy.** Running a newer setup while MixLab and the daemon are up asks,
   closes both, and installs without an "error opening file for writing" dialog.

## T182b

Design [docs/specs/2026-09-25-t182b-a-helper-that-keeps-up-and-an-uninstall-that-finishes-design.md](../../docs/specs/2026-09-25-t182b-a-helper-that-keeps-up-and-an-uninstall-that-finishes-design.md).

9. **An old helper.** On a machine whose `C:\Program Files\MixEngine\mixengine-elevate.exe` is older
   than `HELPER_VERSION` (the stray `0.1.0` is one), the first launch after installing asks once,
   and afterwards `mix elevation status --json` reports `installed_helper.version` as
   `HELPER_VERSION`.
10. **An update that did not change the helper** asks nothing.
11. **A kept home with sites.** Uninstall with the data box unticked: afterwards there is no NRPT rule
    (`Get-DnsClientNrptRule`), no `MixEngine Local CA` of this home in `LocalMachine\Root`, no
    MixEngine block in the hosts file, and no `mixengined` process.
12. **The uninstaller opens on a banner**, *Checking MixLab's folders…*, and then on a page whose
    boxes are already drawn when it appears.
13. **The page comes to the front.** Once the banner closes, the uninstaller's window is in front
    and has the focus; nothing has to be found on the taskbar.
14. **Uninstall answers at once.** Pressing Uninstall moves straight to the progress page, which
    reads *Checking what is in use.* with the bar moving. With a file of the install held open (a
    terminal in `%LOCALAPPDATA%\Programs\MixEngine` running `mix status --watch`), a Retry/Cancel box
    names it; closing the terminal and pressing Retry goes on, and Cancel removes nothing.
15. **A removal that takes a while shows it.** With the data box ticked on a home of a gigabyte or
    more, the bar runs as a marquee and the log reads *removing … folders (… MiB), this can take a
    minute*, then *still removing, …s so far* every ten seconds.
16. **The window's own folders.** `%LOCALAPPDATA%\io.github.mixnz.mixlab` is gone afterwards whatever
    was ticked; `%APPDATA%\io.github.mixnz.mixlab` is gone only when the data box was ticked.
17. **The desktop icon goes with it**, without refreshing the desktop.
18. **A home something else is standing in.** With a terminal whose current directory is inside
    `%LOCALAPPDATA%\MixEngine` opened *after* the checks page (so the checks could not name it) and
    the data box ticked, the uninstall stops, the log names the folder as open in another program,
    nothing of the home is removed, and after closing the terminal a second run finishes. (File
    Explorer no longer stops it: since T182e a folder Explorer shows is moved out, item 19.)

## T182e

Design [docs/specs/2026-09-26-t182e-an-uninstall-moves-what-it-can-design.md](../../docs/specs/2026-09-26-t182e-an-uninstall-moves-what-it-can-design.md).

19. **What can move is moved.** VS Code open (it watches `bin` on `PATH`) and File Explorer open
    inside `%LOCALAPPDATA%\MixEngine\etc`. Tick the data box: no Retry box appears, the uninstall
    finishes, and neither folder is left.
20. **What cannot move is named first, on a page of its own.** A terminal whose current directory is
    `%LOCALAPPDATA%\MixEngine\data`. Tick the data box and click Next: a page lists the terminal's
    program, pid and folder, before any UAC prompt. Close it and click **Check again**: the list
    reads *Nothing is in the way now*. Click Uninstall: it goes on and finishes.
21. **Something opened after the page is still caught.** On that page with the list clear, open the
    terminal in `data` again, then click Uninstall: a Retry/Cancel box names it before any UAC
    prompt. Close it and press Retry: the uninstall goes on and finishes.
22. **Nothing in the way, no page.** With nothing open in MixLab's folders, the choices page's Next
    goes straight to the progress page.
23. **An updated install leaves nothing.** On an install that has updated itself once (so
    `%LOCALAPPDATA%\Programs\MixEngine\update.lock` exists), a complete uninstall leaves no
    `Programs\MixEngine` folder, and its log ends with *Remove folder*. The log shows the firewall row
    as `MixEngine - shared sites`, with a plain dash.
