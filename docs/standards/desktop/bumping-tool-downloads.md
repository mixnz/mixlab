# Bumping a downloaded tool version

MixLab fetches `mysqldump`/`mysql`, `pg_dump`/`psql` and `mongodump`/`mongorestore` from their
vendors when a machine has none. Each download is pinned twice in
[`src-tauri/src/modules/db/drivers/tools.rs`](../../../apps/desktop/src-tauri/src/modules/db/drivers/tools.rs): a version constant that builds the
URL, and a SHA-256 in `archive_source` that the file has to hash to before anything is unpacked.
Both change together — a version bumped without its checksum fails every install with
`error.checksumMismatch`.

| Constant | Suite | Platforms it is fetched for |
| --- | --- | --- |
| `MYSQL_VERSION` | MySQL | Windows, macOS |
| `MYSQL_LINUX_VERSION` | MySQL | Linux x86-64 |
| `PG_VERSION` | PostgreSQL | Windows, macOS (one universal build for both architectures) |
| `MONGO_TOOLS_VERSION` | MongoDB | all |

## When

**`PG_VERSION` has a deadline; the others do not.** `pg_dump` refuses outright to dump a server
whose major version is newer than its own — it reaches backwards to any older release but not
forwards by one. PostgreSQL ships a major release every September, so the pin has to be raised then,
or dumping breaks for whoever upgrades their server first. Nothing in the build will say so.

The rest are bumped only when there is a reason: a withdrawn file, a security fix in a library the
tools carry, a server version the client cannot reach. `MYSQL_VERSION` in particular is deliberately
old — see the comment on it.

## Doing it

1. **Find the new version.**
   - **PostgreSQL** — [EDB's binaries page](https://www.enterprisedb.com/download-postgresql-binaries).
     Its links are opaque `sbp.enterprisedb.com/getfile.jsp?fileid=…` ids; every one redirects to a
     URL that names its version, and that is what the constant is built from:

     ```sh
     curl -sI "https://sbp.enterprisedb.com/getfile.jsp?fileid=1260435" | grep -i '^location:'
     # → https://get.enterprisedb.com/postgresql/postgresql-18.6-1-windows-x64-binaries.zip
     ```

     `PG_VERSION` is the whole `18.6-1`, EDB's build number included, and one bump covers both
     platforms — the page has a *Mac OS X* row whose redirect ends `-osx-binaries.zip`, the same
     version with a different suffix. Take the checksum of each; they are separate files.
   - **MongoDB** — <https://downloads.mongodb.org/tools/db/release.json>, which carries the
     checksums too.
   - **MySQL** — the archives under <https://dev.mysql.com/downloads/mysql/>.

2. **Download it and take the hash.**

   ```sh
   curl -fL -o archive "https://get.enterprisedb.com/postgresql/postgresql-18.6-1-windows-x64-binaries.zip"
   sha256sum archive
   ```

   Cross-check it against the vendor wherever they publish one — MongoDB's `release.json` has
   SHA-256, MySQL publishes MD5. **EDB publishes neither.** A PostgreSQL hash therefore attests only
   that this is the file that arrived on the machine that bumped it, which is worth saying out loud
   in review rather than leaving implied.

3. **Update the constant and the checksum** in `archive_source`, together.

4. **Check what comes out of the archive.** `collect` copies the two programs plus whatever
   `library_dir` calls a library, and a new release can move or rename things. The PostgreSQL rule
   is a *denylist*, `PG_SPARE_LIBRARIES`: the archive carries the whole install's libraries, of
   which ICU, the server's XML support and (on Windows) the Stack Builder GUI are the bulk and none
   of it is loaded by these two programs. New name in that archive, new entry there — and note the
   list is checked on both platforms, which name ICU differently (`icudt77.dll` against
   `libicudata.77.dylib`).

   The macOS archive is the one where this matters most. It stores every versioned alias as a
   **copy** of the library rather than a symlink to it, so ICU alone is three files of 76MB. The
   aliases of the libraries that *are* kept are copied too, and deliberately: which name a given
   build links against is not worth guessing at when the spare copy costs a few megabytes.

   To work out what is really needed, put the tools in an empty directory with the libraries you
   think they want, and run them — a missing one shows up as the program failing to start:

   ```sh
   # Git Bash ships GNU tar, which cannot read a zip. The system bsdtar can — and is also the `tar`
   # the app itself finds at runtime, which is why the zips work there.
   /c/Windows/System32/tar.exe -xf archive -C unpacked
   mkdir minimal && cp unpacked/pgsql/bin/{pg_dump,psql}.exe minimal/
   cp unpacked/pgsql/bin/{libpq,libssl-3-x64,libcrypto-3-x64,libintl-9,libiconv-2,libwinpthread-1,zlib1,liblz4,libzstd}.dll minimal/
   ./minimal/pg_dump.exe --version
   ```

   `--version` proves the libraries load. It does not prove the ones only a real session reaches
   are there, so follow it with a dump against a live server, `-Z` included — that is what pulls in
   the compressors:

   ```sh
   PGPASSWORD=… ./minimal/pg_dump.exe -h HOST -U USER -d DB -Fc -Z zstd -f out.pgc
   ```

5. **Verify on every platform.** Run the
   the *Tool downloads* workflow (the standalone client's `tool-downloads.yml`, not yet wired into this repository's CI — a phase 11 debt) — Actions → *Tool
   downloads* → *Run workflow*, or push to the `ci/tool-downloads` branch:

   ```sh
   git push --force origin HEAD:ci/tool-downloads
   ```

   It fetches each pinned archive on a real Windows, Intel Mac, Apple Silicon Mac and Linux runner,
   unpacks it through the app's own `install`, and runs what comes out. A job reporting *no archive
   for this platform* passed: not every suite is published everywhere.

   This is not optional for a PostgreSQL bump. A library that stops being copied breaks the tools on
   one operating system and nowhere else, and no amount of checking from another machine finds it.

6. **No changelog entry.** A bump nobody notices is not a user-visible change; see
   [changelog.md](changelog.md). A bump that adds a platform, or that a user has to act on, is.

## Adding a platform to a suite

A new arm in `archive_source` needs three things beyond the URL and hash: `library_dir` has to know
which files beside the programs to copy on that platform and where they go (Windows loads from the
executable's own directory, macOS from `@loader_path/../lib`, Linux from `$ORIGIN/../lib/private`);
the settings screen's `downloadable` answer changes on its own, so check that `tools.noDownload*`
still reads true where the button is now hidden; and `require`'s per-suite arm has to have a
*NotFound* message as well as a *NotInstalled* one, since the tools can now be offered rather than
only asked for.
