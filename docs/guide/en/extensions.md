+++
title = "Extensions"
slug = "extensions"
order = 10
summary = "The tools you reach for beside the stack, such as phpMyAdmin, Mailpit and MinIO, installed from a signed registry, with what each one may do shown before you agree."
+++

# Extensions

> **This handbook covers MixLab through the `mix` command line.** If you would rather work in a
> graphical interface, you already have one: every installer places the MixLab window beside the
> command line. Both drive the same MixEngine underneath, so everything in this handbook still
> applies.

An extension is a tool that lives beside your stack rather than inside it: a database administration
interface, a mail catcher, an object store, a search engine. MixLab installs it, supervises it,
and gives it a name and a certificate the same way it does for your own sites.

## What is on offer

```bash
mix extension available
mix extension list
```

`available` is the signed registry MixLab publishes; `list` is what this machine has installed.

There are three shapes an extension can take, and it is worth recognising which you are installing:

| Kind | What it is |
| --- | --- |
| `web-app` | Source served on your own stack, on a generated internal site. For example phpMyAdmin, Adminer |
| `service` | A program MixLab supervises like any other. For example Mailpit, MinIO, MeiliSearch |
| `recipe` | Configuration only: extra web-server directives, a `php.ini` profile |

## Look before you install

```bash
mix extension plan mailpit
```

This changes nothing and prints what installing would produce: what it would download, what services
it would create, what site it would be reachable at, and **what it is asking to be allowed to do**.

Two lines of that plan deserve reading rather than skimming, and they only appear for a `web-app`:

- **Which database an administrative interface would open onto.** A tool like phpMyAdmin freezes
  that at install time, and which server it administers is not a detail to discover afterwards.
- **Which account it would be signed in as.** An extension may declare that it signs in with a
  server's superuser account, the most consequential thing an extension can be granted. The plan
  names the account, says the password comes from your operating system's credential store when the
  pool starts, and says that nothing writes it to disk.

`mix extension install` asks about all of it before it does anything. `--yes` skips the question,
and is for a script that has already read the plan.

## Installing and removing

```bash
mix extension install mailpit
mix extension start mailpit
mix extension stop mailpit
mix extension uninstall mailpit
```

Installing is a job; `--no-wait` hands you the job id instead of waiting.

`mix extension uninstall` **keeps the extension's data** unless you say otherwise, because that is
the answer that can be undone. `--delete-data` is the one that cannot.

## Installing something the registry does not carry

```bash
mix extension inspect ./my-tool
mix extension plan --path ./my-tool
mix extension install --path ./my-tool
```

`inspect` reads an `extension.toml` and tells you what it declares, without installing anything.

**Nothing vouches for an extension installed from a path**, and the record says so for as long as it
stays installed. That is not a warning you can dismiss: it is what makes an unsigned extension
visible in every listing that names it, so nobody has to remember where it came from.

## What an extension is not

**An extension is not an API client.** It does not get to call MixEngine's own API, ask the daemon
to change your machine, or reach anything it did not declare. What it gets is what its manifest
declared and what you agreed to: a port, a site, a service, a database connection. Nothing else.

An extension's site shows up in `mix site list` like any other, and can be started and stopped.
Every other edit to it is refused, and the refusal names the uninstall command that removes it: the
site belongs to the extension, and editing it behind the extension's back would break it.
