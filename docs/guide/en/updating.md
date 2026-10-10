+++
title = "Keeping MixLab current"
slug = "updating"
order = 12
summary = "Updates are opt-in, checked against a signature, and rehearsed before anything is replaced. One binary is deliberately never replaced this way."
+++

# Keeping MixLab current

> **This handbook covers MixLab through the `mix` command line.** If you would rather work in a
> graphical interface, you already have one: every installer places the MixLab window beside the
> command line. Both drive the same MixEngine underneath, so everything in this handbook still
> applies.

```bash
mix self-update --check
mix self-update
```

`--check` prints what is available (the version, the size, and what changed) and installs nothing.
Without it, the same information is shown and then you are asked.

## Updates are never silent

An update restarts the services you are running. That makes it a thing you choose, not a thing that
happens to you while you are working, so **nothing is ever installed without being asked**. The
daemon does check quietly, at start and once a day, so that `mix status` can tell you an
update exists, and both checks fail silently when there is no network.

`--yes` answers the question in advance, for a script with nobody at the keyboard.

## What happens when you say yes

In order, and none of the steps is optional:

1. The release is downloaded and hashed against the **signed** update feed. A payload that does not
   match is not unpacked.
2. The signature is checked against a public key compiled into MixEngine itself. Nothing about the
   transport is trusted to decide whether a file is ours.
3. **The new `mixengined` is run once**, before anything is replaced, to be sure this machine will
   start it. An update that would leave you with a daemon that does not run is stopped here rather
   than discovered afterwards.
4. What is running is stopped, the binaries are replaced, and the daemon exits.
5. `mix` starts the new daemon, which starts your services again.

## The privileged helper

`mixengine-elevate` runs as an administrator, so replacing it needs your permission. It has a
version of its own that moves only when the helper changes, so most updates leave it alone and ask
nothing.

When an update does change it, the new daemon asks for permission once, at its first start, and
the helper already installed checks MixLab's signature on its replacement before it lets itself be
overwritten. If you decline, nothing breaks: the old helper keeps serving everything it knows, and
the replacement rides along with the next prompt MixLab needs anyway.

## When you installed MixLab from the `.pkg` on a Mac

`mix self-update` still updates it, the way the `.pkg` installed it. It downloads the next `.pkg`,
checks it against the signed release, and opens it in Installer.app. Nothing stops while you go
through the installer, and cancelling it costs nothing.

When the installer is done, run `mix self-update --finish`, or press **Finish** in MixLab.
MixEngine restarts on the new version and starts the services that were running.

Over SSH, the installer opens on the Mac's own screen. `mix self-update` also prints the path of the
downloaded package and the `sudo installer` command that installs it from a terminal.

The first version that can do this has to be installed by hand once, like any `.pkg`.

## On Linux

A copy installed from the `.deb` or the `.rpm` is owned by `apt` or `dnf`, so MixLab does not
replace its files itself. `mix self-update` downloads the next package of the same kind, checks it
against the signed release, and prints the command that installs it:

```bash
sudo apt install '<the path it printed>/mixlab_0.0.19-1_amd64.deb'
```

On a desktop it also opens the package in your software centre. Once it is installed, finish with
`mix self-update --finish`.

The Windows installer is updated by `mix self-update` in place.

## PHP, Node.js and the servers you installed

This page is about MixLab itself. The versions of PHP, Node.js, databases and web servers you
installed are updated separately, one at a time and only when you ask. See
[Updating to a newer patch](./runtimes.md) and [Updating a server to a newer patch](./services.md).

## Versions

MixLab uses semantic versioning, one version across everything it ships. Before 1.0 the API may
break between minor versions, and each break is listed in the changelog. That is what
`mix self-update --check` prints before it asks.
