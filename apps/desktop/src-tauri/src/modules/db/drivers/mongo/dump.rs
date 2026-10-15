//! Backing up and restoring a MongoDB database with `mongodump` and `mongorestore`.
//!
//! The child process, its progress and its cancellation are [`super::super::dump`]'s. These two
//! tools take a URI and nothing else, so its password is on their command line: a limit of the
//! tools, not a choice made here.

use super::super::dump::{open_file, run, share, Fed, Invocation, Progress, Watch};
use crate::error::AppError;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Splits a MongoDB URI into the parts this module rewrites: what is before the host list, the
/// host list itself, the `/database` path, and the `?options`.
fn split_uri(uri: &str) -> Result<(String, String, String, String), AppError> {
    let (scheme, rest) = uri
        .split_once("://")
        .ok_or_else(|| err!("error.notMongoUri"))?;
    let (credentials, hosts_and_more) = match rest.rsplit_once('@') {
        Some((credentials, rest)) => (format!("{credentials}@"), rest),
        None => (String::new(), rest),
    };
    let (hosts_and_path, query) = match hosts_and_more.split_once('?') {
        Some((head, query)) => (head, format!("?{query}")),
        None => (hosts_and_more, String::new()),
    };
    let (hosts, path) = match hosts_and_path.split_once('/') {
        Some((hosts, path)) => (hosts, format!("/{path}")),
        None => (hosts_and_path, String::new()),
    };
    Ok((
        format!("{scheme}://{credentials}"),
        hosts.to_string(),
        path,
        query,
    ))
}

/// The URI the tools are given: the database path dropped, since the database is named separately
/// and the tools refuse to be told twice, and the host replaced when the connection is tunneled.
///
/// The `/` that stood before the dropped database stays: a URI whose options follow the host list
/// with nothing between them is one the tools refuse to parse at all.
fn tool_uri(uri: &str, endpoint: Option<(&str, u16)>) -> Result<String, AppError> {
    let (head, hosts, _path, query) = split_uri(uri)?;
    let Some((host, port)) = endpoint else {
        return Ok(format!("{head}{hosts}/{query}"));
    };
    if head.starts_with("mongodb+srv://") {
        return Err(err!("error.srvOverTunnel"));
    }
    // Only the one host is forwarded, so the tools must talk to it directly rather than discover
    // the replica set's own addresses and try to reach those.
    let separator = if query.is_empty() { "?" } else { "&" };
    Ok(format!(
        "{head}{host}:{port}/{query}{separator}directConnection=true"
    ))
}

/// Writes `database` to `path` as a mongodump archive: the whole database, collections and
/// indexes, in one file rather than a directory of BSON.
///
/// `documents` is what `$collStats` says the database's documents weigh, and is where the progress
/// reported through `watch` comes from — see [`archive_share`]. Zero when the server would not say,
/// which costs the dump nothing but its percentage.
pub fn dump(
    tool: &Path,
    uri: &str,
    endpoint: Option<(&str, u16)>,
    database: &str,
    path: &str,
    documents: u64,
    watch: &Watch<'_>,
) -> Result<(), AppError> {
    let args = vec![
        format!("--uri={}", tool_uri(uri, endpoint)?),
        format!("--db={database}"),
        format!("--archive={path}"),
    ];
    let archive = PathBuf::from(path);
    run(
        Invocation {
            tool,
            args: &args,
            env: &[],
            stdin: None,
            stdout: None,
            name: "mongodump",
        },
        watch,
        |_| {
            let written = std::fs::metadata(&archive)
                .map(|meta| meta.len())
                .unwrap_or(0);
            (watch.report)(Progress {
                percent: archive_share(written, documents),
                ..Progress::default()
            });
        },
    )
}

/// How big a mongodump archive comes out against what the documents in it weigh.
///
/// One, as near as makes no difference: an archive is those documents' own BSON, which is exactly
/// what `$collStats` measures, and the headers around them are a few hundred bytes per namespace.
/// Measured against this server twice — 200,066,402 bytes of archive for 200,065,450 of documents,
/// and 50,018,402 for 50,017,450. Named rather than left implicit because it is an assumption about
/// a file format, and the one place to correct if a future mongodump stops holding to it.
const ARCHIVE_PER_DOCUMENT: f64 = 1.0;

