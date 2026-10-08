# Changelog

## Unreleased

### Added
- `mix site create --from <domain>` adopts one site from a `mixengine.toml` that lists several.
  `mix project show` lists the sites in the file and which ones this machine does not have yet.
- In MixLab, a project's panel on the Projects screen lists the sites in its `mixengine.toml`. Add
  creates a missing one from what the file says.
- Ruby on Windows can build gems with C extensions, such as the ones Rails needs. Install the
  devkit with `mix package install msys2 <version>`, or from the Ruby row in MixLab's Languages
  tab. `mix runtime install ruby` names the version to use.
- `mix runtime available` and MixLab's Languages tab say what a release cannot do on this machine,
  such as a Windows Ruby that has no compiler.
- Applying a blueprint ends with what is left to run, such as `npm run dev` for Next.js. In MixLab,
  Run opens each one in a Terminal tab with the project's runtimes on PATH, and Save keeps a dev
  server as a Terminal target.
- The WordPress blueprint downloads and unpacks WordPress into the project, after you agree to it.
- Blueprints for CakePHP, CodeIgniter, Craft CMS, Statamic and Yii.
- A Terminal target on this machine can set environment variables and folders ahead of PATH, and
  can choose what a tab does with its commands when it comes back: run them, type them, or just
  open.

### Changed
- Applying a blueprint in MixLab sets up a web server when the home has none, and opens the site
  in your browser when nothing is left to run.

### Fixed
- Exporting a project with more than one site now writes every site into `mixengine.toml`, each with
  the services it uses. It used to leave all of them and their services out. Entries for sites this
  machine does not have are kept, and the export names them.

## v0.0.16

### Added
- MixLab's Tunnel tab shares an address on this machine on the internet, such as a dev server, through a Cloudflare quick tunnel. MixEngine is not needed.
- On a Mac, MixLab ▸ Remove MixLab from this Mac… removes everything in one go, after showing you
  what it will undo. From a terminal, `mix uninstall --package` does the same.
- On Linux, `mix uninstall` now tells you the command that removes the package itself.
- SeaweedFS in Add-ons: a local S3 for the files your project stores in the cloud. Its S3 port shows on its row in Add-ons and in `mix extension list`.

### Changed
- A module added to a preset now appears for everyone who chose that preset. On the Database tools preset, the tray icon now opens a panel, which lists running tunnels.
- The Add-ons screen shows the ports each add-on holds, such as Mailpit's SMTP port.
- Starting, stopping or turning on an add-on shows that it is under way on its row, as the Dashboard does, until it has finished.
- MixLab has a new Dashboard, a black Dark theme and ten colour themes in place of the accent colour, and shows CPU per core or for the whole machine.
- The Add-ons screen shows an install's progress, lists each add-on once with what it is for, and opens or turns a web add-on on and off from its own row. `mix extension list` shows what each one is for too.
- A site that cannot start says why, on the Sites and Add-ons screens and in `mix site show`, and MixEngine's starting page says where to look.
- Mailpit's web page opens from the Add-ons screen. A Mailpit installed before this needs installing again to get the Open button.

### Fixed
- `mix database credentials … | tail -1` gives the password alone again. It carried two leading
  spaces, so a script that passed it to a database client was refused.
- A blueprint whose database account name is already taken on the server picks the next free one
  (`shop-2`, `shop-3`, …) and says so under the step, instead of stopping there.
- A service that cannot start says why in the first sentence — the missing password, the entry
  that would not resolve — instead of "the process could not be started at all"; a database
  missing its own password names `mix service reset-credential`, which writes a new one.
- A version you keep when updating no longer offers the same update again, in `mix` and MixLab.
- An add-on's site, such as Adminer, opens right after it is installed or reinstalled, without restarting MixEngine.
- phpMyAdmin starts and signs in to your database again. It was looking for the database password under an old name.
- Uninstalling an add-on that is running, such as Mailpit, stops it first, so it can be installed again. An install also clears what an earlier failed uninstall left behind.
- Answering `mix self-update`, `mix elevation grant` or a server switch after half a minute no
  longer fails with "the connection to the daemon failed: Broken pipe". `mix` reconnects on its own.
- After an update, MixEngine starts again only the services that were running. Before, it also
  started the ones you had stopped.
- Removing MixLab with its data now forgets every password MixEngine kept in the macOS Keychain,
  including the ones an older MixEngine or MixLab wrote. Before, those rows failed and the
  removal stopped there.

