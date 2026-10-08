//! Index schema 2: one signed root and one file per kind — roadmap task **T196**.
//!
//! An encoding of schema 1 and nothing more: [`decode`] gives back the packages `index.json` lists,
//! value for value. The root names every kind file by sha256, which is what lets one signature
//! cover all of them — a kind file is believed only because a signed root states its hash.
//!
//! The reference is `tools/catalogue.py` in `mixengine-packages`. Where this is stricter than its
//! `decode_kind`, the T196 design's D12 says why; none of those places is reachable from what the
//! publisher's encoder writes.
//!
//! Like [`super::format`], nothing here is `#[serde(deny_unknown_fields)]`: a field this build does
//! not know is ignored, so adding an optional one is not a schema change.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::format::{Arch, Artifact, Channel, Extensions, Os, Package, Requires, Timestamp};

/// The schema both documents are.
pub const SCHEMA: u32 = 2;

/// The root's name. Its signature and every kind file sit beside it, wherever it was fetched from.
pub const ROOT: &str = "index-v2.json";

/// What a kind's file is called.
///
/// **Derived, not stated**: the root names a kind and its hash and never a location, so a mirror of
/// the index is a copy of the files and nothing in them has to change.
#[must_use]
pub fn kind_file(kind: &str) -> String {
    format!("index-v2-{kind}.json")
}

/// Whether `name` is a kind name: `^[a-z][a-z0-9-]*$`, the root schema's own pattern.
///
/// Asked before a name out of a document becomes part of a path or a URL.
#[must_use]
pub fn is_kind_name(name: &str) -> bool {
    let mut bytes = name.bytes();

    bytes.next().is_some_and(|first| first.is_ascii_lowercase())
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// The one signed document.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Root {
    /// The document version. Checked against [`SCHEMA`] before anything else is believed.
    pub schema: u32,

    /// When the publishing pipeline generated it — what makes a rollback detectable, as in schema 1.
    pub generated_at: Timestamp,

    /// Where artifacts live, stated once. An artifact is at
    /// `{base_url}/{kind}-{version}/{kind}-{version}-{os}-{arch}.{format}` and nowhere else.
    pub base_url: String,

    /// Every kind there is, and the exact bytes of its file. A kind that is not here does not exist.
    pub kinds: BTreeMap<String, Entry>,
}

/// What the root says about one kind file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Entry {
    /// Lowercase hex, of the file's exact bytes.
    pub sha256: String,

    /// Its length in bytes.
    pub size: u64,
}

/// Every version of one kind, as published.
#[derive(Debug, Clone, Deserialize)]
pub struct KindFile {
    /// The document version.
    pub schema: u32,

    /// Which kind the file says it is, so that one saved under the wrong name says so.
    pub kind: String,

    /// Everything an artifact says that is not about its bytes, referred to by position.
    shapes: Vec<Shape>,

    /// The versions, in the generator's order.
    packages: Vec<Release>,
}

/// What an artifact says that is not about its bytes.
///
/// **Only these five fields are read** (`lacks` since T206). The reference decoder spreads the shape over the artifact,
/// so a shape carrying `sha256` would win there; here a shape has nowhere to put one.
#[derive(Debug, Clone, Deserialize)]
struct Shape {
    provides: BTreeMap<String, String>,
    #[serde(default)]
    requires: Requires,
    #[serde(default)]
    extension_dir: Option<String>,
    #[serde(default)]
    extensions: Extensions,
    #[serde(default)]
    lacks: BTreeMap<String, String>,
}

/// One version, before its kind and its shapes are put back.
#[derive(Debug, Clone, Deserialize)]
struct Release {
    version: String,
    channel: Channel,
    #[serde(default)]
    eol: Option<String>,
    artifacts: Vec<Cell>,
}

/// One build: what it is for, what its bytes are, and which shape it has.
#[derive(Debug, Clone, Deserialize)]
struct Cell {
    os: Os,
    arch: Arch,

    /// The one part of an archive's name that cannot be derived. **A string and not an enum**: a
    /// format this build has never heard of is refused by the installer for that artifact, where an
    /// enum would make the whole kind unreadable.
    format: String,
    sha256: String,
    size: u64,
    shape: usize,
}

/// Whether `format` may end a URL: `^[a-z0-9.]+$`.
fn is_suffix(format: &str) -> bool {
    !format.is_empty()
        && format
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.')
}

