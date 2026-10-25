//! What an installed build can load, what it does load, and the file that says so.
//!
//! Three facts meet here. Two are the artifact's, written down at install time by
//! [`super::remember`]: which of its extensions are linked in, and which are files it could load.
//! The third is the user's, and it is stored as a **deviation** — `{"xdebug": true}` — so that a
//! reinstall or a patch upgrade brings the new build's defaults with it and keeps only what somebody
//! deliberately turned round.
//!
//! # It is not a `match` on the kind
//!
//! Everything here keys off the artifact declaring an extension directory. A Node install declares
//! none, so its state is empty, it renders no documents, and it gets no directory under `etc/`. The
//! day a runtime that is not PHP publishes loadable modules, this needs no edit.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mixengine_proto::{PackageVersion, RuntimeKind};

use crate::{Error, Paths, Result, RuntimeError, Store};

/// Whether an extension can be turned off at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Linkage {
    /// Compiled in. Always loaded, and no file switches it on or off.
    Static,
    /// A file inside the install that an ini line loads.
    Shared,
}

/// Why an extension is in the state it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// This build's own answer.
    BuildDefault,
    /// Somebody said otherwise.
    User,
}

/// One extension, as a listing describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extension {
    /// What it is called, as the index spells it.
    pub name: String,
    /// Whether it can be turned off.
    pub linkage: Linkage,
    /// Whether it is loaded.
    pub enabled: bool,
    /// Whether that is this build's answer or somebody's choice.
    pub source: Source,
}

/// Everything one installed runtime says about its extensions.
#[derive(Debug, Clone)]
pub struct State {
    /// Which language.
    pub kind: RuntimeKind,
    /// Which version.
    pub version: PackageVersion,
    /// Where the runtime is, so a rendered `extension_dir` can be absolute.
    pub install_path: PathBuf,
    /// Where its loadable extensions are inside that directory, when it has any.
    pub directory: Option<String>,
    /// What the artifact published.
    pub offered: crate::index::Extensions,
    /// What somebody turned round, by name.
    pub choices: BTreeMap<String, bool>,

    /// Settings an installed MixEngine extension asks every managed PHP to carry.
    ///
    /// **Rendered here rather than by whatever writes the file**, because the placeholders in them
    /// are the extension's: `sendmail_path = "{install_dir}/sendmail.sh"` means the directory *that
    /// extension* was installed into, and only its row knows where that is.
    ///
    /// They land as ordinary generated files in the same `conf.d`, which is what makes an
    /// uninstalled extension's ini disappear with it: [`render`] removes every `.ini` of ours that
    /// nothing declares any more, and that pass already existed for an extension somebody turned
    /// off.
    pub additions: Vec<IniAddition>,

    /// The trust bundle this runtime should believe, when there is one.
    ///
    /// **Filled by [`render`] and not by [`state`]**, because it is a fact about the *home* rather
    /// than about the install: `state` is handed a `Store` and answers out of one row, and `render`
    /// is the one that knows where `etc/` is. [`None`] on a home whose daemon has not written a
    /// bundle — a machine whose trust store could not be read — and the ini then says nothing about
    /// certificates at all, which leaves PHP exactly as it was before this task.
    pub ca_bundle: Option<PathBuf>,
}

/// One extension's `[recipe] php_ini`, with its placeholders already substituted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IniAddition {
    /// Which extension asked, which is also what its file is named after.
    pub extension: String,

    /// The lines, in the order the manifest wrote them.
    pub entries: Vec<(String, String)>,
}

impl State {
    /// The shared extensions this runtime loads, in name order.
    ///
    /// `enabled ∪ {chosen on} − {chosen off}`, intersected with `shared` — the intersection being
    /// what stops a choice about a name this build does not ship from producing a line PHP warns
    /// about on every start.
    #[must_use]
    pub fn loaded(&self) -> Vec<String> {
        let shared: BTreeSet<&String> = self.offered.shared.iter().collect();

        let mut loaded: BTreeSet<String> = self
            .offered
            .enabled
            .iter()
            .filter(|name| shared.contains(name))
            .cloned()
            .collect();

        for (name, wanted) in &self.choices {
            if !shared.contains(name) {
                continue;
            }

            if *wanted {
                loaded.insert(name.clone());
            } else {
                loaded.remove(name);
            }
        }

        loaded.into_iter().collect()
    }