/// How far through the archive a dump is, from the bytes on disk against what the documents weigh.
///
/// This is the whole of mongodump's progress, and it is deliberately not read out of what mongodump
/// says. Its own account is a bar drawn every three seconds, and it dumps four collections at once
/// unless told otherwise — so the collection it names is one of several in flight, and counting
/// them would report a dump further along than it is. The file on disk is under no such doubt.
///
/// It does arrive in steps rather than smoothly: mongodump holds about 16MB back before writing,
/// so a 200MB dump climbs in a dozen jumps and a small one in two or three. The bigger the dump the
/// less that shows, which is the right way round — a dump small enough for the steps to be coarse
/// is over in seconds.
fn archive_share(written: u64, documents: u64) -> Option<u8> {
    if documents == 0 {
        return None;
    }
    share(written, (documents as f64 * ARCHIVE_PER_DOCUMENT) as u64)
}

/// The magic number every mongodump archive starts with.
const ARCHIVE_MAGIC: u32 = 0x8199_e26d;

/// The database a mongodump archive holds, read out of the archive itself.
///
/// An archive begins with its magic number, then a header document, then one metadata document per
/// collection — and each of those names the database it came from. Only the first is read: an
/// archive written by MixLab holds one database, being dumped with `--db`.
///
/// This is needed because `mongorestore` puts documents back into the namespaces the archive
/// names, and the only way to send them somewhere else is to tell it what to rename *from*.
fn archive_database(path: &str) -> Result<String, AppError> {
    use mongodb::bson::Document;
    use std::io::Read;

    let mut file = open_file(path)?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)
        .map_err(|e| err!("error.cannotReadFile", path = path, message = e))?;
    if u32::from_le_bytes(magic) != ARCHIVE_MAGIC {
        return Err(err!("error.notMongoArchive", path = path));
    }
    let unreadable = |e: mongodb::bson::de::Error| {
        err!("error.archiveDatabaseUnreadable", path = path, message = e)
    };
    // The header, which carries versions rather than namespaces.
    Document::from_reader(&mut file).map_err(unreadable)?;
    let metadata = Document::from_reader(&mut file).map_err(unreadable)?;
    metadata
        .get_str("db")
        .map(str::to_string)
        .map_err(|_| err!("error.archiveNamesNoDatabase", path = path))
}

/// Restores a mongodump archive into `database`, whatever database it was dumped from.
///
/// The rename is what makes the choice in the sidebar mean something: without it the documents go
/// back where they came from, which for an archive from another server is rarely where they are
/// wanted.
///
/// The archive is poured into mongorestore rather than named to it — `--archive` with no value is
/// how it is told to read one from its standard input. That costs nothing, since it reads the
/// archive from end to end either way, and it is what makes the progress reported through `watch` a
/// count of bytes actually handed over rather than anything inferred.
pub fn restore(
    tool: &Path,
    uri: &str,
    endpoint: Option<(&str, u16)>,
    database: &str,
    path: &str,
    watch: &Watch<'_>,
) -> Result<(), AppError> {
    let mut args = vec![
        format!("--uri={}", tool_uri(uri, endpoint)?),
        "--archive".to_string(),
    ];
    let source = archive_database(path)?;
    if source != database {
        // Every collection of the one database, and only that database: an archive holding more
        // than the dump wrote would otherwise have the rest restored under their own names.
        args.push(format!("--nsInclude={source}.*"));
        args.push(format!("--nsFrom={source}.*"));
        args.push(format!("--nsTo={database}.*"));
    }

    let file = open_file(path)?;
    // A file whose size cannot be read is still restored, just without a percentage to go with it.
    let total = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    let sent = Arc::new(AtomicU64::new(0));
    let counted = Arc::clone(&sent);
    let fed = Fed {
        file,
        path: path.to_string(),
        sent,
        lead: None,
    };

    run(
        Invocation {
            tool,
            args: &args,
            env: &[],
            stdin: Some(fed),
            stdout: None,
            name: "mongorestore",
        },
        watch,
        |_| {
            (watch.report)(Progress {
                percent: share(counted.load(Ordering::Relaxed), total),
                ..Progress::default()
            });
        },
    )
}

#[cfg(test)]
mod tests;