/// The schema 1 packages a kind file stands for, with artifacts under `base_url`.
///
/// Nothing is percent-encoded and a trailing `/` on `base_url` is ignored, so the URL is byte for
/// byte the one schema 1 states.
///
/// # Errors
///
/// A sentence naming the artifact whose `shape` points past the table, or whose `format` is not a
/// file suffix.
pub fn decode(file: KindFile, base_url: &str) -> Result<Vec<Package>, String> {
    let base = base_url.trim_end_matches('/');
    let kind = file.kind;
    let mut packages = Vec::with_capacity(file.packages.len());

    for release in file.packages {
        let stem = format!("{kind}-{}", release.version);
        let mut artifacts = Vec::with_capacity(release.artifacts.len());

        for cell in release.artifacts {
            let (os, arch) = (cell.os.as_str(), cell.arch.as_str());

            let Some(shape) = file.shapes.get(cell.shape) else {
                return Err(format!(
                    "{stem} {os}/{arch} names shape {} and the file has {}",
                    cell.shape,
                    file.shapes.len()
                ));
            };
            if !is_suffix(&cell.format) {
                return Err(format!(
                    "{stem} {os}/{arch} has the format {:?}",
                    cell.format
                ));
            }

            artifacts.push(Artifact {
                os: cell.os,
                arch: cell.arch,
                url: format!("{base}/{stem}/{stem}-{os}-{arch}.{}", cell.format),
                sha256: cell.sha256,
                size: cell.size,
                provides: shape.provides.clone(),
                requires: shape.requires.clone(),
                extension_dir: shape.extension_dir.clone(),
                extensions: shape.extensions.clone(),
                lacks: shape.lacks.clone(),
            });
        }

        packages.push(Package {
            kind: kind.clone(),
            version: release.version,
            channel: release.channel,
            eol: release.eol,
            artifacts,
        });
    }

    Ok(packages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{Arch, Os};

    const BASE: &str = "https://github.com/mixnz/mixengine-packages/releases/download";

    fn php() -> serde_json::Value {
        serde_json::json!({
            "schema": 2, "kind": "php",
            "shapes": [
                { "provides": { "php": "bin/php" }, "requires": { "glibc": "2.35" },
                  "extension_dir": "ext",
                  "extensions": { "static": ["core"], "shared": ["xdebug"] } },
                { "provides": { "php": "php.exe" }, "requires": { "vcredist": "2019" } },
                { "provides": { "php": "bin/php" } }
            ],
            "packages": [{
                "version": "8.5.11", "channel": "stable", "eol": "2029-12-31",
                "artifacts": [
                    { "os": "linux", "arch": "x86_64", "format": "tar.zst",
                      "sha256": "aa", "size": 5, "shape": 0 },
                    { "os": "windows", "arch": "x86_64", "format": "zip",
                      "sha256": "bb", "size": 6, "shape": 1 },
                    { "os": "macos", "arch": "aarch64", "format": "tar.gz",
                      "sha256": "cc", "size": 7, "shape": 2 }
                ]
            }]
        })
    }

    fn read(value: serde_json::Value) -> KindFile {
        serde_json::from_value(value).expect("a kind file")
    }

    /// **`lacks` travels in the shape**, where the packaging repository's encoder puts every field
    /// that is not about the bytes — roadmap task **T206**, D1.
    #[test]
    fn a_shape_carries_what_its_cells_lack() {
        let mut file = php();
        file["shapes"][1]["lacks"] = serde_json::json!({ "native gems": "no compiler" });

        let packages = decode(read(file), BASE).expect("decodes");
        let windows = packages[0]
            .artifacts
            .iter()
            .find(|artifact| artifact.os == Os::Windows)
            .expect("the windows cell");

        assert_eq!(
            windows.lacks.get("native gems").map(String::as_str),
            Some("no compiler")
        );
        assert!(packages[0].artifacts[0].lacks.is_empty());
    }

    /// The whole claim of the format: an encoding of schema 1 and nothing more.
    #[test]
    fn a_kind_file_decodes_to_what_schema_1_says() {
        let packages = decode(read(php()), BASE).expect("decodes");

        let expected: Vec<Package> = serde_json::from_value(serde_json::json!([{
            "kind": "php", "version": "8.5.11", "channel": "stable", "eol": "2029-12-31",
            "artifacts": [
                { "os": "linux", "arch": "x86_64",
                  "url": format!("{BASE}/php-8.5.11/php-8.5.11-linux-x86_64.tar.zst"),
                  "sha256": "aa", "size": 5, "provides": { "php": "bin/php" },
                  "requires": { "glibc": "2.35" }, "extension_dir": "ext",
                  "extensions": { "static": ["core"], "shared": ["xdebug"] } },
                { "os": "windows", "arch": "x86_64",
                  "url": format!("{BASE}/php-8.5.11/php-8.5.11-windows-x86_64.zip"),
                  "sha256": "bb", "size": 6, "provides": { "php": "php.exe" },
                  "requires": { "vcredist": "2019" } },
                { "os": "macos", "arch": "aarch64",
                  "url": format!("{BASE}/php-8.5.11/php-8.5.11-macos-aarch64.tar.gz"),
                  "sha256": "cc", "size": 7, "provides": { "php": "bin/php" } }
            ]
        }]))
        .expect("the schema 1 view");

        assert_eq!(packages, expected);
    }

    #[test]
    fn a_trailing_slash_on_the_base_is_ignored() {
        let packages = decode(read(php()), &format!("{BASE}/")).expect("decodes");

        assert!(
            packages[0].artifacts[0]
                .url
                .starts_with(&format!("{BASE}/php-8.5.11/"))
        );
    }

    /// The reference decoder merges the shape over the artifact; this one must not — the T196
    /// design's D12.
    #[test]
    fn a_shape_cannot_overwrite_what_an_artifact_says_about_its_bytes() {
        let mut file = php();
        file["shapes"][0]["sha256"] = serde_json::json!("evil");
        file["shapes"][0]["size"] = serde_json::json!(1);
        file["shapes"][0]["url"] = serde_json::json!("https://evil.invalid/x.zip");
        file["shapes"][0]["os"] = serde_json::json!("windows");

        let packages = decode(read(file), BASE).expect("decodes");
        let artifact = &packages[0].artifacts[0];

        assert_eq!((artifact.sha256.as_str(), artifact.size), ("aa", 5));
        assert!(artifact.url.starts_with(BASE), "{}", artifact.url);
        assert_eq!((artifact.os, artifact.arch), (Os::Linux, Arch::X86_64));
    }

    #[test]
    fn a_shape_past_the_table_makes_the_kind_unreadable() {
        let mut file = php();
        file["packages"][0]["artifacts"][0]["shape"] = serde_json::json!(3);

        let refusal = decode(read(file), BASE).expect_err("no such shape");

        assert!(refusal.contains("shape 3"), "{refusal}");
    }

    #[test]
    fn a_format_this_build_has_never_heard_of_still_composes() {
        let mut file = php();
        file["packages"][0]["artifacts"][0]["format"] = serde_json::json!("tar.xz");

        let packages = decode(read(file), BASE).expect("decodes");

        assert!(
            packages[0].artifacts[0]
                .url
                .ends_with("-linux-x86_64.tar.xz")
        );
    }

    /// A format reaches a URL, so it is a suffix or it is refused.
    #[test]
    fn a_format_that_is_not_a_suffix_is_refused() {
        for format in ["zip/../../x", "tar zst", "", "ZIP", "zip?x=1"] {
            let mut file = php();
            file["packages"][0]["artifacts"][0]["format"] = serde_json::json!(format);

            assert!(
                decode(read(file), BASE).is_err(),
                "{format:?} should be refused"
            );
        }
    }

    #[test]
    fn a_kind_name_is_a_lowercase_word() {
        for name in ["php", "php-fpm", "a1"] {
            assert!(is_kind_name(name), "{name}");
        }
        for name in ["", "PHP", "1a", "a/b", "..", "a b", "a.b", "-a"] {
            assert!(!is_kind_name(name), "{name:?}");
        }
        assert_eq!(kind_file("php"), "index-v2-php.json");
    }

    #[test]
    fn a_root_reads_and_ignores_what_it_does_not_know() {
        let root: Root = serde_json::from_value(serde_json::json!({
            "schema": 2, "generated_at": "2026-09-30T15:09:10Z", "base_url": BASE,
            "kinds": { "php": { "sha256": "ab", "size": 30670, "later": true } },
            "mirrors": []
        }))
        .expect("a root");

        assert_eq!(root.kinds["php"].size, 30670);
        assert_eq!(root.generated_at.to_string(), "2026-09-30T15:09:10Z");
    }
}
