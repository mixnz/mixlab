+++
title = "Privacy"
slug = "privacy"
order = 17
summary = "What MixLab keeps on your machine, the few times it reaches the network by itself, and what the sync server holds when you sign in, none of which it can read."
+++

# Privacy

Effective 21 September 2026 · Last updated 21 September 2026

**MixLab collects nothing about you.** This covers all of it: the `mix` command line, the
MixEngine daemon and the MixLab window. There is no analytics, telemetry, crash reporting or
advertising code in any of them.

The one service of ours is **sync**, and it is off until you sign in. What it carries is encrypted
on your machine before it leaves, under keys the server never receives, so the server holds your
data and cannot read it. The rest of this page says exactly what is kept where.

## Who we are

MixLab is developed and published by mixnz (Nguyễn Hải Quang), Cẩm Mỹ, Đồng Nai, Vietnam. Questions
about this policy, or about privacy in MixLab, go to haiquang9994@outlook.com.

## What stays on your machine

- **The MixEngine home** (`mix status` names it) holds the runtimes, services, sites,
  certificates and logs MixEngine manages. None of it is sent anywhere.
- **The MixLab window** keeps its settings, saved connections, REST requests and history, query
  drafts and snippets as files in its application data folder, named `io.github.mixnz.mixlab`, in
  the usual place for your system.
- **Passwords and secrets** are not written to those files. They go to the credential store your
  operating system already provides (Credential Manager on Windows, the Keychain on macOS, the
  Secret Service on Linux), under the name `MixLab`. They are never logged, and debug output prints
  them as `***`.

## When MixLab reaches the network by itself

Everywhere else it is silent unless you tell it to connect somewhere.

- **Checking for updates.** The daemon reads
  `https://github.com/mixnz/mixlab/releases/latest/download/latest.json` when it starts and once a
  day. The request carries no identifier and nothing about you or your work.
- **Packages and extensions.** The index of what can be installed, and whatever you install from
  it, come from `github.com/mixnz/mixengine-packages`.
- **Database tools.** Dump and restore need the vendors' own tools. Only when you ask, MixLab
  downloads them from `dev.mysql.com`, `get.enterprisedb.com`, `downloads.mongodb.org` and
  `fastdl.mongodb.org`.

GitHub and those vendors see the IP address and user agent of the request, as any web server does,
under their own privacy policies. We receive nothing from them.

**Connections you make yourself**, to a database, over SSH or as a REST request, go straight from
your machine to the address you entered. Nothing of ours sits in the path.

## Sync

Sync is off until you sign in, and after that every kind of data stays off until you turn it on in
Settings. The default server is `https://sync-0.lab.mixnz.com`, run by us on Cloudflare Workers.

**What the server holds:**

- your email address, because it is your login and where codes are sent;
- a hash of that address, which names your account;
- a *verifier* derived from your password, never the password itself;
- the names you gave your machines, and when each was last seen;
- your records as ciphertext, with their sizes and the times they changed.

**What it cannot see:** anything inside a record, the name of any kind of data, a host, a URL, a
file name or a password. The keys that would open a record exist only on your machines.

**Who else is involved.** Confirmation and reset codes are sent through **Mailtrap**, which sees
your address and the letter. **Cloudflare** runs the server and keeps its standard request logs,
including IP addresses, under Cloudflare's privacy policy; we read them only to fix faults. To stop
abuse, the server counts requests per IP address and per account, and forgets each count after an
hour.

**A server you add yourself** belongs to whoever runs it, not to us, and this section does not
describe it.

## Keeping and deleting

- A record you delete leaves a marker for 90 days, so your other machines learn it is gone; then
  the marker goes too.
- Removing a machine from your account signs it out at once.
- Deleting your account removes the account and every record outright, and leaves nothing behind
  that names your address.
- Resetting a forgotten password without your recovery key deletes every record, because nothing
  could read them any more.

## Children

MixLab is a tool for software developers. It is not directed at children, and we do not knowingly
collect personal data from anyone.

## Your rights

Data-protection laws such as the GDPR and the CCPA give you rights to access, correct, export and
delete your data. For sync, MixLab's Settings shows which machines are signed in and removes them;
anything it does not do yet, deleting your account among it, goes to the address above. Everything
else is on your machine and under your control: the MixEngine home, the application data folder,
and the entries named `MixLab` in your credential store.

## Changes

A change to this page updates the date at the top. Its full history is this repository's.
