//! The file every install leaves in its own directory — spec D1.
//!
//! It is the row as it was recorded, minus the path (which is where the file is) and minus what a
//! person changes later: the default, extension choices and pins stay in SQLite.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mixengine_proto::{PackageChannel, PackageVersion, RuntimeKind};
use serde::{Deserialize, Serialize};

use super::Subject;

/// The marker's file name, inside the install directory.
pub const FILE_NAME: &str = ".mixengine-install.json";

/// The layout this build writes and reads. A different number is no marker.
pub const SCHEMA: u32 = 1;

/// What a marker says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "what", rename_all = "snake_case")]
pub enum Marker {
    /// A language runtime.
    Runtime(RuntimeMarker),
    /// A service package.
    Package(PackageMarker),
}

/// A runtime's row, as [`crate::runtimes::remember`] was given it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeMarker {
    /// Which language.
    pub kind: RuntimeKind,
    /// Which version.
    pub version: PackageVersion,
    /// Which channel the index published it on.
    pub channel: PackageChannel,
    /// Which URL it came from.
    pub url: String,
    /// The hash the index published for it.
    pub sha256: String,
    /// How large the archive was.
    pub bytes: u64,
    /// What its executables are called, and where they are inside the directory.
    pub provides: BTreeMap<String, String>,
    /// Where it keeps loadable extensions, relative to the directory.
    #[serde(default)]
    pub extension_dir: Option<String>,
    /// What it offers.
    #[serde(default)]
    pub extensions: crate::index::Extensions,
}

/// A package's row, as [`crate::packages::remember`] was given it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageMarker {
    /// Which package, by the name a recipe is found under.
    pub package: String,
    /// Which version.
    pub version: PackageVersion,
    /// Which URL it came from.
    pub url: String,
    /// The hash the index published for it.
    pub sha256: String,
    /// How large the archive was.
    pub bytes: u64,
    /// What its executables are called, and where they are inside the directory.
    pub provides: BTreeMap<String, String>,
}

/// The file as written: the schema beside the marker, so a reader can refuse a layout it does not
/// know before trusting any field of it.
#[derive(Serialize, Deserialize)]
struct Envelope {
    schema: u32,
    #[serde(flatten)]
    marker: Marker,
}

impl Marker {
    /// The marker for a runtime about to be recorded.
    #[must_use]
    pub fn runtime(installation: &crate::runtimes::Installation) -> Self {
        Self::Runtime(RuntimeMarker {
            kind: installation.kind,
            version: installation.version.clone(),
            channel: installation.channel,
            url: installation.url.clone(),
            sha256: installation.sha256.clone(),
            bytes: installation.bytes,
            provides: installation.provides.clone(),
            extension_dir: installation.extension_dir.clone(),
            extensions: installation.extensions.clone(),
        })
    }

    /// The marker for a package about to be recorded.
    #[must_use]
    pub fn package(installation: &crate::packages::Installation) -> Self {
        Self::Package(PackageMarker {
            package: installation.package.clone(),
            version: installation.version.clone(),
            url: installation.url.clone(),
            sha256: installation.sha256.clone(),
            bytes: installation.bytes,
            provides: installation.provides.clone(),
        })
    }

    /// The bytes of the file.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let envelope = Envelope {
            schema: SCHEMA,
            marker: self.clone(),
        };

        // Serialising plain data cannot fail; an empty file is what a reader treats as no marker,
        // which is the harmless answer if it ever did.
        let mut bytes = serde_json::to_vec_pretty(&envelope).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }

    /// Whether this marker is about exactly that install.
    #[must_use]
    pub fn names(&self, subject: &Subject) -> bool {
        match (self, subject) {
            (Self::Runtime(marker), Subject::Runtime { kind, version }) => {
                marker.kind == *kind && marker.version == *version
            }
            (Self::Package(marker), Subject::Package { package, version }) => {
                marker.package == *package && marker.version == *version
            }
            _ => false,
        }
    }

    /// The hash the marker records.
    #[must_use]
    pub fn sha256(&self) -> &str {
        match self {
            Self::Runtime(marker) => &marker.sha256,
            Self::Package(marker) => &marker.sha256,
        }
    }

    /// Which URL the marker says the install came from.
    #[must_use]
    pub fn url(&self) -> &str {
        match self {
            Self::Runtime(marker) => &marker.url,
            Self::Package(marker) => &marker.url,
        }
    }

    /// What the marker says the directory holds.
    #[must_use]
    pub fn provides(&self) -> &BTreeMap<String, String> {
        match self {
            Self::Runtime(marker) => &marker.provides,
            Self::Package(marker) => &marker.provides,
        }
    }
}

impl RuntimeMarker {
    /// The row to record for the directory this marker sits in.
    #[must_use]
    pub fn installation(&self, path: PathBuf) -> crate::runtimes::Installation {
        crate::runtimes::Installation {
            kind: self.kind,
            version: self.version.clone(),
            channel: self.channel,
            path,
            bytes: self.bytes,
            url: self.url.clone(),
            sha256: self.sha256.clone(),
            provides: self.provides.clone(),
            extension_dir: self.extension_dir.clone(),
            extensions: self.extensions.clone(),
        }
    }
}

