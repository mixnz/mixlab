//! Getting a rendered file onto the disk: diffed, staged, checked, and only then installed.
//!
//! Four rules, in the order they apply, and each of them exists because of what the *next* thing to
//! happen is — a web server being told to reload:
//!
//! 1. **Identical is not a change.** A rendering that byte-matches what is already there is not
//!    written at all, and [`install`] says so with [`Written::Unchanged`]. The caller's next move is
//!    a reload, and a service reloading because the daemon restarted — dropping connections, on
//!    `mariadb` re-reading a data directory — is a cost the user never asked for and cannot see the
//!    reason for.
//! 2. **The whole set is staged first.** Files go into a staging directory beside the destination,
//!    never into it, because a validator has to be shown a *complete* configuration: a Caddyfile
//!    that imports six site files cannot be judged with two of them from before this render.
//! 3. **Validation happens against the staging directory**, so a configuration that does not parse
//!    is one nothing was installed from. [`features/services.md`] requires exactly that: the
//!    previous config stays live and the error is surfaced.
//! 4. **The install is a rename per file.** Renaming within one directory tree is atomic on every
//!    filesystem this runs on, so nothing ever reads half a file — a web server whose reload races
//!    the write of its own config is the failure this rules out.
//!
//! Rule 4 is [`crate::install`]'s "the rename is the commit", one layer smaller: there it is a
//! directory of an unpacked runtime, here it is a config file. Unlike an install, this cannot be one
//! transaction across the set — several files cannot be renamed at one instant — and it does not
//! need to be: the set was validated as a set, and the window between two renames is microseconds
//! inside which nothing has been asked to reload yet.
//!
//! [`features/services.md`]: ../../../../docs/features/services.md

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{Error, Result, ServiceError};

/// What a [`Validator`]'s arguments write where the file being checked goes.
///
/// A placeholder rather than "appended last", because the real commands put it in the middle:
/// `caddy validate --config <file> --adapter caddyfile`, `postgres --check -D <dir>`.
pub const CONFIG: &str = "{config}";

/// How long a validator is given before it is killed and its verdict counted as a refusal.
///
/// Generous by the standard of what these commands do — `caddy validate` on a dozen sites is
/// milliseconds — because the alternative failure is a Windows runner under load reporting a
/// configuration as broken when it is fine.
const PATIENCE: Duration = Duration::from_secs(30);

/// One rendered file, and where it goes relative to the service's `etc/<service-id>/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// Where it goes, relative to the service's configuration directory. May name a subdirectory —
    /// `sites/blog.test.caddy` — which [`install`] creates.
    relative: PathBuf,

    /// What is in it.
    contents: String,
}

impl Document {
    /// A file at `relative` holding `contents`.
    #[must_use]
    pub fn new(relative: impl Into<PathBuf>, contents: impl Into<String>) -> Self {
        Self {
            relative: relative.into(),
            contents: contents.into(),
        }
    }

    /// Where it goes, relative to the service's configuration directory.
    #[must_use]
    pub fn relative(&self) -> &Path {
        &self.relative
    }

    /// What is in it.
    #[must_use]
    pub fn contents(&self) -> &str {
        &self.contents
    }
}

/// What happened to one file.
///
/// The two that are not [`Unchanged`](Self::Unchanged) are kept apart because they are different
/// news: a file that appeared is a service being configured for the first time, and one that
/// changed is a reload somebody is about to have to explain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written {
    /// There was no such file before.
    Created,
    /// There was, and it said something else.
    Updated,
    /// There was, and it said exactly this. Nothing was written.
    Unchanged,
}

impl Written {
    /// Whether the file on disk is different from what it was.
    ///
    /// What a reload decision is made of — see the module note's first rule.
    #[must_use]
    pub fn changed(self) -> bool {
        !matches!(self, Self::Unchanged)
    }
}

/// What installing one service's set of documents did.
///
/// **Two lists rather than one**, because a removal is not a document and a `Vec<Written>` has no
/// entry for one. It matters to exactly one caller and for exactly one reason: a walk whose only
/// difference is a site that was deleted has to count as changed, or the front end goes on serving
/// the site until something else moves — which is the whole point of a swept directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// One per document, in the order they were given.
    pub written: Vec<Written>,

    /// What a swept directory carried that no document in this set owns. Already removed.
    pub removed: Vec<PathBuf>,
}

impl Installed {
    /// Whether anything on disk is different from what it was.
    ///
    /// What a reload decision is made of — see the module note's first rule.
    #[must_use]
    pub fn changed(&self) -> bool {
        !self.removed.is_empty() || self.written.iter().any(|written| written.changed())
    }
}

