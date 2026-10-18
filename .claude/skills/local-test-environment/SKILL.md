---
name: local-test-environment
description: Use when cargo build or cargo test fails on a developer machine in a way the code does not explain — "Access is denied (os error 5)", WSAEACCES (10013) on a port, a CLI suite naming a package the code supports, "fakeservice is not there", a run in WSL that never finishes — and before running mix or mixengined by hand.
---

# When the machine is the one failing

Several suites here answer for the machine as much as for the code: a port somebody holds, a
locked executable, a keyring that does not exist. Before hunting for a cause in the code, rule out
the machine. The quickest test is the clean tree: `git stash -u`, run the same command on master,
`git stash pop`. Red on a clean master is not your change.

## Signatures

| You see | It is | Do |
| --- | --- | --- |
| `failed to remove file ...\target\debug\mixengined.exe` · `Access is denied. (os error 5)` | A daemon from a hand run is still alive; Windows locks a running executable. `caddy.exe` supervised from `target\debug` does the same | `Get-Process mixengined,caddy -ErrorAction SilentlyContinue \| Stop-Process -Force`. `mix daemon stop` is not enough when `mix` itself cannot be rebuilt |
| A `crates/mixengine-cli/tests/` suite says `this build of MixEngine cannot run <package>` right after a change to core | `cargo test -p mixengine-cli --test <suite>` does not rebuild `mixengine-daemon`; the harness starts the old one | `cargo build -p mixengine-daemon -p mixengine-cli` first. `cargo test --workspace` builds everything and is not affected |
| `dns::server` tests panic with `an ephemeral port is free for both transports` · `os error 10013` | Hyper-V/WSL reserves much of the dynamic port range (`netsh int ipv4 show excludedportrange protocol=udp`) | A property of the machine; let CI answer for the suite |
| `<target>/debug/fakeservice is not there — supervision tests drive a real process`, on every supervision test of the run within milliseconds | `cargo test -p <one crate>` builds that crate's targets only, and `fakeservice` is a binary of `mixengine-testkit`. A target directory that never ran `--workspace` — WSL's `/tmp/mixtarget`, any new `CARGO_TARGET_DIR` — has none | `cargo build -p mixengine-testkit --bins` once per target directory, then the named suite again. Not the code |
| `cargo test --workspace` in WSL never ends, and later runs print nothing — or a "Unlock Login Keyring" dialog appears on the desktop | `mixengine-platform/tests/secrets.rs` uses the real secret service. With none, it waits forever and the stuck `cargo` holds the target directory's lock. Where WSLg runs a gnome-keyring, the keyring asks the person to unlock it. A plain `cargo test -p mixengine-platform` runs this suite too | In WSL run named suites only (`-p <crate> --lib`, `--bins` or `--test <suite>`), and never `secrets`. Before trusting a "slow" run, `ps -eo pid,etime,cmd \| grep cargo` and kill the old ones |
| Red only on the Windows CI leg, green here, and it involves a token, a DACL or a privilege | CI's Windows runner has a full administrator token; a normal shell here has a UAC-filtered one | Reproduce from an elevated shell (below) rather than pushing probes |

## Running MixEngine by hand

- **Use a home of your own.** `--home <dir>` or `MIXENGINE_HOME=<dir>`, pointing at a scratch
  directory. The default home may hold a database from an older build that today's daemon refuses
  (*"migration 1 was previously applied but has been modified"*). Do not delete or migrate it.
- **Port 80 may already be taken** — on a machine with WSL, `wslrelay` holds `127.0.0.1:80` and
  `::1:80`, and Caddy cannot start. To check one rendered directive, copy `etc/caddy` elsewhere,
  change `http_port`/`https_port`/`admin` in the copy, and run caddy on that file directly.
- **Stop what you started** before the next cargo command (first row of the table).

## A test may not assume a port is free

No test asserts that a service gets port *N*, or that a second instance gets the first one's port
plus one: allocation asks the machine by binding, so the answer is whatever the machine holds at
that moment. At the end-to-end level assert only that instances get **different** ports, all
inside `[preferred, preferred + SEARCH]`, and that an instance which did not get its preferred port
is told so. "Exactly the next port" belongs only where the test owns the range —
`mixengine_core::services::ports`, `a_free_run`.

To reproduce a port collision here, hold the port while the test runs:

```powershell
$l = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 41000); $l.Start(); cargo test ...; $l.Stop()
```

Bind probes to loopback, never `0.0.0.0` — a wildcard bind makes Windows Firewall ask once per
test binary.

## An elevated run, with its output kept

```powershell
$out = "<scratchpad>\out.txt"
$inner = "cd '<repo>'; cargo test -p <crate> -- --nocapture *>> '$out'"
Start-Process -Verb RunAs -FilePath "powershell.exe" -ArgumentList @("-NoProfile","-Command",$inner) -Wait
```

The file is UTF-16LE (`iconv -f UTF-16LE -t UTF-8`). Every run is a UAC prompt the person has to
click: say so first, and gather everything to measure into one run.
