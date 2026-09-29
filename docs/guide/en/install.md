+++
title = "Installing MixLab"
slug = "install"
order = 2
summary = "The installer for your system, what it touches, what it deliberately does not, and how to check what you downloaded."
+++

# Installing MixLab

> **This handbook covers MixLab through the `mix` command line.** If you would rather work in a
> graphical interface, you already have one: every installer places the MixLab window beside the
> command line. Both drive the same MixEngine underneath, so everything in this handbook still
> applies.

Every build is published on the project's GitHub releases page, with a checksum and a signature
beside it. Pick the file for your system below. Installing changes as little as it can: nothing is
added to your certificate store, your DNS settings or your firewall until the day you ask for
something that needs it. [What MixLab asks permission for](./permissions.md) has the detail.

**No stable release exists yet.** Every download link below is a permanent URL that GitHub always
resolves to whichever release is newest and *not* a pre-release, so once the first one ships these
links go live with no edit to this page. Until then, get the newest pre-release by hand from
[the releases page](https://github.com/mixnz/mixlab/releases). Right now that is
`v0.0.12`.

## What you are installing

Five programs, and it is worth knowing what each is before one of them surprises you.

| Program | What it does |
| --- | --- |
| `mixengined` | The daemon. It owns everything MixEngine knows and supervises everything it runs. |
| `mix` | The command you type. It asks the daemon and prints the answer. |
| `mixengine-shim` | The stand-in for `php`, `node`, `python` and `ruby` that picks the right version. |
| `mixengine-elevate` | The one program that runs as an administrator, for a few seconds at a time. |
| **MixLab** | The window: a dashboard for the daemon, plus a database client, an HTTP client and a terminal. |

The first three and MixLab are installed together. On Windows they are installed as you, and
`mixengine-elevate` is not placed by the installer: MixLab installs it itself, the first time
something needs an administrator, inside a prompt you were going to see anyway. The `.pkg`, the
`.deb` and the `.rpm` place it as root while they install. Either way MixLab keeps it current: when
an update changes it, the next permission prompt replaces it.

**If you do not want the window, there is a download without it.** Every system publishes a
**headless** installer holding the four command-line programs and nothing else: no window, and on
Linux no WebKitGTK to install. It is linked in each section below and is what a server, a container
image, or any machine with no display wants.

## Windows

[**Download the installer**](https://github.com/mixnz/mixlab/releases/latest/download/mixlab-windows-x86_64-setup.exe)
· [headless installer](https://github.com/mixnz/mixlab/releases/latest/download/mixengine-windows-x86_64-headless-setup.exe)
· Windows ARM: [installer](https://github.com/mixnz/mixlab/releases/latest/download/mixlab-windows-aarch64-setup.exe),
[headless](https://github.com/mixnz/mixlab/releases/latest/download/mixengine-windows-aarch64-headless-setup.exe)

Two installers are published, and either is a complete install.

- **`mixlab-<version>-windows-x86_64-setup.exe`**: a per-user installer. It writes into your own
  profile and puts its directory on your `PATH`, so installing asks for no administrator and
  touches nobody else's account on the machine. It also adds **MixLab** to your Start Menu, offers
  a desktop shortcut on the components page, and makes MixLab the program that opens a `mixlab://`
  link.
- **`mixengine-<version>-windows-x86_64-headless-setup.exe`**: the same installer without MixLab,
  for the four command-line programs and nothing else.

The first time MixLab needs an administrator, usually when you allow its first-run setup, one
prompt also puts its privileged helper in place.

Windows ARM builds are published beside them, named `aarch64`.

**Expect a SmartScreen warning.** MixLab is not signed with an Authenticode certificate, so
Windows shows *"Windows protected your PC"* and hides the button behind **More info → Run anyway**.
That is a statement about a certificate nobody has bought, not about the file: check the signature
below if you want a real answer about what you downloaded. The warning tends to come back with every
release, because reputation with no publisher identity accrues to a file rather than to a project.

## macOS

[**Download the package**](https://github.com/mixnz/mixlab/releases/latest/download/mixlab-macos-universal.pkg)
· [headless package](https://github.com/mixnz/mixlab/releases/latest/download/mixengine-macos-universal-headless.pkg)

**`mixlab-<version>-macos-universal.pkg`**, one package for both Intel and Apple silicon. It puts
the command-line programs in `/usr/local/bin` and **MixLab** in `/Applications`, so the window is in
Spotlight and the Launchpad the moment the install finishes.

**`mixengine-<version>-macos-universal-headless.pkg`** installs the same four command-line programs
and the same privileged helper, without MixLab, for a machine that wants no window. Updates offer
each Mac the package of the kind it has.

MixLab has no Apple Developer ID either, so double-clicking the package in Finder gets you a
Gatekeeper dialog and, on macOS 15 and later, a trip through **System Settings → Privacy & Security
→ Open Anyway**. Installing from a terminal avoids all of that:

```bash
sudo installer -pkg mixlab-*-macos-universal.pkg -target /
```

That is the instruction to reach for first on a command-line product. The package runs as root, so
it also places the privileged helper for you.

## Linux

[**`.deb`**](https://github.com/mixnz/mixlab/releases/latest/download/mixlab_amd64.deb)
· [**`.rpm`**](https://github.com/mixnz/mixlab/releases/latest/download/mixlab-x86_64.rpm)
· arm64: [`.deb`](https://github.com/mixnz/mixlab/releases/latest/download/mixlab_arm64.deb),
[`.rpm`](https://github.com/mixnz/mixlab/releases/latest/download/mixlab-aarch64.rpm)
· headless: [`.deb`](https://github.com/mixnz/mixlab/releases/latest/download/mixengine-headless_amd64.deb),
[`.rpm`](https://github.com/mixnz/mixlab/releases/latest/download/mixengine-headless-x86_64.rpm),
arm64 [`.deb`](https://github.com/mixnz/mixlab/releases/latest/download/mixengine-headless_arm64.deb),
[`.rpm`](https://github.com/mixnz/mixlab/releases/latest/download/mixengine-headless-aarch64.rpm)

Two packages for each family, each a complete install:

- **`mixlab`**: MixLab and the four command-line programs
- **`mixengine-headless`**: the four command-line programs alone, for a server or a machine with no
  display

The `.deb` is for Debian, Ubuntu and their relatives, the `.rpm` for Fedora, RHEL and openSUSE:

```bash
sudo apt install ./mixlab_*_amd64.deb
sudo dnf install ./mixlab-*.x86_64.rpm
```

The two packages replace each other: installing one removes the other.

**Other distributions are not supported.** Arch, NixOS, Gentoo and the rest have no package here;
building from source (below) is the way there.

**The window needs WebKitGTK 4.1 and glibc 2.35**: Ubuntu 22.04, Debian 12, Fedora 38, openSUSE
Leap 15.6 or newer. MixLab is a webview application, and the library it draws with is your
distribution's: `libwebkit2gtk-4.1-0` on Debian and Ubuntu, `webkit2gtk4.1` on Fedora and RHEL,
`libwebkit2gtk-4_1-0` on openSUSE. The `mixlab` package declares it, so your package manager pulls
it in; it also adds a **MixLab** menu entry and its icon.

**The headless package asks for none of that.** The four command-line programs are built against
glibc 2.28, so they run on the long-term-support distributions they are aimed at, and they need no
webview at all.

**Updates come as the next package.** `mix self-update` downloads it, checks it against the signed
release, and prints the command that installs it, `sudo apt install …` or `sudo dnf install …`. On
a desktop it also opens the package in your software centre.

`aarch64` builds are published beside the `x86_64` ones.

## From source

MixLab is Rust, and nothing else:

```bash
git clone https://github.com/mixnz/mixlab.git
cd mixlab
cargo build --release
```

The binaries land in `target/release/`. MixLab is built separately, as a workspace of its own
under `apps/desktop/`. A source build is one more way of installing that runs entirely as you,
which is why placing the privileged helper is never a packager's job.

## Checking what you downloaded

Two files sit beside every artifact, and they answer different questions.

```bash
sha256sum -c mixlab_*_amd64.deb.sha256
minisign -Vm mixlab_*_amd64.deb -P <the key in packaging/updates.pub>
```

The `.sha256` tells you whether two downloads of the same file are the same file. **It is not a
signature** and is not offered as one: anybody who could replace the artifact could replace the
checksum beside it. The unversioned files the links above point at carry their own `.sha256` and
`.minisig`, named after themselves rather than after the versioned file they are a copy of. The
`.minisig` is the real answer: an Ed25519 signature MixLab's own release pipeline makes, against
a public key committed in this project's repository as `packaging/updates.pub` and compiled into
MixEngine itself. That is the same key `mix self-update` checks before it replaces anything.

## After installing

Open a new terminal, since the installer changed your `PATH` and a shell that was already running
has not heard about it. Then ask:

```bash
mix status
```

The first `mix` command starts the daemon if it is not already running. What you should see is a
healthy daemon, its version, and nothing being supervised yet.

Then put the runtime commands on your `PATH`, which is a separate step because it is a separate
directory:

```bash
mix path install
```

That fills `<root>/bin` with the shims that make `php`, `node`, `python` and `ruby` resolve to the
version each directory asks for, rather than to one version for the whole machine. MixLab does
the same from its Dashboard, or from the **Terminal commands** switch in Settings.

If you installed the window, open **MixLab** from the Start Menu, `/Applications`, your desktop's
application menu, or by running `mixlab`. It shows the same daemon `mix status` just answered.

## What the installer did not do

Nothing outside your own account, and nothing to the rest of the machine:

- **No certificate authority** was installed. That happens the first time you ask for HTTPS.
- **No DNS or hosts change** was made. That happens the first time you create a site.
- **No firewall rule** and **no port grant**. Those happen when a site needs them.
- **No runtime and no server** was downloaded. MixLab installs PHP, MariaDB and the rest on
  request, and only the versions you ask for.
- **Nothing was registered to start at login.** `mix autostart enable` is how that becomes true.

Every one of those is described in [What MixLab asks permission for](./permissions.md), including
what each prompt will literally change before you agree to it.

Ready? [Your first site](./getting-started.md) takes about five minutes.