/// Which line of a refusal is the one worth repeating.
///
/// **Programs disagree about where they put the reason**, and the difference is not cosmetic: a
/// refusal reported by the wrong line is a message telling somebody their configuration is wrong and
/// nothing at all about why. `caddy validate` prints what it is doing and then what went wrong;
/// `nginx -t` prints what went wrong and then `nginx: configuration file <path> test failed`, which
/// on its own says only that the file this message already names is the file that failed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Reason {
    /// The last line the program printed, which is where most of them end up.
    #[default]
    Last,

    /// The first, for the ones that close with a summary instead.
    First,
}

/// A command that judges a rendered configuration before it is installed.
///
/// `caddy validate`, `nginx -t`, `postgres --check`. Built by the recipe, because which program can
/// judge a file — and what to hand it — is knowledge about that service and nothing else.
///
/// It is run **from the staging directory**, so a configuration whose entry point includes its
/// siblings by relative path is judged the way it will be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validator {
    /// The program to run. Absolute, for [`ServiceSpec`]'s reason: a relative one is whatever the
    /// `PATH` happens to say at the moment it runs.
    ///
    /// [`ServiceSpec`]: mixengine_proto::ServiceSpec
    program: PathBuf,

    /// Its arguments, with [`CONFIG`] standing in for the file being checked.
    args: Vec<String>,

    /// Which rendered file the command is about, relative to the configuration directory. This is
    /// what [`CONFIG`] becomes.
    entry: PathBuf,

    /// Where in what it prints this program leaves the reason it refused.
    reason: Reason,
}

impl Validator {
    /// Run `program` against the rendered `entry`.
    #[must_use]
    pub fn new(program: impl Into<PathBuf>, entry: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            entry: entry.into(),
            reason: Reason::default(),
        }
    }

    /// Say where this program puts the reason it refused a file. See [`Reason`].
    #[must_use]
    pub fn reason(mut self, reason: Reason) -> Self {
        self.reason = reason;
        self
    }

    /// Add an argument. [`CONFIG`] anywhere inside it becomes the path of the staged file.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Add several.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Judge the configuration staged in `staging`.
    ///
    /// # Errors
    ///
    /// [`ServiceError::ConfigRejected`] when the command says no — carrying whichever line
    /// [`Reason`] says the program leaves the answer on — or when it does not answer in time.
    /// [`Error::Platform`] when the program cannot be started at all, which is a recipe naming a
    /// binary that is not installed rather than a configuration that is wrong.
    async fn judge(&self, staging: &Path) -> Result<()> {
        let config = staging.join(&self.entry);
        let args: Vec<OsString> = self
            .args
            .iter()
            .map(|arg| OsString::from(arg.replace(CONFIG, &config.to_string_lossy())))
            .collect();

        let ran = mixengine_platform::process::run_once(
            &self.program,
            &args,
            staging,
            &BTreeMap::new(),
            PATIENCE,
        )
        .await?;

        if ran.succeeded() {
            return Ok(());
        }

        Err(Error::Service(ServiceError::ConfigRejected {
            path: config,
            checker: self.program.clone(),
            // What the program said, or the fact that it said nothing — an empty `detail` would
            // leave a message that names a broken file and no reason.
            detail: if ran.timed_out() {
                format!(
                    "{} did not answer within {PATIENCE:?}",
                    self.program.display()
                )
            } else {
                match self.reason {
                    Reason::Last => ran.complaint(),
                    Reason::First => ran.complaints().lines().next(),
                }
                .unwrap_or("it refused the file without saying why")
                .to_owned()
            },
        }))
    }
}