    /// Every extension this build has, and what is true of each.
    #[must_use]
    pub fn listing(&self) -> Vec<Extension> {
        let loaded: BTreeSet<String> = self.loaded().into_iter().collect();

        let compiled_in = self.offered.compiled_in.iter().map(|name| Extension {
            name: name.clone(),
            linkage: Linkage::Static,
            enabled: true,
            source: Source::BuildDefault,
        });

        let shared = self.offered.shared.iter().map(|name| Extension {
            name: name.clone(),
            linkage: Linkage::Shared,
            enabled: loaded.contains(name),
            // A choice that agrees with the build is never stored — see `decide` — so a key being
            // present is the whole of the question.
            source: if self.choices.contains_key(name) {
                Source::User
            } else {
                Source::BuildDefault
            },
        });

        let mut listing: Vec<Extension> = compiled_in.chain(shared).collect();
        listing.sort_by(|left, right| {
            left.linkage
                .cmp(&right.linkage)
                .then_with(|| left.name.cmp(&right.name))
        });
        listing
    }

    /// The choices this runtime would have after `name` is turned `enabled`.
    ///
    /// **A choice that agrees with the build is removed rather than written.** A stored deviation
    /// that deviates from nothing would survive the upgrade that changes the default and would then
    /// silently keep the old answer — which is the exact failure storing deviations avoids.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::ExtensionCompiledIn`] for a name this build links in, and
    /// [`Error::NotFound`] for a name it has never heard of.
    pub fn decide(&self, name: &str, enabled: bool) -> Result<BTreeMap<String, bool>> {
        if self.offered.compiled_in.iter().any(|linked| linked == name) {
            return Err(Error::Runtime(RuntimeError::ExtensionCompiledIn {
                kind: self.kind,
                version: self.version.clone(),
                name: name.to_owned(),
            }));
        }

        if !self.offered.shared.iter().any(|shared| shared == name) {
            return Err(Error::NotFound {
                kind: "extension",
                id: name.to_owned(),
            });
        }

        let mut choices = self.choices.clone();
        let by_default = self.offered.enabled.iter().any(|on| on == name);

        if enabled == by_default {
            choices.remove(name);
        } else {
            choices.insert(name.to_owned(), enabled);
        }

        Ok(choices)
    }
}

/// The variable both consumers set, and the only way the generated set reaches PHP.
///
/// **There is no `php.ini`.** This was measured to work on all three systems, and a second file is
/// a second place for the truth to live.
pub const SCAN_DIR_ENV: &str = "PHP_INI_SCAN_DIR";

/// The file that carries `extension_dir` and MixEngine's opinion about a development machine.
const MIXENGINE_INI: &str = "00-mixengine.ini";

/// The two names PHP loads as engine extensions rather than as ordinary ones.
///
/// A fact about PHP and not about the index, which is why it is written here beside
/// [`super::smoke_test`] — the same place, and for the same reason, that "which flag prints a
/// version" lives. The value is the bare name on both systems; modern PHP resolves it to
/// `php_<name>.dll` on Windows itself.
const ZEND: [&str; 2] = ["opcache", "xdebug"];

/// What one extension's file is called, which is what decides load order.
///
/// `conf.d` is scanned in name order:
///
/// - `20` `igbinary`, because `redis` links against it when it can find it and silently stores a
///   serialisation nothing else reads when it cannot;
/// - `40` `opcache`, because an optimiser wants to be under whatever wraps it;
/// - `90` `xdebug`, which wants to be outermost and is the one whose presence changes how everything
///   else behaves;
/// - `50` for everything else.
fn prefix(name: &str) -> &'static str {
    match name {
        "igbinary" => "20",
        "opcache" => "40",
        "xdebug" => "90",
        _ => "50",
    }
}