## v0.0.15

### Added
- MixLab installs with one command: `curl -fsSL https://mixnz.github.io/mixlab/install.sh | sh`
  on macOS and Linux, `irm https://mixnz.github.io/mixlab/install.ps1 | iex` on Windows. It picks
  the installer for your machine and checks its signature first.
- In MixLab, a site's ⋮ menu on the Sites screen starts it, stops it or deletes it.
- A runtime install or a blueprint being applied can be cancelled from MixLab.
- Domains shows the certificate each site is actually served with, and what is wrong with it.
- Projects writes a project's `mixengine.toml` with one click.

### Changed
- On Windows and macOS, the Domains screen says browsers follow the system store, instead of
  "Not searched" with a Fix button that had nothing to fix.

### Fixed
- Deleting a shared site also closes the firewall rule and the local network name it was shared
  under. Before, they stayed until MixEngine next started.
- Icon sizes in the tables of Sites, Domains, Packages and Add-ons now match Dashboard and Projects.

## v0.0.14

### Added
- MixEngine runs databases on a Linux server with no desktop: `mix daemon credential-store home`
  keeps that home's passwords in a file only your account can read.
- Available versions are listed one row per version series, with older patches one click away.
- An installed runtime or server updates to the newest patch of its series, and its sites,
  settings and pins move with it (`mix runtime upgrade`, `mix package upgrade`, and *Update* in
  MixLab).

### Changed
- MixEngine checks for new versions with one small request, and downloads the version list only
  for the runtimes and servers it is asked about, and only when they change.
- When part of the version list cannot be read, the rest is still listed, and `mix` and MixLab
  say which runtime or server is missing.
- MixLab no longer calls itself MixDB anywhere: not in its messages in a terminal, and not in the
  names of the temporary tables it creates while it changes a table. Your theme, language, modules
  and open tabs carry over.
- Switching between light and dark in Settings fades from one to the other instead of flashing.
- Languages and Packages list the newest version of each first, both what is installed and what
  you can download.
- MixLab is about half its old size on Linux and macOS, and a third smaller on Windows.
- On Linux and macOS, `mix` and the helper programs beside it are about a fifth smaller.
- On Linux and macOS, the MixEngine daemon is about a sixth smaller.

### Fixed
- MixEngine's screens show that they are loading while they wait for MixEngine's answer, instead
  of saying there is nothing there or showing a blank page.
- On macOS, the first launch of a new version asks for your Keychain password once instead of
  twice. A connection MixEngine handed over reads its password when you press Connect, and a tab
  left connected to one opens on its form.
- Starting a database whose password is in the Keychain no longer fails while macOS is still asking
  you to allow it. The service waits for your answer, then starts.

## v0.0.13

### Changed
- MixLab keeps running in the tray when you close its window, whichever modules you use, so open
  terminal sessions and connections survive. Quit from the tray icon's menu. The switch that opens
  MixLab at login is now in Settings → General.
- Messages in MixLab, in `mix` and in the handbook read more plainly, in English and Vietnamese.

### Fixed
- The tray panel no longer leaves a dim rectangle over the desktop around its edges.
- With no project yet, the new-site form sends you to the Projects screen instead of to
  `mix project add`, a command that does not exist.
- The handbook's troubleshooting page gives the right form of `mix runtime uninstall` and
  `mix package uninstall`.

## v0.0.12

### Fixed
- After installing an update on Windows, MixLab restarts on the new version even when MixEngine
  does not come back. It no longer stays open on the old version and fails the next try with
  *Access is denied*.
- An update starts again only the services that were running before it, not every service
  MixEngine knows.

## v0.0.11

### Changed
- MixEngine uses less CPU while nothing is happening: it no longer rewrites every service's
  configuration every thirty seconds to check what is idle, and it checks that MySQL, MariaDB and
  Redis are healthy without starting a program every ten seconds.
- CPU in the tray, the Dashboard, the Metrics screen and `mix metrics` is now a share of the whole
  machine, as in Task Manager, and no longer jumps from one second to the next.
- When a new version is offered, *Read the release notes* opens the release page in your browser,
  instead of listing the commits that went into it.

### Fixed
- On Windows, MixLab and `mix` write every path with backslashes. Paths such as
  `C:\Users\you\blog/public` no longer mix the two separators.