/// Put `documents` into `directory`, unchanged files left alone and nothing installed unless
/// `validator` accepts the set.
///
/// `swept` names directories, relative to `directory`, whose contents must be **exactly** the
/// documents rendered into them: anything else in one is removed. Only a front end declares one
/// today, and it declares `sites/` — a site whose row is gone would otherwise keep the file it had
/// and keep being served.
///
/// **A set that is entirely unchanged is not validated.** There is nothing to judge: those bytes are
/// the ones already on disk, which were judged when they were written, and running a validator on
/// every walk would put a process launch into `service.list`. A sweep with something to remove is a
/// change and does re-validate, which is deliberate — the checker has to be shown the set that will
/// exist.
///
/// **Two renders of one service may overlap, and each stages into a directory of its own.** Nothing
/// serialises them: `Registry::graph` renders the whole home and is reached from about fifteen
/// places, each spawned and joined by nobody. `staging_for` is what makes that safe, and why it is
/// a name per render rather than a lock above this is argued there.
///
/// What is deliberately *not* claimed is that two renders are one operation. They are two, they
/// commit the same bytes over each other because they read the same rows, and the last one wins —
/// which is what "rendering the same state twice" already means everywhere else in this module. A
/// render whose answer differed from a concurrent one would be a database that changed underneath
/// them, and the render after that change is the one a caller wanted.
///
/// **The staging directory is the swept set already.** It is created fresh on every install, so what
/// the validator is shown is the documents and nothing else; the removal below is what makes the
/// *installed* directory agree with what was judged.
///
/// # Errors
///
/// [`Error::Io`] naming the file or directory that could not be read, written, renamed or removed;
/// [`ServiceError::ConfigRejected`] when the validator refuses the staged set — in which case
/// nothing has been installed or removed, and the staging directory is gone.
pub async fn install(
    directory: &Path,
    documents: &[Document],
    swept: &[&str],
    validator: Option<&Validator>,
) -> Result<Installed> {
    let mut written = Vec::with_capacity(documents.len());

    for document in documents {
        written.push(compare(&directory.join(document.relative()), document).await?);
    }

    let removable = orphans(directory, documents, swept).await?;

    if removable.is_empty() && written.iter().all(|one| !one.changed()) {
        return Ok(Installed {
            written,
            removed: Vec::new(),
        });
    }

    let staging = staging_for(directory).await?;
    let staged = stage(&staging, documents).await;

    // Both failures leave the staging directory behind, and neither is allowed to. No later render
    // will clear it now that each one creates a name of its own, so the removal below is the only
    // thing that ever will.
    let staged = match staged {
        Ok(()) => match validator {
            Some(validator) => validator.judge(&staging).await,
            None => Ok(()),
        },
        Err(error) => Err(error),
    };

    if let Err(error) = staged {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return Err(error);
    }

    create_dir(directory).await?;

    for (document, one) in documents.iter().zip(&written) {
        if !one.changed() {
            continue;
        }

        let target = directory.join(document.relative());

        if let Some(parent) = target.parent() {
            create_dir(parent).await?;
        }

        commit(&staging.join(document.relative()), &target).await?;
    }

    // **After the commit and not before it.** A removal that ran first would leave a window in which
    // a server asked to reload for some other reason read a set with a site missing and its
    // replacement not yet installed. Renaming a staged file into place cannot fail for want of
    // content, so by here the set is whole, and taking the leftovers out is what makes the directory
    // equal to what the checker was shown.
    for orphan in &removable {
        tokio::fs::remove_file(orphan)
            .await
            .map_err(|source| Error::Io {
                action: "remove the generated file at",
                path: orphan.clone(),
                source,
            })?;
    }

    // Whatever is left in here is a file that did not change and was therefore never installed from
    // it. Failing to remove the directory is not worth failing a start over — it is under `etc/`,
    // which is disposable by construction — but it is worth saying, because a staging directory that
    // survives is the visible half of a filesystem that has stopped cooperating.
    if let Err(error) = tokio::fs::remove_dir_all(&staging).await {
        tracing::warn!(
            path = %staging.display(),
            %error,
            "the staging directory could not be removed"
        );
    }

    Ok(Installed {
        written,
        removed: removable,
    })
}

/// Show `documents` to `validator` and install none of them.
///
/// [`install`]'s first half, and the whole of the reason it is separable: a rendering can be judged
/// by the program that will read it *before* anything about the home changes. That is what lets an
/// extension carrying a `[[recipe.front_end]]` fragment the front end will not accept be refused
/// while it is still a document rather than an installation.
///
/// **It differs from [`drift`] in what it can answer.** `drift` compares a rendering against what is
/// on disk and never runs a program; this runs the real checker and says nothing about what is
/// installed. `mix doctor` wants the first, an install wants the second, and neither one is the
/// other with a flag.
///
/// The staging directory is created and removed here whether the judgement passes or fails; a
/// judgement that passed leaves exactly what a judgement that failed leaves, which is nothing.
///
/// # Errors
///
/// [`Error::Io`] naming the file or directory that could not be created or written, and
/// [`ServiceError::ConfigRejected`] carrying what the validator said.
pub async fn judge(
    directory: &Path,
    documents: &[Document],
    validator: Option<&Validator>,
) -> Result<()> {
    let staging = staging_for(directory).await?;

    let judged = match stage(&staging, documents).await {
        Ok(()) => match validator {
            Some(validator) => validator.judge(&staging).await,
            None => Ok(()),
        },
        Err(error) => Err(error),
    };

    // Unconditionally, and before the answer is returned: this function installs nothing, so a
    // staging directory left behind is the only trace it could leave at all.
    if let Err(error) = tokio::fs::remove_dir_all(&staging).await {
        tracing::warn!(
            path = %staging.display(),
            %error,
            "the staging directory of a judgement could not be removed"
        );
    }

    judged
}

