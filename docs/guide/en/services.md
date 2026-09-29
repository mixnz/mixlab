+++
title = "Servers, databases and caches"
slug = "services"
order = 6
summary = "Caddy or Nginx, MariaDB, MySQL, PostgreSQL, Redis and Memcached — installed on request, configured for you, and never printing a password."
+++

# Servers, databases and caches

> **This handbook covers MixLab through the `mix` command line.** If you would rather work in a
> graphical interface, you already have one: every installer places the MixLab window beside the
> command line. Both drive the same MixEngine underneath, so everything in this handbook still
> applies.

Two words, kept apart the way MixLab keeps them apart.

A **package** is a program MixLab knows how to run — Caddy, MariaDB, Redis. Installing one puts a
copy of it in MixLab's own directory and does nothing else.

A **service** is a running instance of a package: a port, a data directory, a generated
configuration, a log, and a state. `mariadb@main` and `mariadb@legacy` are two services of one
package, with different ports, different data and possibly different versions.

## What is on offer

| Service | Default line | Default port |
| --- | --- | --- |
| Caddy | 2.x | 80 and 443 — the default front end |
| Nginx | 1.27 | 80 and 443 — the alternative, one front end at a time |
| php-fpm | one per installed PHP | a socket, or a local port on Windows |
| MariaDB | 11.4 LTS | 3306 |
| MySQL | 8.4 LTS | 3306 — a different product from MariaDB, not a version of it |
| PostgreSQL | 16 | 5432 |
| Redis | 7.x | 6379 |
| Memcached | 1.6 | 11211 |
| MongoDB | 8.x | 27017 — no accounts, so it only ever listens on loopback |

**Nothing arrives by itself.** A fresh MixLab has no web server until you install one, and
"default" above means *the one this project recommends when there is a choice*, not *the one that is
already there*.

## Installing and creating

```bash
mix package available
mix package install mariadb 12.3.2
mix service create mariadb@main 12.3.2
```

The part of a service id before the `@` is the package it is an instance of, which is why
`mix service create` needs no separate argument for it. The part after the `@` is yours: it is what
tells two of them apart, and MixEngine attaches no meaning to the words. Caddy runs once for a whole
MixEngine home, so its service is simply `caddy` with no `@` at all.

The name does not have to say which version a service runs. `mix service list` shows the version
beside each id, and so does MixLab, so `mysql@main` is enough.

The id cannot be changed afterwards — it is also the generated configuration directory, the log
directory, the socket and the address the password is stored at — so renaming one means creating the
other and deleting this one, which keeps the data.

Useful flags on `mix service create`:

| Flag | What it does |
| --- | --- |
| `--port` | The port it listens on. The recipe's own default when left out |
| `--bind` | The address it binds. `127.0.0.1` when left out |
| `--data-dir` | Where its data lives. A directory under the home when left out |
| `--autostart` | Start it whenever the daemon starts |

### Who gets 3306

MariaDB and MySQL want the same port, and so do two instances of either. The rule is one rule:
**first created, first served**. The first to ask for 3306 gets it; the next gets the first free
port above. MixLab reports the port it chose, because a port you did not pick is one you have to
be told about.

A port you name explicitly is taken at your word, with no allocation at all.

### One data directory, one service

`mix service create` refuses a `--data-dir` another service already holds, and names who holds it.
Two servers over one set of files corrupt them, and that cost lands on your data rather than on a
start that fails.

## Running them

```bash
mix service list
mix service status mariadb@main
mix service start mariadb@main
mix service stop mariadb@main
mix service logs mariadb@main --follow
```

`mix service status` requires an id where `start` and the rest take an optional one: a status with
no subject is a `list` typed wrongly, and answering it as a list would hide that.

Deleting a service takes the row and the configuration generated from it, and **never the data** —
that is somebody's databases. The answer names the directory that was left, so nobody has to go
looking:

```bash
mix service delete mariadb@legacy
```

### From the tray

MixLab has an icon in the tray (Windows), the menu bar (macOS) or the system tray (Linux), whichever
modules you use. With MixEngine on, a click on Windows and macOS slides a panel into the corner of
the screen, beside the taskbar or under the menu bar. On Linux a click opens a short menu, and
**Open control panel** opens the same panel as a small window, because Linux does not tell an
application about clicks on its icon. Without MixEngine there is no panel, and a click brings
MixLab's window back.

The panel's header has **Open MixLab** and a power button that quits MixLab. Below it, the MixEngine
card shows CPU and memory and ends with **Stop MixEngine**, which asks you to confirm first. Then
come your services with Start and Stop, **Stop all**, and your sites (a click opens one).

On Windows and macOS a right click on the icon opens **Open MixLab** and **Quit MixLab**.

Closing MixLab's window only hides it, so terminal sessions and database connections stay open. To
quit, use the icon's menu, the power button in the panel, or ⌘Q on macOS. Quitting MixLab leaves
MixEngine running.