/// Where this runtime's generated ini set lives: `etc/<kind>/<version>/conf.d/`.
///
/// **Under `etc/` and not inside the install**, which is what `docs/features/runtime-versions.md`
/// said before this module existed and what this changes: an install is a rename of a staging
/// directory over the destination, so a generated `conf.d` living inside it is destroyed by
/// reinstalling the same version — and generated configuration is disposable by the project's own
/// rule.
#[must_use]
pub fn conf_d(etc: &Path, kind: RuntimeKind, version: &str) -> PathBuf {
    etc.join(kind.as_str()).join(version).join("conf.d")
}

/// What an `extension =` / `zend_extension =` line names, per platform.
///
/// **The full filename, on every system and every branch** — which is one rule rather than a rule
/// and an exception, and the reason is that PHP itself has only ever had one.
///
/// The value is joined to `extension_dir` and handed to the dynamic loader *verbatim*. On PHP 7.0
/// and 7.1 that is the whole of it: `main/php_ini.c` builds `<extension_dir>/<value>` for
/// `zend_extension` and `ext/standard/dl.c` does the same for `extension`, with no suffix appended
/// and — this is the part that matters — **no platform branch anywhere in it**. A bare
/// `extension = igbinary` therefore asks for a file called `igbinary`, which no PHP build ships, on
/// Linux and macOS alike. PHP 7.2 added a fallback to both files: try the value as a filename
/// first, then build `<PHP_SHLIB_EXT_PREFIX><value>.<PHP_SHLIB_SUFFIX>` and try that. So a bare name
/// works from 7.2 on and fails on the two oldest branches this build offers, which is why writing
/// the filename is the spelling that loads everywhere rather than the cautious one.
///
/// What PHP would have built is exactly what is written here. Those two macros are
/// `""` and `"so"` on Unix (`main/build-defs.h.in`, and `so` on macOS too — a PHP extension is never
/// a `.dylib`), `"php_"` and `"dll"` on Windows (`win32/build/config.w32.h.in`) — and the Windows
/// spelling is the one nothing could guess: Windows appends `.dll` to an extension-less name but
/// never the `php_` prefix every windows.php.net archive actually ships its modules under.
fn module_file(name: &str) -> String {
    if cfg!(windows) {
        format!("php_{name}.dll")
    } else {
        format!("{name}.so")
    }
}

impl State {
    /// Every file this runtime's ini set is made of, in the order they will be scanned.
    ///
    /// Empty for a runtime that declares no extension directory.
    #[must_use]
    pub fn documents(&self) -> Vec<crate::generate::Document> {
        let Some(directory) = &self.directory else {
            return Vec::new();
        };

        let mut documents = vec![crate::generate::Document::new(
            MIXENGINE_INI,
            self.mixengine_ini(directory),
        )];

        for name in self.loaded() {
            let directive = if ZEND.contains(&name.as_str()) {
                "zend_extension"
            } else {
                "extension"
            };
            let module = module_file(&name);

            documents.push(crate::generate::Document::new(
                format!("{}-{name}.ini", prefix(&name)),
                format!(
                    "; Generated by MixEngine for {} {}. Edits are overwritten.\n\
                     {directive} = {module}\n",
                    self.kind, self.version
                ),
            ));
        }

        // **`60`, which is after every extension and before `xdebug`.** A setting is about how the
        // engine behaves and wants whatever it might mention already loaded; `90` stays xdebug's,
        // because what it wraps includes these.
        for addition in &self.additions {
            let mut contents = format!(
                "; Generated by MixEngine for {} {}, from the {} extension. Edits are \
                 overwritten.
",
                self.kind, self.version, addition.extension
            );

            for (key, value) in &addition.entries {
                contents.push_str(&format!(
                    "{key} = {value}
"
                ));
            }

            documents.push(crate::generate::Document::new(
                format!("60-{}.ini", addition.extension),
                contents,
            ));
        }

        documents
    }