- MixLab and `mix service adopt` no longer list a MySQL or MariaDB temporary folder as an
  unfinished database every time MixEngine starts.

## v0.0.10

### Added
- MixLab and `mix service adopt` bring back databases an earlier install left in its folders, with
  a new admin password. The databases inside are kept.

### Changed
- A new MixLab version is offered in a panel in the corner: download it, then install it when you
  are ready. Settings → Updates takes the same two steps.
- `php`, `node`, `python` and the rest are on your PATH only for the languages you installed with
  MixEngine, so a Node.js or Python you installed yourself is no longer hidden. A tool you add with
  `npm install -g` or `pip install` is a command as soon as it lands.
- Sync asks your server once what changed on your other machines, instead of once for each kind
  of item. A sync server you run yourself needs updating to this release.
- The download icon shows only when another machine changed something, not every time sync
  checks with your server.
- Start MixEngine waits until the four data folders are listed, so you can move them before the
  first start.

### Fixed
- On Windows, MixEngine no longer uses several percent of your CPU while the tray panel or the
  Dashboard is open.
- A blueprint that installs PHP now turns on its PHP extensions, such as `redis` for Laravel, and
  turns them on for the PHP version the project uses, not the newest one installed.
- Reinstalling MixLab over folders an earlier install kept now lists the runtimes and packages
  already in them, instead of refusing to install them again.
- If MixLab closed while sync was sending an edit, it no longer says afterwards that the edit was
  replaced by a newer one.
- On Windows, MixLab opens straight into its window. An empty window no longer flashes and
  disappears first.
- On a narrow window, a resting service no longer makes the Dashboard's service table scroll
  sideways, and memory such as `44.3 MB` stays on one line. Hover *Resting* to see when it starts
  again.

## v0.0.9

### Added
- MixLab checks for, downloads and installs its own updates from Settings → Updates, whether or not
  MixEngine is running. A dot on the Settings button says when a new version is waiting.

### Changed
- MixEngine no longer checks for updates on its own. Run `mix self-update --check` to ask.
- The MixEngine tab no longer has an Updates section. Updates live in MixLab's Settings.
- While sync runs, MixLab shows a download icon when it fetches changes and an upload icon only when
  it sends yours, in place of the spinning one.

## v0.0.8

### Added
- MixLab puts `php`, `node`, `python` and the rest on your PATH in one click: a switch in Settings,
  and a reminder on the Dashboard until it is done. Same as `mix path install`, no admin prompt.
- A Mac that installed MixLab from the `.pkg` can now update from MixLab or with `mix self-update`.
  The next `.pkg` opens in Installer.app, and when it is done MixEngine restarts on the new version
  with your services. Install the first version that can do this by hand, once.
- The Dashboard, the Services screen and `mix service list` show which version each service runs.

### Changed
- On Windows, uninstalling MixLab removes everything it set up on the machine: the hosts entries,
  the certificate, the `.test` DNS rule and the port grant. You choose whether to keep your data and
  any folders you moved elsewhere. If a program still has a file open, the uninstaller names it and
  changes nothing. The Uninstall section in MixLab is gone.
- MixEngine keeps its admin helper up to date on its own. After an update it asks for admin rights
  once, and only when the helper changed.
- Each system has two downloads: the installer with MixLab, and the same installer without it. The
  zip, tarball and AppImage downloads are gone, and a Linux package updates through its installer.
- On Windows, the commands in MixEngine's `bin` folder take about 14 MB instead of about 220 MB, and
  each `php` or `node` you type starts a few milliseconds sooner.
- An idle MixEngine uses far less CPU. The Dashboard stops asking for live numbers while MixLab's
  window is not in focus.
- On macOS, after an update MixEngine asks for your login password once to reach your database
  passwords. Before, it asked once for each of them.

### Fixed
- Removing MixLab with its data now also removes its saved passwords and your sync sign-in.
  Before, installing it again signed you straight back in.
- On macOS, opening MixLab again after closing its window to the tray shows the window. Before, only
  the tray's Open MixLab did.
- The installer screen in MixLab no longer gets stuck. It has Open installer again and Back.
- The PATH switch in Settings and the reminder on the Dashboard show the current state each time you
  come back to them.

## v0.0.7

### Added
- Sync, in Settings: sign in to MixLab's server or one of your own and choose, one kind at a time,
  what follows you to your other machines. Everything is encrypted on this machine before it
  leaves, and the server cannot read any of it. A forgotten password is recovered with the
  recovery key and loses nothing, and an account moves to another server unchanged. Saved
  passwords can follow too, each kind on its own switch.