/// Every file in a swept directory that no document in this set owns.
///
/// **Files only.** A swept directory holds one rendering per site and nothing else, so a
/// subdirectory in one is something this build did not put there — it is said and left, because a
/// recursive delete driven by a relative path out of a recipe is a much worse thing to be wrong
/// about than a directory nobody reads.
/// What [`install`] would change, without changing anything.
///
/// **A read in the strict sense**: no staging directory, no validator, no directory created.
/// `install` creates what it installs into and runs a checker over a staged set; neither may happen
/// on the path `mix doctor` calls, whose whole guarantee is that it writes nothing.
///
/// The two halves are both needed and neither sees the other's answer. A per-document comparison
/// cannot see a site file the recipe stopped rendering, and a sweep cannot see a file whose contents
/// moved.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Drift {
    /// The relative paths whose rendering differs from what is on disk, or that are not there.
    pub changed: Vec<PathBuf>,

    /// The absolute paths of files the recipe no longer renders and the sweep would remove.
    pub removable: Vec<PathBuf>,
}

impl Drift {
    /// Is the directory already what this set of documents says it should be?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.removable.is_empty()
    }
}

/// What [`install`] would do to `directory`, asked without doing it.
///
/// The reason `mix doctor` can ask whether a generated file still matches its row: the question can
/// only be answered by rendering again and comparing — generated configuration is never parsed back
/// — and answering it must not install what it finds.
///
/// # Errors
///
/// [`Error::Io`] naming the file or directory that could not be read.
pub async fn drift(directory: &Path, documents: &[Document], swept: &[&str]) -> Result<Drift> {
    let mut changed = Vec::new();

    for document in documents {
        if compare(&directory.join(document.relative()), document)
            .await?
            .changed()
        {
            changed.push(document.relative().to_path_buf());
        }
    }

    Ok(Drift {
        changed,
        removable: orphans(directory, documents, swept).await?,
    })
}

async fn orphans(directory: &Path, documents: &[Document], swept: &[&str]) -> Result<Vec<PathBuf>> {
    if swept.is_empty() {
        return Ok(Vec::new());
    }

    let owned: BTreeSet<PathBuf> = documents
        .iter()
        .map(|document| directory.join(document.relative()))
        .collect();

    let mut orphans = Vec::new();

    for relative in swept {
        let sweeping = directory.join(relative);

        let read = |source| Error::Io {
            action: "read the generated directory at",
            path: sweeping.clone(),
            source,
        };

        // A swept directory that is not there holds nothing to sweep, which is what a front end with
        // no sites looks like on its first render.
        let mut entries = match tokio::fs::read_dir(&sweeping).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(read(source)),
        };

        while let Some(entry) = entries.next_entry().await.map_err(read)? {
            let path = entry.path();

            if owned.contains(&path) {
                continue;
            }

            if entry.file_type().await.map_err(read)?.is_file() {
                orphans.push(path);
            } else {
                tracing::warn!(
                    path = %path.display(),
                    "something that is not a file is in a directory MixEngine sweeps; it is left \
                     where it is"
                );
            }
        }
    }

    orphans.sort();

    Ok(orphans)
}

/// What installing `document` at `target` would do.
async fn compare(target: &Path, document: &Document) -> Result<Written> {
    match tokio::fs::read_to_string(target).await {
        Ok(existing) if existing == document.contents() => Ok(Written::Unchanged),
        Ok(_) => Ok(Written::Updated),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Written::Created),

        // A file that is there and cannot be read — a permission, a directory where a file belongs,
        // bytes that are not UTF-8 — is deliberately *not* treated as "different, write over it".
        // Reading is the only evidence there is that the thing being replaced is a file this build
        // wrote, and a caller that overwrote whatever it could not read would be one bad path away
        // from replacing something that matters.
        Err(source) => Err(Error::Io {
            action: "read the generated file at",
            path: target.to_path_buf(),
            source,
        }),
    }
}

/// How many names a render will try before it gives up looking for one of its own.
///
/// Far above the number of renders that can be in flight — the daemon reaches this through
/// `Registry::graph`, and fifteen handlers all rendering at once would be a machine in trouble for
/// reasons that have nothing to do with this. What the ceiling is really for is the case where the
/// directory beside `etc/` cannot be written at all: without it that failure is an endless loop
/// rather than an error naming the path.
const NAMES: u32 = 128;