    /// `extension_dir`, then the settings a development machine wants instead of PHP's shipping
    /// defaults.
    fn mixengine_ini(&self, directory: &str) -> String {
        // Absolute, and always written — see `conf_d` and the test beside it.
        let absolute = self.install_path.join(directory);

        format!(
            "; Generated by MixEngine for {} {}. Edits are overwritten, and nothing reads this file\n\
             ; back into state.\n\
             extension_dir = \"{}\"\n\
             \n\
             ; A development machine's defaults, which are not PHP's.\n\
             memory_limit = 512M\n\
             upload_max_filesize = 128M\n\
             post_max_size = 128M\n\
             max_execution_time = 120\n\
             display_errors = On\n\
             error_reporting = E_ALL\n\
             date.timezone = UTC\n\
             \n\
             ; Present and idle until an ini says otherwise, whether it is linked in or loaded.\n\
             ; `revalidate_freq = 0` is the difference between opcache in production and opcache on\n\
             ; a laptop: an edited file takes effect on the next request.\n\
             opcache.enable = 1\n\
             opcache.revalidate_freq = 0\n{}",
            self.kind,
            self.version,
            absolute.display(),
            self.trust()
        )
    }

    /// The two lines that tell PHP which authorities to believe.
    ///
    /// **Here rather than in an environment variable**, and that is the point: `PHP_INI_SCAN_DIR`
    /// names this directory for a `php` in a terminal, and
    /// [`services::pools`](crate::services::pools) sets the same one on the php-fpm spec — so
    /// `php -r` and `curl_exec()` in a browser get the same answer, which is what the `conf.d`
    /// model exists to hold. An exported variable would have reached the terminal and not the pool,
    /// and a site calling another site of this home over HTTPS is the case that matters most.
    ///
    /// **Both directives, because PHP has two TLS stacks.** `curl.cainfo` is libcurl's and
    /// `openssl.cafile` is the openssl stream wrapper's; a `file_get_contents("https://…")` uses the
    /// second and nothing about the first.
    ///
    /// Nothing at all when there is no bundle — [`State::ca_bundle`]'s own rule, and
    /// `extension_dir`'s: a setting pointing at a file nothing wrote is worse than no setting, and
    /// on Windows it would be worse still, since the artifact ships no CA file and PHP would be left
    /// naming one that is not there instead of falling back to Schannel.
    fn trust(&self) -> String {
        let Some(bundle) = &self.ca_bundle else {
            return String::new();
        };

        format!(
            "\n; Every authority this machine trusts, and MixEngine's own, so a site of this\n\
             ; home can be reached over HTTPS from PHP, and the public internet still can.\n\
             openssl.cafile = \"{0}\"\n\
             curl.cainfo = \"{0}\"\n",
            bundle.display()
        )
    }
}

/// Read one runtime's extension state out of its row.
///
/// # Errors
///
/// [`Error::NotFound`] when that version is not installed, [`RuntimeError::UnreadableRuntimeRow`]
/// when either JSON column holds something this build cannot read, and [`Error::Database`] when the
/// table cannot be read.
pub async fn state(store: &Store, kind: RuntimeKind, version: &PackageVersion) -> Result<State> {
    let (kind_column, version_column) = (kind.as_str(), version.as_str());

    let row = sqlx::query!(
        "SELECT install_path, extension_dir, extensions_json, extension_choices_json
         FROM runtime_installs WHERE kind = ? AND version = ?",
        kind_column,
        version_column
    )
    .fetch_optional(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?
    .ok_or_else(|| Error::NotFound {
        kind: "runtime",
        id: format!("{kind} {version}"),
    })?;

    let unreadable = |column: &'static str, value: &str| {
        Error::Runtime(RuntimeError::UnreadableRuntimeRow {
            column,
            value: value.to_owned(),
        })
    };

    let offered: crate::index::Extensions = serde_json::from_str(&row.extensions_json)
        .map_err(|_| unreadable("extensions_json", &row.extensions_json))?;
    let choices: BTreeMap<String, bool> = serde_json::from_str(&row.extension_choices_json)
        .map_err(|_| unreadable("extension_choices_json", &row.extension_choices_json))?;

    Ok(State {
        kind,
        version: version.clone(),
        install_path: PathBuf::from(row.install_path),
        directory: Some(row.extension_dir).filter(|dir| !dir.is_empty()),
        offered,
        choices,
        additions: additions(store, kind).await?,
        // `render` fills this: it is the caller that knows where `etc/` is. See the field.
        ca_bundle: None,
    })
}