- Run your own sync server, on Cloudflare Workers or in Docker: Settings → Sync links to a guide
  that covers both.
- MixEngine in the tray on Windows, macOS and Linux: a panel with your services (Start and Stop),
  Stop all, your sites, Open MixLab and Stop MixEngine, which asks first. Closing
  MixLab's window keeps it in the tray, and a Settings switch opens it there at login.
- Java 11, 17, 21 and 25 install and pin like any runtime: `mix runtime install java`, a `java` pin
  in MixLab's project form, and `java`, `javac`, `jar`, `jshell`, `keytool` and `jlink` on the PATH.
  A pinned JDK carries its own `JAVA_HOME` and trusts your local HTTPS sites; on Linux the install
  says which system libraries it may be missing and goes on.
- Go 1.21 to 1.27 installs and pins like any runtime: `mix runtime install go`, a `go` pin in
  MixLab's project form, and `go` and `gofmt` on the PATH. A pinned Go stays the Go that builds: a
  `go.mod` asking for a newer release no longer downloads and runs it.
- MongoDB 6.0 to 8.3 install and run as a service: `mix package install mongodb` and `mix service
  create mongodb@main`, or the same from MixLab's Packages and Services screens. It listens on
  loopback only and keeps its data across restarts.
- MixLab opens a MongoDB service in a Mongo tab, from its Services screen and from `mix database
  open`, and no longer offers to create a database on a server that makes none.
- Two blueprints with MongoDB: `laravel-mongodb` (Laravel on PHP-FPM, the `mongodb` extension on)
  and `express-mongodb` (a Node.js server on port 3000).
- A release that needs a processor with AVX — every MongoDB — is refused before it downloads on a
  processor without it, in MixLab and in `mix`.
- Installing a runtime or a service on Windows no longer ends at a missing Visual C++ runtime: MixLab
  and `mix` say what is missing before anything downloads, and after one yes MixEngine downloads
  Microsoft's redistributable, checks that Microsoft signed it, installs it — Windows asks for
  approval if it needs to — and then installs what was asked for. `mix runtime install` and `mix
  package install` take `--yes`, and `mix blueprint apply` takes `--install-prerequisites`.
- Where Linux or macOS is too old for a release, MixLab and `mix` name the newest release that does
  run and offer to install that instead. `mix runtime available` and `mix package available` show a
  `NEEDS` column when a release lacks something here.
- `--ignore-requirements` on the same commands installs anyway; the runtime is still run once before
  it is kept.
- One site can now answer different path prefixes with different things: `mix site create` and `mix
  site update` take `--proxy /api=http://127.0.0.1:3003/xyz`, `--php /admin` and `--files
  /assets=dist`, repeatable, with `--no-routes` to clear them. The upstream's path, when it has one,
  replaces the matched prefix — so a dev server at `/` and an API on another port under `/api` is a
  single site. The longest prefix wins, and both Caddy and nginx serve it identically.
- MixLab's site form edits those routes, and its Sites list shows how many a site has.
- Every tab of MixLab's Runtimes screen has a search box over the versions not installed yet, so a
  long list of releases can be narrowed by name, version or channel.
- `mix service autostart <service> --on|--off` reads and sets whether a service starts with
  MixEngine, and `mix service list` has an `AUTOSTART` column.
- Services set that way now start when MixEngine does, in dependency order — so a machine that was
  restarted comes back with the same things running, without anybody starting them by hand.
- MixLab's Services screen has that switch, beside the idle timeout, and its Dashboard has an
  Autostart column — so what comes back after a reboot is visible without opening anything.
- `mix blueprint apply --with-front-end` installs a web server too where the machine has none, so a
  blueprint with a site no longer ends with a site nothing serves. A machine that already has one —
  Caddy or nginx — is left alone.
- `mix blueprint apply --autostart` marks the services it creates to start with MixEngine. Services
  the apply found already there keep whatever their owner set.
- MixLab's Dashboard offers to build your first site while the machine has none: pick a stack, name
  it, and one button installs, configures and starts everything it needs, then hands you its
  address.