/// Claim a staging directory for this render, and nothing but this render.
///
/// A sibling and not a subdirectory: a subdirectory would be inside the set a validator is shown
/// and inside whatever a later task sweeps. Leading dot so that a person looking at `etc/` sees
/// their configuration and not the machinery, and a suffix as well, because the name has to be one
/// no service id can produce — [`ServiceId`] refuses both a leading dot and an interior one outside
/// an instance name.
///
/// **One name per render rather than one per service, and the filesystem is what hands it out.**
/// There used to be a single `.{service}.staging`, opened by deleting it, and two renders of one
/// service that overlapped destroyed each other three different ways: the second deleted the set the
/// first was handing to its validator, so `caddy validate` reported a configuration missing; or the
/// first came back to commit and found the directory gone; or — on Windows, where a file being read
/// cannot be unlinked — the second could not delete it at all and failed with a sharing violation.
/// A Windows CI leg produced the first of those twice, on unrelated branches, before this was
/// written. Nothing serialises the renders: `Registry::graph` is reached from about fifteen places,
/// each spawned and joined by nobody.
///
/// **`create_dir` and not "check, then create", because the check is the bug.** Directory creation
/// is the one exclusion the filesystem gives for free — exactly one caller is told `Ok`, every other
/// gets `AlreadyExists` — so a render owns the name it created and no lock above it is needed to say
/// so. Two renders now write two sets, validate two sets and commit the same bytes over each other,
/// which is what "rendering the same state twice" already means everywhere else in this module.
///
/// A crash between here and the removal at the end of [`install`] leaves one directory behind, and
/// the next render steps over it to the next number. That is untidy rather than wrong: it is a name
/// no service can have, holding a rendering nobody installed.
///
/// [`ServiceId`]: mixengine_proto::ServiceId
async fn staging_for(directory: &Path) -> Result<PathBuf> {
    let name = directory.file_name().map_or_else(
        || "service".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );

    // The service's own directory may not exist yet — a first render creates it below — but the one
    // holding it has to, because that is where this is about to create something.
    if let Some(parent) = directory.parent() {
        create_dir(parent).await?;
    }

    for attempt in 0..NAMES {
        let candidate = directory.with_file_name(format!(".{name}.staging.{attempt}"));

        match tokio::fs::create_dir(&candidate).await {
            Ok(()) => return Ok(candidate),
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(Error::Io {
                    action: "create the staging directory at",
                    path: candidate,
                    source,
                });
            }
        }
    }

    Err(Error::Io {
        action: "find an unused staging directory beside",
        path: directory.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("{NAMES} names beside this directory are all taken"),
        ),
    })
}

/// Write every document into `staging`, which [`staging_for`] has just created empty.
///
/// Nothing is cleared here any more. This used to open by deleting the directory, because the name
/// was shared and what a previous failed render had left in it was a configuration nobody
/// validated — and a validator shown a mixture of two renders would be judging a file set that never
/// existed. A name this render created cannot hold anybody else's work, so the mixture is gone along
/// with the deletion that was there to prevent it.
async fn stage(staging: &Path, documents: &[Document]) -> Result<()> {
    for document in documents {
        let path = staging.join(document.relative());

        if let Some(parent) = path.parent() {
            create_dir(parent).await?;
        }

        tokio::fs::write(&path, document.contents())
            .await
            .map_err(|source| Error::Io {
                action: "stage the generated file at",
                path,
                source,
            })?;
    }

    Ok(())
}

/// Move one staged file onto its final name.
async fn commit(staged: &Path, target: &Path) -> Result<()> {
    let io = |source| Error::Io {
        action: "install the generated file at",
        path: target.to_path_buf(),
        source,
    };

    match tokio::fs::rename(staged, target).await {
        Ok(()) => Ok(()),

        // The Windows case, and the one place this is not atomic. A rename replaces an existing
        // file everywhere this runs — except while another process holds that file open without
        // sharing deletion, which on Windows is most of them. Removing it first opens a window in
        // which the config is absent; that is strictly better than the alternative, which is a
        // service left running the configuration of two releases ago with no error anybody sees.
        Err(_) if target.exists() => {
            tokio::fs::remove_file(target).await.map_err(io)?;
            tokio::fs::rename(staged, target).await.map_err(io)
        }

        Err(source) => Err(io(source)),
    }
}

/// [`crate::paths::create_dir`], without blocking the runtime.
async fn create_dir(path: &Path) -> Result<()> {
    tokio::fs::create_dir_all(path)
        .await
        .map_err(|source| Error::Io {
            action: "create directory",
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(test)]
mod tests;