/// What every installed extension asks this kind of runtime to carry.
///
/// **PHP and nothing else, because `[recipe] php_ini` says so.** The feature says *"for every
/// managed PHP"*, and a Node install has no ini set to add a line to; asking anyway would be a file
/// rendered into a directory that does not exist.
///
/// # Errors
///
/// Whatever reading the `extensions` table reports, and [`ExtensionError::ExtensionField`] for a
/// value whose placeholders cannot be rendered — which is a manifest that was installed and then
/// could not be applied, so it is reported rather than skipped.
///
/// [`ExtensionError::ExtensionField`]: crate::ExtensionError::ExtensionField
async fn additions(store: &Store, kind: RuntimeKind) -> Result<Vec<IniAddition>> {
    if kind != RuntimeKind::Php {
        return Ok(Vec::new());
    }

    let mut additions = Vec::new();

    for installed in crate::extensions::store::all(store).await? {
        let Some(recipe) = &installed.manifest.recipe else {
            continue;
        };

        if recipe.php_ini.is_empty() {
            continue;
        }

        let context = crate::extensions::render::Context::installed(&installed);

        let mut entries = Vec::with_capacity(recipe.php_ini.len());

        for entry in &recipe.php_ini {
            let field = format!("recipe.php_ini.{}", entry.key);
            let value =
                crate::extensions::render::value(&installed.id, &field, &entry.value, &context)?;

            entries.push((entry.key.clone(), value));
        }

        additions.push(IniAddition {
            extension: installed.id.as_str().to_owned(),
            entries,
        });
    }

    Ok(additions)
}

/// Put this runtime's ini set on disk, and say whether anything about it moved.
///
/// **The sweep is this function's own and not [`install`]'s.** That one sweeps a directory a recipe
/// declares, and takes everything in it that no document owns; this directory belongs to a runtime
/// rather than a service, and only its `.ini` files are ours to remove — anything else in a
/// `conf.d` is somebody's, dropped there by hand. An extension turned off would otherwise leave its
/// file behind and go on being loaded, and the same pass repairs a directory left by a build that
/// named its files differently.
///
/// [`install`]: crate::generate::document::install
///
/// # Errors
///
/// [`Error::Io`] naming the file or directory that could not be read, written or removed.
pub async fn render(paths: &Paths, state: &State) -> Result<bool> {
    let directory = conf_d(paths.etc(), state.kind, state.version.as_str());

    // **Named only when it is there**, on `extension_dir`'s own rule: a setting pointing at a file
    // nothing wrote is worse than no setting. A home whose daemon has never run, or whose trust
    // store could not be read, renders an ini that says nothing about certificates.
    let bundle = crate::generate::ca::path(paths.etc());
    let state = &State {
        ca_bundle: bundle.is_file().then_some(bundle),
        ..state.clone()
    };

    let documents = state.documents();

    if documents.is_empty() {
        return Ok(false);
    }

    let installed = crate::generate::document::install(&directory, &documents, &[], None).await?;
    let mut changed = installed.changed();

    let ours: BTreeSet<PathBuf> = documents
        .iter()
        .map(|document| document.relative().to_path_buf())
        .collect();

    let unreadable = |source| Error::Io {
        action: "read the generated directory at",
        path: directory.clone(),
        source,
    };

    let mut entries = tokio::fs::read_dir(&directory).await.map_err(unreadable)?;

    while let Some(entry) = entries.next_entry().await.map_err(unreadable)? {
        let name = PathBuf::from(entry.file_name());

        if ours.contains(&name) || name.extension().is_none_or(|kind| kind != "ini") {
            continue;
        }

        tokio::fs::remove_file(entry.path())
            .await
            .map_err(|source| Error::Io {
                action: "remove the generated file at",
                path: entry.path(),
                source,
            })?;

        // Info rather than debug: `extension_lifecycle` has twice found this file still there on
        // windows-latest (runs 36036932030 and 36308355036) with nothing in the log to say whether
        // this line ran. Now the daemon log answers that.
        tracing::info!(
            file = %entry.path().display(),
            "removed a file an extension that is no longer loaded left behind"
        );
        changed = true;
    }

    Ok(changed)
}