impl PackageMarker {
    /// The row to record for the directory this marker sits in.
    #[must_use]
    pub fn installation(&self, path: PathBuf) -> crate::packages::Installation {
        crate::packages::Installation {
            package: self.package.clone(),
            version: self.version.clone(),
            path,
            bytes: self.bytes,
            url: self.url.clone(),
            sha256: self.sha256.clone(),
            provides: self.provides.clone(),
        }
    }
}

/// The marker in `dir`, when there is one this build can read.
///
/// Missing, unreadable, unparseable and a schema from another build are all [`None`]: each means
/// "check this directory the way one without a marker is checked", and none is a reason to stop.
#[must_use]
pub fn read(dir: &Path) -> Option<Marker> {
    let path = dir.join(FILE_NAME);
    let bytes = std::fs::read(&path).ok()?;

    // The schema first, on its own: a newer layout may not parse as this one at all, and "from
    // another build" is the more useful thing to log than the field it tripped on.
    let schema = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|value| value.get("schema").and_then(serde_json::Value::as_u64));
    if schema != Some(u64::from(SCHEMA)) {
        tracing::debug!(path = %path.display(), ?schema, "a marker this build does not read");
        return None;
    }

    match serde_json::from_slice::<Envelope>(&bytes) {
        Ok(envelope) => Some(envelope.marker),
        Err(error) => {
            tracing::debug!(path = %path.display(), %error, "a marker that does not parse");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use mixengine_proto::{PackageVersion, RuntimeKind};

    use crate::adopt::Subject;

    fn a_node() -> crate::runtimes::Installation {
        crate::runtimes::Installation {
            kind: RuntimeKind::Node,
            version: PackageVersion::parse("24.19.0").expect("a version"),
            channel: mixengine_proto::PackageChannel::Stable,
            path: PathBuf::from("/runtimes/node/24.19.0"),
            bytes: 31_457_280,
            url: "https://example.invalid/node.zip".to_owned(),
            sha256: "ab".repeat(32),
            provides: [("node".to_owned(), "node.exe".to_owned())]
                .into_iter()
                .collect(),
            extension_dir: None,
            extensions: crate::index::Extensions::default(),
        }
    }

    /// What goes in comes out, and the path is the directory's rather than the file's.
    #[test]
    fn a_runtime_marker_reads_back_as_the_installation_it_was_written_from() {
        let temp = tempfile::tempdir().expect("a directory");
        std::fs::write(
            temp.path().join(FILE_NAME),
            Marker::runtime(&a_node()).encode(),
        )
        .expect("write");

        let Some(Marker::Runtime(read)) = read(temp.path()) else {
            panic!("a runtime marker");
        };
        let back = read.installation(temp.path().to_path_buf());

        assert_eq!(back.version.as_str(), "24.19.0");
        assert_eq!(back.sha256, "ab".repeat(32));
        assert_eq!(
            back.provides.get("node").map(String::as_str),
            Some("node.exe")
        );
        assert_eq!(back.path, temp.path());
    }

    /// A marker that is not one is no marker, never a panic.
    #[test]
    fn an_empty_or_truncated_marker_is_no_marker() {
        let temp = tempfile::tempdir().expect("a directory");

        for garbage in [
            &b""[..],
            b"{}",
            b"{\"schema\":1,\"what\":\"runtime\",\"kind\":",
        ] {
            std::fs::write(temp.path().join(FILE_NAME), garbage).expect("write");
            assert_eq!(
                read(temp.path()),
                None,
                "{}",
                String::from_utf8_lossy(garbage)
            );
        }
    }

    /// A schema this build does not know is no marker: guessing at a newer layout would record a row
    /// from fields that may mean something else.
    #[test]
    fn a_marker_from_a_newer_schema_is_no_marker() {
        let temp = tempfile::tempdir().expect("a directory");
        let written = String::from_utf8(Marker::runtime(&a_node()).encode()).expect("utf-8");
        std::fs::write(
            temp.path().join(FILE_NAME),
            written.replace("\"schema\": 1", "\"schema\": 2"),
        )
        .expect("write");

        assert_eq!(read(temp.path()), None);
    }

    /// A marker names one version, and a directory named after another is not it.
    #[test]
    fn a_marker_names_its_own_version_and_no_other() {
        let marker = Marker::runtime(&a_node());

        assert!(marker.names(&Subject::Runtime {
            kind: RuntimeKind::Node,
            version: PackageVersion::parse("24.19.0").expect("a version"),
        }));
        assert!(!marker.names(&Subject::Runtime {
            kind: RuntimeKind::Node,
            version: PackageVersion::parse("22.0.0").expect("a version"),
        }));
        assert!(!marker.names(&Subject::Runtime {
            kind: RuntimeKind::Php,
            version: PackageVersion::parse("24.19.0").expect("a version"),
        }));
    }
}