- Applying a blueprint in MixLab now ends where the project does: it spends the elevation prompt,
  starts the services that project needs, and names the address of the site it made with a button to
  open it. Before, an apply from the Blueprints screen stopped at a list of steps. An apply whose
  `[scaffold]` command failed says so at the top of that panel instead, and hands over the address
  as plain text rather than as an invitation to open a half-built site.
- MixLab says what leaving the `[scaffold]` consent box unticked will mean *before* the apply runs —
  the button reads *Set up without running the command*, and the finished apply says the project
  folder is still empty and prints the command you can run yourself. It used to be one line among
  ten, after the fact.
- `mix blueprint apply --start` starts the services this project needs once the apply is done, after
  the one elevation prompt rather than before it — not every service the machine has. `mix service
  start --project <name>` asks the same question on its own.
- MixLab's first launch brings a MixDB user's saved connections, hosts, environments, drafts and
  their passwords across — once, leaving the MixDB install and its credentials untouched.
- MixLab asks on first run what it will be used for — MixEngine alone, everything, or the database
  tools — and Settings has a Modules pane that changes the answer. Turning a module off closes its
  tabs and hides it; nothing saved is deleted, and turning it back on finds it where it was.
- *Save battery* in MixLab's Settings, and `mix service save-resources`: pause services nobody is
  using, and start them again on the next visit. Off unless you turn it on.
- A service MixEngine paused shows as *Resting* in grey on the Dashboard and Services, not as a red
  *Stopped*.
- A PHP site whose pool is still starting shows a page that reloads itself, instead of a bare 502.
- MixLab's Services screen starts, stops and restarts the selected service from its header.

### Changed
- This release starts its database from scratch. A home made by v0.0.6 or earlier will not open
  and reports that it was written by a different version of MixEngine. Delete `mixengine.db` in
  that home, or the whole home, and start MixEngine again.
- Downloads that include MixLab are named `mixlab-…`: the Windows installer and zip, the macOS
  `.pkg`, the AppImage, the `.deb` and the `.rpm`. The headless downloads keep the `mixengine-…`
  name. On Linux the package is `mixlab` now, and installing it replaces an installed `mixengine`
  package. Remove it with `apt remove mixlab` or `dnf remove mixlab`.
- MixLab's links are `mixlab://` now. `mixdb://` links no longer open it; the installer takes
  back the `mixdb://` registration an earlier release made.
- In a database tab, the header, the table list and the filter bar use MixLab's usual sizes, and the
  sidebar opens wider, so the picker, the tabs and the search box are no longer cramped. Rows and
  results stay compact.
- The product is called MixLab, and MixEngine is the engine inside it: one download still gives you
  both, and MixEngine on its own remains the headless archive for a machine with no screen. The
  handbook moved to `https://mixnz.github.io/mixlab/` and the old address stops answering. Nothing
  you type changed — `mix`, `mixengined` and `MIXENGINE_HOME` keep their names, and an existing
  install keeps updating.
- MixLab is set in Geist, with a new colour scheme for its light and dark themes and mint as the
  default accent; the System theme now follows the operating system as it switches.
- The Liquid glass appearance setting is gone.
- MixLab's Dashboard, Projects, Sites and Domains & TLS screens are redrawn; the Dashboard's services
  can be filtered to running or stopped, and a service's menu opens its logs.
- MixLab's Runtimes, PHP extensions, Services, Blueprints, Metrics, Logs, Add-ons and Settings screens
  are redrawn: PHP extensions are tiles with an On/Off filter, Services lists each service's real state,
  and Blueprints can be searched.
- MixLab's database connection editor is redrawn: saved connections can be searched and filtered by
  engine, the engine is picked from a list, and a route card shows how the connection travels with a
  connection string to copy that never includes the password.
- MixLab's terminal follows the light and dark theme and the accent; REST methods and response statuses
  are drawn as coloured tags and pills.
- Requests in MixLab's REST sidebar are as tall as tables in the database sidebar.
- Services are no longer stopped for being idle unless *Save battery* is on. A site that is up
  stays up. A service you gave its own idle time with `mix service idle` keeps it.
- The web server starts with MixEngine. Existing homes have it turned on once, including one
  where you had turned it off.
- The project form no longer shows *keep warm*; it only matters with *Save battery* on, and
  `mix project keep-warm` still sets it.
- Creating a site from MixLab's project form offers everything the site form does, routes included,
  and both pick a site's services with switches.
- Every MixLab dialog keeps its title and buttons in view while its contents scroll, stays clear of
  the window's top and bottom, has a close button, and lays out its buttons the same way.