To have the icon after you log in, turn on **Open MixLab in the tray when I log in** in **Settings →
General**. This is a separate switch from **Start MixEngine when I log in** (`mix autostart`), and
you can turn on either one or both.

On GNOME the icon needs the *AppIndicator and KStatusNotifierItem Support* extension. Ubuntu
includes it; Fedora and plain GNOME do not. Without it there is no icon, closing the window quits
MixLab as before, and the login switch opens the window instead.

## Which web server your sites go through

One of Caddy and Nginx is your front end at a time: every site in the home is reached through it,
and `mix service front-end` says which one it is.

```bash
mix service front-end
```

Changing it is one command, and it is a real operation rather than a setting: the server you are on
is stopped, its row goes, every site is rendered for the new one, and that one is started.

```bash
mix package install nginx 1.27.3
mix service set-front-end nginx
```

**No site is reachable while that happens**, so `mix` tells you what is about to happen and asks
before it starts. Pass `--yes` in a script.

**On Linux the new server needs permission to answer on ports 80 and 443**, and that permission
belongs to the program rather than to MixLab — so moving to a different program means asking for
it again, and a prompt may appear. If nobody allows it, **nothing changes**: you stay on the server
you were on, MixLab says so, and `mix elevation grant` followed by the same command finishes the
job. macOS and Windows need no second permission.

Two things do not travel with the switch, and MixLab names them rather than dropping them
quietly: settings you had overridden — an `nginx.conf` setting means nothing to Caddy — and any
limits or idle policy you had set on the old server. The old server's data directory is left exactly
where it was.

## Databases and accounts

Making a database is one command, and it starts the server if it is not running:

```bash
mix database create mariadb@main --name blog
mix database create mariadb@main --name shop --user shop_app
```

**Nothing prints the password by default.** It is generated and put into your operating system's own
credential store — Credential Manager on Windows, the Keychain on macOS, the Secret Service on
Linux — and what is printed is the address it was stored at, as the store's own name and key. That
is what lets a client tell you *"stored in your credential store as …"* without anybody hardcoding
MixLab's naming.

When a project needs the password itself — most often for a `.env` file — `mix database
credentials` prints it, and `--password` on `create` lets you choose it instead of letting MixLab
generate one:

```bash
mix database credentials mariadb@main --user blog
mix database create mariadb@main --name shop --user shop-app --password
```

Without a value, `--password` prompts and reads one line from standard input, so it also works
piped: `echo secret | mix database create … --password`. Choosing a password for an account you
already made changes what is stored, and the server is realigned to it the same way it already is
when a password drifts — but an account already on the server that MixLab holds no credential for
is still refused, even with the correct password: knowing a password is not what makes it yours.

To open the database in a desktop client:

```bash
mix database client mariadb@main   # what is installed, and where this system looked
mix database open mariadb@main     # open it
```

`client` reads only: it starts nothing and opens nothing, and *"no client installed"* is an answer
rather than a failure — it names where MixLab looked and where to get one.

`open` starts the instance if it is stopped, reads the password from the credential store **at that
moment**, and hands it to the client in that process's own environment. It is never printed, never
put in an argument, and so never in your shell history.

## What a service may take, and when it stops

```bash
mix service limits mariadb@main
mix service limits mariadb@main set --memory 512 --cpu 50
mix service idle mariadb@main --after 30m
```

`limits` with no subcommand reads; `set` replaces; `clear` removes. **`set` replaces every field,
not only the ones you name** — `set --cpu 50` clears a memory ceiling that was there — so it prints
all three fields of the result, and a cleared limit is on your screen rather than a surprise. What
your operating system will actually enforce differs, and the answer says which of the two you have:
a **hard** ceiling is a wall — at it, the service is killed or its next allocation fails — while an
**advisory** one is a watched line the service may cross, after which MixLab warns and, where the
recipe permits, restarts. A control drawn as a guarantee when it is advisory would be a lie about
your data.

`idle` says when a service is stopped for being unused, and what is currently holding it open.
**Nothing is stopped for being idle unless you ask**: a site that is up stays up. To save battery,
turn on *Save battery* in MixLab's Settings, or run `mix service save-resources --on`; a PHP pool
nobody used for half an hour, or a database or cache for an hour, is then paused, and the next
request that needs it starts it again — that first load can take a second. `mix service idle`
gives one service its own time either way, and `--after 0` means never.

The web server itself is never stopped for being idle, and it starts with MixEngine.

## The generated configuration

MixLab writes the configuration for every service it runs, out of what it knows. Those files are
disposable — they are regenerated, never read back — so there is nothing there for you to edit and
nothing to keep in sync. If a setting you need has no flag, that is a gap in MixLab rather than
an invitation to edit the file.
