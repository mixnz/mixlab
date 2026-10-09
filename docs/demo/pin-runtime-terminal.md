# Pin a runtime per project: real terminal output

Recorded for the website's `#mixengine` demo, beside the `pin-runtime` clip. Everything below was
typed on a real machine against a real `mixengined`, real PHP builds from the package index and the
real `php` shim in `<home>/bin`. No fixture, and no output edited by hand.

## Where it ran

| | |
| --- | --- |
| Date | 2026-10-10 |
| OS | Windows 11 Pro 10.0.26200 (x86_64) |
| Shell | Git Bash — GNU bash 5.3.15(1)-release, MINGW64_NT-10.0-26200 |
| MixEngine | 0.0.17 development build, commit `f58954ab` |
| PHP | 8.4.26 (default), 8.1.34 |

The run used a home of its own (`MIXENGINE_HOME` pointing into a scratch directory), so the paths in
the setup output are long scratch paths. The `Sites` folder was
`<scratch>/pin-demo/ada/Sites`; the session could not change `HOME`, so every command written below
with `~/Sites` — the two `mix project create` and the first `cd` — was typed with that absolute path
instead. `php -v` prints no path, so its output is unaffected.

## Setup

```
mix runtime install php 8.4.26
mix runtime install php 8.1.34
mix runtime default php 8.4.26
mix project create ~/Sites/blog --name blog
mix project create ~/Sites/legacy --name legacy
```

`blog` has no pin and follows the default.

## The pin MixLab does, as a command

MixLab's project form saves a pin through `project.update`. In the `pin-runtime` clip, `legacy`'s
PHP is set to `8.1.34`, picked from the installed versions. The same call from `mix`:

```
mix project update legacy --pin php=8.1.34
```

What it printed:

```
legacy
  root      C:/Users/haiqu/AppData/Local/Temp/claude/C--Users-haiqu-Developer-mixnz-mixlab/14b96bc9-1b42-4961-b6a7-e2ed3ca0996a/scratchpad/pin-demo/ada/Sites/legacy
  created   2026-10-09T18:43:04Z

RUNTIME   PINNED      RESOLVES    FROM
php       8.1.34      8.1.34      this home
```

`--pin` replaces every pin the project had. A pin can also name a line rather than a release —
`--pin php=8.1` resolves to the newest 8.1 installed, 8.1.34 here, and printed `php  8.1  8.1.34
this home` on the same machine. A project can also be pinned when it is registered:
`mix project create ~/Sites/legacy --name legacy --pin php=8.1.34`.

## The sequence

```
cd ~/Sites && php -v
cd legacy && php -v
cd ../blog && php -v
```

Output, verbatim (all three exited 0):

```
PHP 8.4.26 (cli) (built: Sep 22 2026 15:11:32) (NTS Visual C++ 2022 x64)
Copyright (c) The PHP Group
Built by The PHP Group
Zend Engine v4.4.26, Copyright (c) Zend Technologies
    with Zend OPcache v8.4.26, Copyright (c), by Zend Technologies
PHP 8.1.34 (cli) (built: Dec 16 2025 18:39:53) (NTS Visual C++ 2019 x64)
Copyright (c) The PHP Group
Zend Engine v4.1.34, Copyright (c) Zend Technologies
    with Zend OPcache v8.1.34, Copyright (c), by Zend Technologies
PHP 8.4.26 (cli) (built: Sep 22 2026 15:11:32) (NTS Visual C++ 2022 x64)
Copyright (c) The PHP Group
Built by The PHP Group
Zend Engine v4.4.26, Copyright (c) Zend Technologies
    with Zend OPcache v8.4.26, Copyright (c), by Zend Technologies
```

The banner is the Windows build's: `NTS Visual C++ …` in the first line, and 8.4 adds a
`Built by The PHP Group` line that 8.1 does not print. A macOS or Linux build prints a different
first line.