### Fixed
- Sharing a site on your network asks for a firewall change only when the machine needs one. Sharing
  a second site, or unsharing before you have answered, no longer raises a prompt that changes
  nothing.
- Deleting a site no longer leaves its permission request behind. What `mix elevation status` lists,
  and what MixLab shows before the system asks you to allow it, is what the machine still needs.
- On Linux, `mix uninstall` no longer deletes the privileged helper that the `.deb` or the `.rpm`
  installed. It lists the helper as kept, and removing the package removes it.
- On macOS and Linux, two MixEngine homes, or two people on one machine, can now set up a MariaDB
  or MySQL 5.6 service with the same name at the same time. Before, one first run could stop the
  other, or fail until the machine restarted.
- A PHP pool is given thirty seconds to come up instead of fifteen, which is what a first start on
  Windows can take while the runtime is read off the disk; a pool that was still starting is no
  longer reported as failed.
- A service that is not ready in time now says what it last printed, in `daemon.log` beside the
  timeout, instead of leaving "not ready within 30s" as the whole report.
- On Windows, `mix` and MixLab no longer fail with "All pipe instances are busy" when they reach
  MixEngine while it is still starting: they wait for their answer instead.
- Dumping or restoring a MySQL database in MixLab no longer fails with "Access denied … (using
  password: NO)" when `~/.my.cnf` holds an empty `password=`: the connection's own credentials win.
- A long text cell in MixLab's database grids shows its text instead of a blank cell ending in `…`
  when the value leads with non-breaking spaces or runs to hundreds of kilobytes.
- MixEngine starts on a Windows machine that has no Microsoft Visual C++ runtime. `mix`,
  `mixengined`, the shim and the elevation helper carried a dependency on `vcruntime140.dll` and
  failed to launch without it; every Windows binary now carries its C runtime inside.
- A `reverse-proxy` site whose upstream carried a path — `http://127.0.0.1:3000/api` — no longer
  costs *every* site on the machine its configuration. Caddy refuses a path in a proxy upstream and
  the whole rendering is judged in one go, so one such site silently froze every other one at its
  last good configuration. The path is now a rewrite, and means the same thing on Caddy and nginx.
- A proxy upstream can no longer carry a newline, a brace or a backtick into a generated web server
  configuration.
- MySQL 5.7 finishes its first run instead of hanging on *set the root password*. The server it was
  started with ran the statement, announced itself ready for connections and then never stopped, so
  the step sat there for its full fifteen minutes and the service was reported as never having
  finished its first run — while a `mysqld` nobody could see stayed running. 5.7 now sets the
  password the way 5.6 does, through a server that reads its statements and exits. 5.6 and 8.0 were
  never affected.
- A project name typed with a space at either end is applied as the name it will be stored under.
  A blueprint applied to `Laravel ` used to register `Laravel`, then fail to find its own project
  when it created the site, refuse to resume over its own first attempt, and leave the failed
  apply unable to take the project back.
- A credential MixEngine generates now belongs to the home that generated it. The operating
  system's credential store is shared by every `MIXENGINE_HOME` on a machine, and until now two of
  them with a database of the same name shared one entry: the second to set up its server replaced
  the first one's root password, and the first one's databases became unreachable — including to
  MixEngine's own shutdown, so the server could only be killed. Entries written by an earlier
  version are found and moved the first time they are read; nothing is deleted.
- A database that refuses the password MixEngine holds for it now says what that means and what the
  ways out are, instead of passing the server's `Access denied` through at the end of a blueprint.
- Applying the Next.js blueprint no longer fails on the last step over an empty folder. A project
  named `Next.js 1` now gets the directory `next-js-1`, because `create-next-app` takes its package
  name from the folder it is run in and npm refuses capitals and spaces. A folder you name yourself
  is still used exactly as you spelled it.
- A scaffold command that fails says so in a sentence you can read: colour codes from tools like
  `create-next-app` no longer arrive as `[31m` in the middle of the message, and a multi-line
  explanation keeps its lines instead of being run together with slashes.
- A blueprint whose command cannot use the folder's name now says so before anything is installed,
  and names what to rename the folder to — *rename the folder to `next-js-1` and apply again* —
  instead of downloading a runtime, making a database and a site, and then handing you npm's own
  refusal at the last step.
- The project goes in the folder you chose, in both windows. *Build your first site* used to treat
  the folder you browsed to as the place to make one inside; it now means the same thing the
  Blueprints dialog means by it.
