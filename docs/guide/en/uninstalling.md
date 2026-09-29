+++
title = "Removing MixLab"
slug = "uninstalling"
order = 13
summary = "Undo everything MixLab wrote outside its own directory, see the list before you agree, and keep your databases if you want them."
+++

# Removing MixLab

> **This handbook covers MixLab through the `mix` command line.** If you would rather work in a
> graphical interface, you already have one: every installer places the MixLab window beside the
> command line. Both drive the same MixEngine underneath, so everything in this handbook still
> applies.

MixLab writes almost everything inside one directory. The exceptions are the handful of
privileged changes it asked permission for, and taking those back is what `mix uninstall` is for.

## On Windows, Installed apps does all of it

Uninstall MixLab from Installed apps. The uninstaller asks two things, both unticked:

- **Also delete MixLab's data**: the home, with your databases, certificates and project records.
- **Also delete the folders you moved out of it**: shown only when `[paths]` moved `runtimes`,
  `packages`, `data` or `logs` somewhere else, and listing where.

If MixLab is open, it asks to close it. Then it checks everything that could stop it half-way: a
file in use, or a program running from one of MixLab's folders, such as a `php` you started through
a shim. If anything is in the way, it names it and removes nothing. Close it and click Uninstall
again.

Then one administrator prompt undoes the machine changes listed below, and the program goes. If you
decline the prompt, nothing is removed and MixLab stays installed. Run Uninstall again when you are
ready.

The rest of this page is the same work done with `mix`, which is how you do it on macOS and Linux.

## See the list first

```bash
mix uninstall --dry-run
```

That changes nothing and names every single thing it would remove:

- the hosts block, and the DNS or resolver rule that routes your names
- the grant to listen on port 80 and 443
- the certificate authority, from every store that trusts it
- any firewall rule left by a shared site
- the entry that starts the daemon when you log in
- the `PATH` entry
- the privileged helper, and its audit log
- the MixLab window's cache and logs, and, with the directory below, its saved connections and
  history
- the passwords your databases use, and MixLab's saved passwords and sync sign-in, from your
  system's password store. If you keep the directory below, they stay with it
- and finally MixLab's own directory

## Removing it

Quit MixLab first. If its window is open, it saves its passwords again after they are removed.

```bash
mix uninstall
```

You are asked to confirm, and one administrator prompt covers the privileged half. `--yes` answers
the confirmation in advance, for a script.

**Nothing changes until the prompt is allowed.** Decline it and your `PATH`, your login entry and
your browsers are exactly as they were; run the command again when you are ready.

**A program running from MixLab's folders stops it before it starts.** The list marks it
`BLOCKED`, and the command exits `3`, so a script can tell *close this and try again* from a
failure. Close the program and run the command again.

**The report is a measurement, not a claim.** What comes back is what MixLab found on the machine
*afterwards*, row by row, including the rows that answered *nothing there*. A report that hid those
would leave you unable to tell "there was no resolver wiring" from "the resolver wiring was not
looked at". The command exits non-zero if anything it acted on is still present, so a script can
ask.

Expect the connection to end partway: the daemon is removing the home it serves, so it stops itself.
That is the normal ending, and MixLab reads the final rows back off disk once it has, which is
what makes the answer *nothing is left behind* rather than *the daemon said so*.

## Keeping your data

```bash
mix uninstall --keep-home
```

This undoes everything **outside** the home directory and leaves the home where it is: your
databases in `data/`, your certificates, your projects' records. The daemon stops when it is done,
and a later install picks the home up again.

If you moved `runtimes`, `packages`, `data` or `logs` to another disk with `[paths]`, those folders
are a separate choice:

```bash
mix uninstall --keep-relocated
```

keeps them and removes the home. Use both flags to keep everything. A folder you never moved is
inside the home, so it goes with the home.

### Installing again over the folders you kept

Point the new install at the same folders when it asks where to keep them. When it starts:

- **Your projects, sites and services** come back from the copy the uninstall left in those
  folders: the Dashboard shows *Restore your earlier install*, or run `mix home restore`. Databases
  get a new admin password and stay stopped until you start them.

- **Runtimes and packages** already in them are listed as installed on their own. One MixLab could
  not check, for example because there was no network, shows under *On disk, not listed* in
  Runtimes with an **Adopt** button. From a terminal: `mix runtime found`, then
  `mix runtime adopt php 8.3.33` (and the same with `package`).
- **Databases** in `data/` show on the Dashboard as *Service data from an earlier install*. Open
  *Review* and press **Adopt**. The service comes back stopped, with a new admin password, and every
  database and account in it is kept. From a terminal: `mix service found`, then
  `mix service adopt mariadb@main`.

An application's own database account keeps the password the application already has. Only the
admin password is new, because the old one went with the home.

## Then remove the program itself

`mix uninstall` removes what MixLab did. Removing MixLab is your package manager's job, and it
depends on how you installed it:

```bash
sudo dpkg -r mixlab
sudo rpm -e mixlab
sudo rm -rf /usr/local/bin/mix /usr/local/bin/mixengined /usr/local/bin/mixengine-shim \
  /Applications/MixLab.app
```

On Windows, the installer's own uninstaller already did this part. On macOS, the third line above is
what the `.pkg` placed, **MixLab** included.

## What is deliberately not automatic

The audit log the privileged helper keeps is root-owned, and so is the helper itself. `mix doctor`
reports both and removes neither, because the audit trail is the record of what it is diagnosing.
`mix uninstall` is the command that removes them, and it asks.
