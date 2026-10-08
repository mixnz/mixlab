+++
title = "Projects and sites"
slug = "projects-and-sites"
order = 4
summary = "The two nouns MixLab is built on, what each one owns, and how a checkout carries its own setup."
+++

# Projects and sites

> **This handbook covers MixLab through the `mix` command line.** If you would rather work in a
> graphical interface, you already have one: every installer places the MixLab window beside the
> command line. Both drive the same MixEngine underneath, so everything in this handbook still
> applies.

MixLab has two nouns and they are worth keeping apart.

A **project** is a directory on your disk that MixLab knows about. It owns the path, a name, and
which language versions that directory uses.

A **site** is something served, under a project. It owns one or more domains, what is served out of
which folder, and what serves it. A project with no site is perfectly normal: it is a directory
whose PHP version MixLab knows. A project can have several sites.

## Registering a project

```bash
cd ~/code/blog
mix project create
mix project list
mix project show blog
```

With no arguments, `mix project create` takes the current directory and names the project after it.
`--name` overrides that, and `--pin` fixes a language version for everything under that directory:

```bash
mix project create --name blog --pin php=^8.3 --pin node=22
```

`mix project update` changes any of it afterwards. One thing to know: `--pin` **replaces** every pin
rather than adding to the set, and `--clear-pins` with no `--pin` removes them all. Deleting a
project forgets it; your files are left exactly as they are.

## Declaring a site

```bash
mix site create --domain blog.test --kind php-fpm --https true
mix site list
mix site show blog.test
```

`--doc-root` is the folder that is served, relative to the project root: `public` for most modern
PHP frameworks, and the project root itself when it is left out. `--domain` may be given more than
once; the first is the **primary**, and the rest are aliases. The primary matters: it is what the
site's canonical URL and its certificate are named after.

`mix site update` changes a site. Like `--pin` above, `--domain` and `--service` replace what the
site had rather than adding to it. Giving neither changes neither.

Starting and stopping a site flips a flag and renders the configuration again:

```bash
mix site stop blog.test
mix site start blog.test
```

Nothing is started or killed by those. A site is a declaration; the services it uses have states of
their own.

## The four kinds of site

| `--kind` | What it is |
| --- | --- |
| `php-fpm` | PHP, through a pool of the version this directory resolves to |
| `static` | Files, and nothing running |
| `reverse-proxy` | Everything forwarded to an address you already have listening, given with `--upstream` |
| `node-app` | A Node process you run yourself, on the port given with `--port` |

`reverse-proxy` and `node-app` are the two that matter when you are already running something.
MixLab gives it a real name and a certificate without taking over how it is started.

## `mixengine.toml`, and adopting a colleague's checkout

A project can describe itself, in a file checked into the repository:

```toml
[project]
name = "blog"

[runtimes]
php = "8.3"
node = "22"

[site]
domain = "blog.test"
aliases = ["api.blog.test"]
doc_root = "public"
kind = "php-fpm"
https = true

[[services]]
name = "mariadb"
version = "11.4"
database = "blog"
```

With that file present, `mix project create` and then `mix site create` with no arguments at all do
what the file says. That is what adopting somebody else's checkout looks like: clone, two commands,
and the same PHP version and the same domain as the person who wrote it.

A project with more than one site writes each one as a `[[sites]]` entry, and each entry says which
services that site uses:

```toml
[[sites]]
domain = "blog.test"
doc_root = "public"
kind = "php-fpm"
services = ["mariadb@main"]

[[sites]]
domain = "admin.blog.test"
kind = "reverse-proxy"
upstream = "http://127.0.0.1:3000"
services = []
```

`mix project show` lists the sites in the file and says which ones this machine does not have yet.
Adopt each with `mix site create --from <domain>`. In MixLab, open the project on the Projects
screen and press **Add** next to each missing site.

Going the other way, `mix project export` writes the current project and every site into
`<root>/mixengine.toml`, keeping everything else already in the file. An entry for a site this
machine does not have is left as it is, and the export names it.

## Which version does this directory use?

Four things can decide, and they are consulted in this order:

1. An explicit flag or environment variable for the command you are running.
2. The nearest `mixengine.toml` that names **this language**, walking up from where you are. A
   manifest that says nothing about PHP is not an answer about PHP, so an outer pin still applies.
3. The registered project whose root is that directory or one above it.
4. The global default.

Rather than work that out yourself, ask:

```bash
mix runtime resolve php
```

That answers which installed version this directory gets **and which of the four decided it**, which
is the half people actually want when a version surprises them. Nothing is run to find out.

## Keeping a project warm

With *Save battery* on, services nobody has used for a while are paused (see the services page).
While you are working on a project, that is a pause you do not want:

```bash
mix project keep-warm blog
mix project keep-warm blog --off
```

This is a verb of its own rather than a setting on the project, because it is something you do for
an afternoon and not part of what the project *is*. It reaches the PHP pool the project's sites
name; it does not yet reach a database they query, because nothing in MixLab records which
database a project uses. With *Save battery* off nothing is paused, and this changes nothing.