- A site whose PHP pool is not running now starts it from the request that needed it, whatever left
  it stopped. Before, only a pool the idle sweeper had stopped could be woken — so after a reboot or
  a `mix daemon restart` every PHP site on the machine answered 502 until somebody ran `mix service
  start` by hand. A service you stopped yourself is still left alone.
- Applying a blueprint again after its project folder was deleted makes the folder again, instead
  of running the blueprint's command in a folder that is not there — which Windows reported as
  *cannot start cmd.exe*. A command that cannot be started now also says the system's own reason.

### Changed
- MixLab's Metrics charts now say what they are drawing: a labelled time axis, a labelled value axis,
  and a crosshair that reads out the average, the peak and how many readings that minute is made of.
  Time nobody measured is drawn as such rather than left to look like a flat line, the peak is a band
  around the average instead of a thick line beside it, and the chart is measured in real pixels — so
  a wide pane no longer stretches the strokes out of shape. A rail under the axis marks the stretches
  MixEngine read once a second, which is where that peak band means anything: everywhere else it took
  one reading a minute, and the peak is the average. Each chart also carries a sentence saying what
  it shows, for anyone reading the screen rather than looking at it.
- MixLab stops offering a second MixEngine tab: one tab is the whole of it, so a window showing
  MixEngine alone has no `[+]` button, and `Ctrl/Cmd+1` goes to the tab rather than opening another.
  The close button on the last tab there is now reads *Reload module* — closing it puts a fresh one
  in its place, which is how a module is reloaded.
- MixLab's Runtimes screen puts Web servers, Databases and Cache & queues on the same tab strip as
  Languages: one row of tabs instead of a *Software* tab that had to be opened first.
- MixLab wears MixDB's mark — three data platters fanned out on a blue tile — in the taskbar, the
  Dock, the window and the browser tab.
- Every checkbox in MixLab is drawn by the app rather than by the operating system: one box, one
  tick, the accent you picked, and a half-tick where a list is only partly selected.
- MixLab has a **PHP Extensions** screen: pick an installed PHP and turn `redis`, `mongodb`,
  `xdebug` and the rest on or off. The same panel is still inside Runtimes, and MixEngine's own
  add-ons are now labelled *Add-ons* so the two stop colliding.
- MixLab's MixEngine sidebar is grouped into Overview, Websites, Environment and Library, with
  Settings at the bottom. Each group name now reads as a heading rather than as one more entry: a
  rule above it, and dimmed type that no longer borrowed the colour of the items below it.
- The desktop window is **MixLab**: its own name, identifier, executable and mark, and MixEngine's
  version rather than one of its own. The daemon, `mix`, the home, the keyring namespace and the
  installers keep MixEngine's name and are unchanged.
- MixLab's tab menu and its `Ctrl/Cmd+1 … N` shortcuts lead with MixEngine, and the number keys
  count across the modules you have turned on rather than across all five.
- A new MixLab tab opens whichever module your profile leads with — the MixEngine dashboard for
  *MixEngine* and *Everything*, a database connection for *Database tools* — so `Ctrl/Cmd+T` no
  longer always opens the database client. What was open when you last closed the window is
  restored first, as before.
- When MixLab cannot find `mixengined` beside itself, the MixEngine tab now lists the directories it
  looked in and offers to reinstall MixEngine, instead of inviting a first install.
- With the database client turned off, MixLab's Services screen offers rather than acts: *open* on
  a database service says it will turn the client on first. A `mixdb://` link or `mix database open`
  turns the client on for the tab it opens, and the tab says so.
- `desktop-app` extensions are gone: `mix database open` opens MixLab, the window MixEngine installs,
  and an install that added MixDB as an extension has it removed on upgrade.
- MariaDB and MySQL services no longer fail with `Access denied … (using password: NO)` on a
  machine whose `~/.my.cnf` sets an empty `password=`. MixEngine's own client commands now read
  none of the machine's option files; the `mysql` and `mariadb` you run yourself still do.

## v0.0.6

### Added
- Opt-in per-site HTTP → HTTPS redirect (`--https-redirect` on `site create`/`site update`), off by
  default; answers with a `307` so a site can turn it back off.

### Updated
- Signed-document clients (runtimes, updates, extension registry, RPC) now share one HTTP
  transport instead of building one each, cutting the daemon's idle memory footprint.