/// Turn one extension on or off, and answer with the state that leaves.
///
/// **Validated before anything is written**, which is where the two refusals come from: a request
/// this build cannot satisfy leaves the row exactly as it was rather than producing a file that
/// quietly does nothing.
///
/// # Errors
///
/// [`RuntimeError::ExtensionCompiledIn`], [`Error::NotFound`] for an unknown name or an uninstalled
/// version, and [`Error::Database`] when the row cannot be written.
pub async fn choose(
    store: &Store,
    kind: RuntimeKind,
    version: &PackageVersion,
    name: &str,
    enabled: bool,
) -> Result<State> {
    let current = state(store, kind, version).await?;
    let choices = current.decide(name, enabled)?;

    let encoded = serde_json::to_string(&choices).unwrap_or_else(|_| "{}".to_owned());
    let (kind_column, version_column) = (kind.as_str(), version.as_str());

    sqlx::query!(
        "UPDATE runtime_installs SET extension_choices_json = ? WHERE kind = ? AND version = ?",
        encoded,
        kind_column,
        version_column
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?;

    tracing::info!(
        kind = kind_column,
        version = version_column,
        name,
        enabled,
        "an extension was turned round"
    );

    Ok(State { choices, ..current })
}

/// Render every installed runtime's ini set, and answer with the versions whose set moved.
///
/// [`crate::shims`]' policy rather than a new one: this is a projection of a table, so it is rebuilt
/// on every daemon start as well as after each change, and a home whose `etc/php/` was deleted is
/// repaired by starting the daemon.
///
/// # Errors
///
/// The first failure that stops a runtime from being rendered — a table that cannot be read, a
/// directory that cannot be written.
pub async fn refresh_all(store: &Store, paths: &Paths) -> Result<Vec<PackageVersion>> {
    let mut moved = Vec::new();

    for summary in crate::runtimes::records(store, None).await? {
        let state = state(store, summary.kind, &summary.version).await?;

        // Which extensions this rewrite still writes an ini for — the other half of the question the
        // removal line above answers, when a file outlives its extension.
        if !state.additions.is_empty() {
            tracing::info!(
                runtime = %summary.version,
                extensions = ?state
                    .additions
                    .iter()
                    .map(|addition| addition.extension.as_str())
                    .collect::<Vec<_>>(),
                "an ini set keeps the lines of these extensions"
            );
        }

        if render(paths, &state).await? {
            moved.push(summary.version);
        }
    }

    Ok(moved)
}

/// Remove a runtime's generated ini set, directory and all.
///
/// Called by `runtime.uninstall`, which already removes a pool's `etc/<service-id>/` — this is the
/// second directory that rule now covers. **Removing what is not there is not a failure**: a runtime
/// installed before this build had none.
///
/// # Errors
///
/// [`Error::Io`] when the directory is there and cannot be removed.
pub async fn discard(paths: &Paths, kind: RuntimeKind, version: &PackageVersion) -> Result<()> {
    // The version's directory rather than its `conf.d`, or `etc/php/` fills with empty shells.
    let directory = paths.etc().join(kind.as_str()).join(version.as_str());

    match tokio::fs::remove_dir_all(&directory).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Io {
            action: "remove the generated directory at",
            path: directory,
            source,
        }),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod trust_tests;
