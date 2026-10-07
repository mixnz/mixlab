# Checking the macOS removal by hand

Roadmap task T182a, design
[docs/specs/2026-10-07-t182a-an-uninstall-path-for-macos-design.md](../../docs/specs/2026-10-07-t182a-an-uninstall-path-for-macos-design.md).

The removal undoes real machine state as root: the resolver files, the packet filter and its boot
job, the certificate authority, the helper, the program's own files and the package receipt.
Nothing in CI can answer the administrator prompt, so these cases are walked by a person, on a Mac
whose MixLab setup they are prepared to lose.

Build the package with `bash packaging/macos/build.sh` (`MIX_MACOS_SLICES=aarch64` on Apple
silicon for speed), install it with `sudo installer -pkg <file> -target /`, start MixEngine, and
set up at least one `.test` site with a running service, so there is something outside the home to
undo. Then:

1. **The item.** The application menu holds **Remove MixLab from this Mac…** above Quit, with a
   separator of its own. A development build has no such item.
2. **A program in the way.** Run `node` from `<home>/bin/node` in a terminal, then open the dialog.
   It names `node`, its pid and the folder, and **Remove MixLab** is disabled. Close `node`, press
   **Check again**: the row goes and the button is enabled. Nothing changed meanwhile.
3. **The prompt declined.** Press **Remove MixLab** and decline the password prompt. The dialog says
   nothing was removed; `pkgutil --pkg-info dev.mixengine.cli` still answers; the item is still in
   the menu; `mix uninstall --dry-run --package` still lists every machine row as `would`.
4. **A finished run.** Press **Remove MixLab** again and allow the prompt. MixLab quits. Then:
   - `ls /usr/local/bin | grep mix` prints nothing;
   - `pkgutil --pkg-info dev.mixengine.cli` answers *No receipt*;
   - `grep -c mixengine /etc/hosts` is 0;
   - `ls /etc/resolver` no longer holds MixEngine's `test`, `localhost` or `internal`;
   - `security find-certificate -a -c "MixEngine Local CA" /Library/Keychains/System.keychain`
     holds one authority fewer (other homes' are T182c's);
   - `ls /Library/PrivilegedHelperTools /Library/LaunchDaemons | grep -i mixengine` prints nothing;
   - `ls ~/Library/LaunchAgents | grep -i mix` prints nothing;
   - `~/Library/Application Support/MixEngine` is gone when the data box was ticked and there when
     it was not.
5. **From a terminal.** Install the package again and start MixEngine. Over SSH,
   `mix uninstall --dry-run --package` shows the `package` row as `would`, and `mix uninstall`
   without the flag shows it as `kept`, naming `mix uninstall --package`.

## Walked 2026-10-07

Mac14,3, macOS 15.7.3, a `.pkg` built from the T182a branch over the released v0.0.15. Every case
above held; what the walk found and fixed before it was ticked is recorded in the design's
*Checked by hand* section: a declined prompt read as a failure, the MixEngine tab's prompt dialog
drawn over the removal, and four `.lock` files left in `/Library/Logs/MixEngine`.