### Fixed
- Creating a site now says which service is missing instead of answering `FOREIGN KEY constraint
  failed`, and a `php-fpm` pool that was deleted while its PHP stayed installed is made again where
  the need is found — before, deleting that service left every new PHP site on the machine failing
  until the daemon was restarted.
- The missing-helper hint now matches how each install format actually ships
  `mixengine-elevate`, instead of assuming every release keeps a copy beside `mixengined`.
- MariaDB starts from a certificate issued once by this home's own authority instead of
  generating a new key at every start, cutting several seconds off every warm start.

## v0.0.5

### Fixed
- probe the helper only after a batch that could have replaced it, and settle two flaky gates

## v0.0.4

- Every .tar.zst and .tar.gz we publish begins with the ./ entry tar writes,
  and the path check read it as one escaping the destination — no runtime or
  package could install on macOS or Linux. The unpackers now skip an entry
  that names the destination itself; safe() and the traversal guard are
  unchanged.

## v0.0.3

- `feat(disk)`: disk usage by category plus a cleanup that only reaches what is safe to lose (`mix disk`, `mix cleanup`).
- `fix(mysql)`: MySQL 5.7 now starts on Windows (`--shared-memory`); daemon waits for a leaving lock holder instead of racing it.
- `fix(runtimes)`: correct the Windows DLL file name for a generated extension line.

## v0.0.2

- `docs(guide)`: rewrite the Vietnamese handbook, point every page at MixDB.
- `fix(release)`: correct version literals left behind by the 0.1.0 rename.

## v0.0.1

MixEngine is a local web development environment: run and switch multiple PHP,
Node.js, Python and Ruby versions, plus bundled Nginx/Caddy and MariaDB/MySQL/PostgreSQL/Redis/
Memcached, local domains with automatic HTTPS — without Docker, without hand-written config files.

Note: this will be the first release. Scope and timeline are still under discussion; not finalized.

Nothing is code-signed or notarised, by design: expect SmartScreen on Windows and Gatekeeper's
"Open Anyway" on macOS. A machine with Smart App Control enforcing is not supported. On ARM64
Windows some runtimes have no build of their own and run under emulation, marked `emulated` in
`mix runtime available`.

## 0.0.1-beta.2 — unreleased

Updates on top of beta.1:

- `mix database credentials` reads a stored password back, and `--password` on `mix database
  create` lets you choose one instead of generating it.
- `--refresh` on `mix runtime available`, `mix package available` and `mix extension available`
  bypasses the registry's six-hour cache and asks again immediately.
- A build that did not come out of the packaging pipeline (a local `cargo run`) now keeps its own
  `MixEngine-dev` home instead of touching the real release home and its database.
- A daemon that fails to start now says why, instead of leaving you to guess.
- One-off child processes on Windows start without a stray console window.
- The installer's final rename gets a longer window so a scanner locking the file doesn't fail it.
- Install docs link to each OS's latest release automatically instead of naming a version.

Nothing is code-signed or notarised, by design: expect SmartScreen on Windows and Gatekeeper's
"Open Anyway" on macOS. A machine with Smart App Control enforcing is not supported. On ARM64
Windows some runtimes have no build of their own and run under emulation, marked `emulated` in
`mix runtime available`.

## 0.0.1-beta.1 — unreleased

The first public beta.

- PHP 7.0+, Node.js 16+, Python 3.10+ and Ruby 3.2+, one immutable directory per version, chosen
  per directory with no shell hook and nothing to activate.
- Caddy, nginx, php-fpm, MariaDB, MySQL, PostgreSQL, Redis and Memcached, run from generated
  configuration that is regenerated rather than edited.
- `http://blog.test` and trusted HTTPS with no prompts after first-run setup; LAN sharing,
  blueprints, extensions and `mix doctor`.
- Installers for six OS/arch targets, a signature-verified `mix self-update`, a handbook in English
  and Vietnamese, and a published TypeScript API contract for clients.

A MixEngine built from source keeps its own home directory (`MixEngine-dev`), so a working tree
cannot migrate the database a released MixEngine is using.

Nothing is code-signed or notarised, by design: expect SmartScreen on Windows and Gatekeeper's
"Open Anyway" on macOS. A machine with Smart App Control enforcing is not supported. On ARM64
Windows some runtimes have no build of their own and run under emulation, marked `emulated` in
`mix runtime available`.
