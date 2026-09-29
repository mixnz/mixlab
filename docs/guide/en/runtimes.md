+++
title = "PHP, Node, Python, Ruby, Go and Java versions"
slug = "runtimes"
order = 5
summary = "Install as many versions as you need, and let each directory choose its own, with no shell hook and nothing to remember."
+++

# PHP, Node, Python, Ruby, Go and Java versions

> **This handbook covers MixLab through the `mix` command line.** If you would rather work in a
> graphical interface, you already have one: every installer places the MixLab window beside the
> command line. Both drive the same MixEngine underneath, so everything in this handbook still
> applies.

MixLab installs language runtimes into its own directory, one immutable folder per version, and
never touches whatever your operating system already has. Installing a version never modifies a
version already installed, so nothing you have working can be broken by adding something new.

Six languages are managed: **PHP**, **Node.js**, **Python**, **Ruby**, **Go** and **Java**, plus one
tool, **Composer**, which installs the same way and runs under whichever PHP the directory uses.

## Installing a version

```bash
mix runtime available --kind php
mix runtime install php 8.3.33
mix runtime list
```

The version is exact, on purpose. `8.3` asks *"choose one for me"*, and there is nothing to choose
from until something is installed. Choosing between versions is what resolution does, and resolution
answers with what is on the machine. `mix runtime available` is where a range belongs.

An install is a job, and `mix` waits for it by default: `mix runtime install php 8.3.33 && …` is a
sentence about PHP being there. `--no-wait` returns as soon as the daemon has accepted the work and
hands you a job id, which `mix job wait` can be pointed at later.

**Installing a PHP also creates its php-fpm pool**, such as `php-fpm@8.3.33`: a service like any
other, in `mix service list`. Node, Python, Ruby, Go and Java are invoked per command and have
nothing supervised.

### On a Windows PC with an ARM processor

Some versions have no build made for that processor. Nobody publishes an ARM64 Windows PHP, for
instance. Where that is so, MixLab installs the x86_64 build instead and Windows runs it for you.
It works; it is a little slower than a build made for your machine would be.

You are never left to guess which is which. `mix runtime available` and `mix package available` grow
a `RUNS` column on that machine, saying `native` or `emulated` per version, and the install says so
before it starts downloading. On every other machine the column is not there, because there is
nothing for it to say.

## Composer

```bash
mix runtime available --kind composer   # the versions the index offers
mix runtime install composer 2.10.3     # exact, like every install
composer --version                      # runs composer.phar under this directory's PHP
mix project update shop --pin composer=2.2
```

Composer is a file, not a program: the `composer` command starts the PHP your directory resolves to
and hands it `composer.phar`. So `MIXENGINE_PHP=8.1 composer install` uses PHP 8.1, and a directory
pinned to PHP 7.4 needs the 2.2 line, because Composer 2.3 and later want PHP 7.2.5 or newer.

| Your PHP | Pin |
| --- | --- |
| 7.2.5 and newer | `composer = "2"` |
| 5.3 – 7.2.4 | `composer = "2.2"` |

Installing Composer creates no service and runs nothing; `mix runtime list` shows it beside the
languages.

## Go

```bash
mix runtime available --kind go        # 1.21 to the newest release
mix runtime install go 1.25.14
go version                             # the Go this directory resolves to
mix project update api --pin go=1.25
```

`go` and `gofmt` are commands like every other. `GOROOT` is worked out by `go` itself from where it
is installed, and `GOPATH`, the module cache and the build cache stay where Go puts them. Every
version shares them, which is how Go is designed to be used.

**A pinned Go is the Go that builds.** A `go.mod` asking for a newer release than the one your
directory resolves to does not quietly download that release and run it instead: a `go` started
through MixLab runs with `GOTOOLCHAIN=local`, so such a module stops with Go's own message.

```text
go: go.mod requires go >= 1.27 (running go 1.25.14; GOTOOLCHAIN=local)
```

The answer is to install the newer Go and pin it. Three details:

- A `GOTOOLCHAIN` you export yourself is left exactly as you wrote it.
- An empty one counts as not set, the way Go reads it.
- One you saved with `go env -w` is overridden: the pin wins over a setting that applies to the
  whole machine.

`mix doctor` tells you when MixEngine itself was started with a `GOTOOLCHAIN` other than `local`, or
with a `GOROOT`, because the commands it starts inherit them.

Programs you add with `go install` land in Go's own `GOBIN` (`~/go/bin` unless you changed it),
which MixLab does not put on your `PATH`.

## Java

```bash
mix runtime available --kind java      # the long-term-support lines: 11, 17, 21 and 25
mix runtime install java 21
java --version                         # the JDK this directory resolves to
mix project update api --pin java=21
```

`java`, `javac`, `jar`, `jshell`, `keytool` and `jlink` are commands like every other. Each is
started with **`JAVA_HOME` set to the JDK it belongs to**, even if you have exported another one, so
a program, and any build it runs itself, finds the JDK this directory asked for.

**Maven and Gradle typed in a terminal read your own `JAVA_HOME` first.** If yours points at a
system JDK, `mvn` and `./gradlew` use that one whatever the directory pins; unset it and they find
the pinned `java` on your `PATH` instead. `mix doctor` tells you when MixEngine itself was started
with a `JAVA_HOME` outside its own JDKs.

**HTTPS to your own sites works.** MixLab writes its certificate authority into each installed
JDK's certificate store, so `https://blog.test` verifies from Java with no flag and no extra
argument. Two limits are worth knowing: a runtime you build yourself with `jlink` carries the
original store and does not trust these sites, and a JVM started with `-Djavax.net.ssl.trustStore`
reads that store instead of the JDK's own. If a JDK has lost it, `mix doctor --repair` puts it back.

**On Linux a JDK expects some of the system's libraries**: `zlib` to start at all, `freetype` for
fonts, X11 for windows and ALSA for sound. When your system does not have one, the install says
which and carries on: a server that never draws a window or plays a sound runs without them, and
your distribution's package manager has them when you need them.

## Choosing which one a directory uses

Nothing here changes a shell, patches a profile, or asks you to type an activate command. A
directory resolves to a version, and the shims do the rest.

```bash
mix runtime default php 8.3.33      # the machine-wide fallback
mix project update blog --pin php=^8.1
mix runtime resolve php             # what does *this* directory get, and why?
```

`mix runtime resolve` is the command to remember. It answers what `php -v` would answer, without
running anything, **and** it names which of four sources decided it:

1. An explicit flag or environment variable on the command being run.
2. The nearest `mixengine.toml` that names this language, walking up from where you are.
3. The registered project covering this directory.
4. The global default.

A `mixengine.toml` that says nothing about PHP is not an answer about PHP, so a pin further up still
applies.

### Writing a constraint

Pins and `--version` accept three shapes, all resolved against **installed** versions and never
silently against downloadable ones:

| Written | Means |
| --- | --- |
| `8.3.33` | Exactly that |
| `8.3` or `8` | As many segments as are written have to agree; one nobody wrote is a zero |
| `^8.3` | Up to the leftmost non-zero segment: `^0.12` stops before `0.13` |

A constraint with no pre-release in it never selects one. `8.5` and `^8.5` both pass over
`8.5.0RC1`; naming it exactly is how you ask for it.

## The shims

`mix path install` puts `<root>/bin` on your `PATH`. It holds a small program per command of each
language you have installed with MixEngine (`php`, `composer`, `node`, `npm`, `python`, `pip`,
`ruby`, `go`, `java` and the rest), and each one works out which version this directory wants and
hands over to the real binary.

Three things follow that are worth knowing:

- **It works with the daemon stopped.** A shim reads what it needs directly rather than asking over
  a socket, which is why `php -v` in a project still answers when MixEngine is not running.
- **Only what you installed is there.** Installing the first Node.js puts `node`, `npm` and `npx`
  there; removing the last one takes them away. A language you never installed with MixEngine has
  no command in `<root>/bin`, so `which node` finds the Node.js you installed yourself.
- **A terminal open from before may remember an old path.** After a command appears or goes, open a
  new terminal, or run `hash -r` in bash.

Only `<root>/bin` goes on your `PATH`: one entry, never a directory per version.

```bash
mix path status
mix path uninstall
```

`mix path uninstall` takes the directory back off your `PATH` and leaves the commands where they
are: they live inside MixEngine's own home, and removing the home is what removes them.

## PHP extensions

Extensions are per installed version, because that is what they are compiled against:

```bash
mix runtime ext list --php 8.3.33
mix runtime ext enable redis --php 8.3.33
mix runtime ext disable xdebug --php 8.3.33
```

`list` says which extensions the build has and **why each one is on or off**, which is usually the
question. Leaving `--php` out means the version this directory resolves to.

Enabling loads the extension on every PHP process of that version, the pool included.

## Removing a version

```bash
mix runtime uninstall php 8.1.31
```

This is refused while a registered project pins that version, naming the projects, and while the
php-fpm pool running out of it is running. `--force` crosses the first of those and never the
second.
