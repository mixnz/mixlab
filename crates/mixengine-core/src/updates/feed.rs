//! What a release says about itself — roadmap task **T88**.
//!
//! **A third [`Document`](crate::index::Document) and not a second client.** The three properties an
//! update feed needs are the three [`crate::index::Client`] already does and already has tests for:
//! the signature is checked before the bytes are parsed, so a JSON parser never runs on unverified
//! input; the cache is re-verified on every read, because a file in the user's home is a file any
//! local process can rewrite; and a correctly signed document from before the one we hold is
//! refused. That last one is the attack that matters here — somebody who can answer the URL replays
//! yesterday's feed to keep a machine on a version with a known hole — and getting it for free is
//! the whole argument for the shape of this module.
//!
//! Nothing here is `#[serde(deny_unknown_fields)]`, on [`crate::index::format`]'s rule and for its
//! reason: this document is written by us and read by builds older than it, so a release that adds
//! a field must stay readable by every copy already installed.

use crate::index::{Arch, Artifact, Os, Timestamp};

/// The document version this build can read.
///
/// Bumped only for a change an existing client *cannot* read. Adding an optional field is not one.
pub const SCHEMA: u32 = 1;

/// Where the feed is published.
///
/// The stable release-asset redirect and **not** the GitHub API, which is rate-limited to 60
/// requests an hour per address for unauthenticated callers — `docs/features/updates.md`. A
/// *draft* release is not `latest`, which is what makes tagging and publishing two different acts
/// and is why the release workflow assembles a draft somebody publishes by hand.
pub const DEFAULT_URL: &str =
    "https://github.com/mixnz/mixlab/releases/latest/download/latest.json";

/// One published release, and where its payload is for each machine.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Feed {
    /// The document version. Checked against [`SCHEMA`] before anything else here is believed.
    pub schema: u32,

    /// When the release pipeline wrote this document.
    ///
    /// **Two fields and not one, with [`Feed::version`].** `version` is what is offered; this is
    /// what makes a replay detectable, and a feed re-published for the same version — a corrected
    /// note, an added architecture — must be able to move forward without pretending to be a new
    /// release.
    pub generated_at: Timestamp,

    /// The version being offered.
    pub version: String,

    /// When that release was published, for a person to read.
    pub published_at: Timestamp,

    /// What changed, as the tag's own commit subjects.
    ///
    /// Written by `packaging/feed.sh` from `git log` rather than by GitHub: this document is signed
    /// before the draft release exists, so `--generate-notes` cannot reach it. See the T88 design,
    /// D13.
    pub notes: String,

    /// Where the notes somebody may have edited afterwards live.
    ///
    /// Optional because nothing breaks when it is absent — it is a link printed after the notes —
    /// and because a feed generated before this field existed must still read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_url: Option<String>,

    /// One payload per machine this release was built for.
    ///
    /// [`Artifact`] itself rather than a type of this module's own, which is what lets
    /// [`crate::install::Installer`] apply one unchanged: `provides` says where the binaries are
    /// inside the archive, `sha256` is what binds the payload to this signed document, and
    /// `requires` carries the glibc floor the packaging pipeline already measures. See the T88
    /// design, D3.
    pub artifacts: Vec<Artifact>,

    /// One `mixengine-elevate` per machine — roadmap task **T88a**.
    ///
    /// `default`, so a feed published before this field existed still reads and [`SCHEMA`] does not
    /// move: this module's own rule is that adding an optional field is not a bump.
    #[serde(default)]
    pub helpers: Vec<HelperArtifact>,

    /// One installer per machine — roadmap task **T88f**. Today only the macOS `.pkg`, listed under
    /// both architectures, which a copy the `.pkg` installed is updated with (ADR 0050).
    ///
    /// `default`, so a feed published before this field existed still reads and [`SCHEMA`] does not
    /// move, on [`Feed::helpers`]' rule.
    #[serde(default)]
    pub installers: Vec<InstallerArtifact>,
}

/// Where one machine's `mixengine-elevate` is published — roadmap task **T88a**.
///
/// **Its own release asset rather than a file inside the payload archive.** The signing key exists
/// only in the `release` job, after every build leg has uploaded, so nothing signed can be inside an
/// artifact a build leg produced — and a detached signature is precisely what the elevated process
/// needs in order to check a replacement for itself. See the T88a design, D6.
///
/// Not an [`Artifact`]: there is no archive, no `provides` and no `requires`, and the check is the
/// `.minisig` beside the file rather than a SHA-256 inside this document — which is the one place
/// `docs/features/updates.md` says the feed's own rule deliberately does not extend to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HelperArtifact {
    /// Which operating system this build is for.
    pub os: Os,

    /// Which architecture. macOS publishes one universal file listed under both rows.
    pub arch: Arch,

    /// Where the binary is. Its signature is this plus `.minisig`, the way minisign names one.
    pub url: String,

    /// How big it is, for the sentence a person reads before it is fetched.
    pub size: u64,

    /// The helper's own version, which is not the release's — roadmap task **T182b**, D1.
    ///
    /// Optional on the wire, so a feed from before T182b still reads: there the helper carried the
    /// release's version, and [`None`] means exactly that.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Where one machine's installer is published — roadmap task **T88f**.
///
/// **Bound by a SHA-256 inside this signed document**, as an [`Artifact`] is (T88's D3), and unlike
/// a [`HelperArtifact`], whose detached signature is for the elevated process to check. No second
/// key-handling path is added (ADR 0050, decision 2).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InstallerArtifact {
    /// Which operating system this installer is for.
    pub os: Os,

    /// Which architecture. The universal `.pkg` is listed under both Mac rows.
    pub arch: Arch,

    /// What kind of installer it is: `pkg`.
    pub kind: String,

    /// Where it is.
    pub url: String,

    /// How big it is, for the sentence a person reads before it is fetched.
    pub size: u64,

    /// What its bytes hash to, which is what binds it to this signed document.
    pub sha256: String,

    /// `window` or `headless` — T182b, D5. [`None`] is the window's, which is all a feed from
    /// before T182b listed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flavour: Option<String>,
}

impl InstallerArtifact {
    /// The download pipeline's shape for this file: one file, providing nothing, requiring nothing.
    ///
    /// What lets [`crate::install::Installer`] fetch it with the resume and the checksum every
    /// other download has, rather than a second download path.
    #[must_use]
    pub fn as_artifact(&self) -> Artifact {
        Artifact {
            os: self.os,
            arch: self.arch,
            url: self.url.clone(),
            sha256: self.sha256.clone(),
            size: self.size,
            provides: std::collections::BTreeMap::new(),
            requires: crate::index::Requires::default(),
            extension_dir: None,
            extensions: crate::index::Extensions::default(),
        }
    }
}

impl Feed {
    /// The payload for one machine, or [`None`] when this release has no build for it.
    ///
    /// macOS publishes one *universal* archive under two rows, one per architecture, so a caller
    /// asks with the pair it already has rather than learning what "universal" means — the T88
    /// design, D6.
    #[must_use]
    pub fn artifact(&self, os: Os, arch: Arch) -> Option<&Artifact> {
        self.artifacts
            .iter()
            .find(|artifact| artifact.os == os && artifact.arch == arch)
    }

    /// The privileged helper for one machine, or [`None`] when this release published none for it.
    ///
    /// A release from before T88a has an empty list and answers [`None`] for every pair, which is
    /// the honest answer: there is nothing to fetch, and the daemon leaves the helper as it is.
    #[must_use]
    pub fn helper(&self, os: Os, arch: Arch) -> Option<&HelperArtifact> {
        self.helpers
            .iter()
            .find(|helper| helper.os == os && helper.arch == arch)
    }

    /// The installer for one machine, or [`None`] when this release published none for it — roadmap
    /// task **T88f**. A release from before it has an empty list and answers [`None`] for every pair.
    #[must_use]
    ///
    /// **And for the flavour this copy is** — T182b, D5: `headless` for a copy with no window, which
    /// the headless package installs, and the window's package otherwise. A row with no `flavour` is
    /// the window's, which is all a feed from before T182b published.
    ///
    /// `kind` is `pkg`, `deb` or `rpm`: a copy a package manager owns is updated by the next
    /// package of the same kind (`updates::placement::installer_kind`).
    pub fn installer(
        &self,
        os: Os,
        arch: Arch,
        kind: &str,
        headless: bool,
    ) -> Option<&InstallerArtifact> {
        self.installers.iter().find(|installer| {
            let theirs = installer.flavour.as_deref() == Some("headless");
            installer.os == os
                && installer.arch == arch
                && installer.kind == kind
                && theirs == headless
        })
    }
}

impl crate::index::Document for Feed {
    const SCHEMA: u32 = SCHEMA;
    const LABEL: &'static str = "update feed";
    const CACHE_FILE: &'static str = "latest.json";

    fn schema(&self) -> u32 {
        self.schema
    }

    fn generated_at(&self) -> Timestamp {
        self.generated_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The document the packaging pipeline writes, as `packaging/feed.sh` writes it.
    fn document() -> serde_json::Value {
        serde_json::json!({
            "schema": 1,
            "generated_at": "2026-09-05T09:12:00Z",
            "version": "0.2.0",
            "published_at": "2026-09-05T09:12:00Z",
            "notes": "feat(cli): mix self-update",
            "notes_url": "https://github.com/mixnz/mixlab/releases/tag/v0.2.0",
            "artifacts": [{
                "os": "windows",
                "arch": "x86_64",
                "url": "https://example.invalid/mixengine-0.2.0-windows-x86_64.zip",
                "sha256": "00",
                "size": 1,
                "provides": { "mix": "mixengine/mix.exe" }
            }]
        })
    }

    #[test]
    fn the_feed_reads_what_the_packaging_pipeline_writes() {
        let feed: Feed = serde_json::from_value(document()).expect("a feed");

        assert_eq!(feed.version, "0.2.0");
        assert_eq!(feed.schema, SCHEMA);
        assert!(feed.artifact(Os::Windows, Arch::X86_64).is_some());
        assert!(
            feed.artifact(Os::Linux, Arch::X86_64).is_none(),
            "a release with no build for this pair must answer None rather than the first row it \
             happens to hold"
        );
    }

    /// A field a later release adds must not stop this build reading the document — the rule
    /// `index/format.rs` states, and the reason there is no `deny_unknown_fields` here.
    #[test]
    fn a_field_this_build_does_not_know_is_ignored() {
        let mut document = document();
        document["channel"] = serde_json::json!("beta");

        serde_json::from_value::<Feed>(document).expect("an unknown field is ignored");
    }

    /// T88a. The helper is never swapped by an update and cannot be, so the release publishes it as
    /// its own asset and the signed feed is what names it.
    #[test]
    fn the_feed_names_a_helper_for_each_machine() {
        let mut document = document();
        document["helpers"] = serde_json::json!([{
            "os": "windows",
            "arch": "x86_64",
            "url": "https://example.invalid/mixengine-elevate-0.2.0-windows-x86_64.exe",
            "size": 812_345
        }]);

        let feed: Feed = serde_json::from_value(document).expect("a feed");

        let helper = feed
            .helper(Os::Windows, Arch::X86_64)
            .expect("the row for this machine");
        assert_eq!(helper.size, 812_345);
        assert_eq!(
            helper.version, None,
            "a feed from before T182b names no helper version"
        );
        assert!(
            feed.helper(Os::Linux, Arch::X86_64).is_none(),
            "a release with no helper for this pair answers None rather than the first row it holds"
        );
    }

    /// T182b, D1. The helper carries a version of its own, which is not the release's.
    #[test]
    fn a_helper_names_its_own_version() {
        let mut document = document();
        document["helpers"] = serde_json::json!([{
            "os": "linux",
            "arch": "x86_64",
            "url": "https://example.invalid/mixengine-elevate-0.1.1-linux-x86_64",
            "size": 812_345,
            "version": "0.1.1"
        }]);

        let feed: Feed = serde_json::from_value(document).expect("a feed");

        assert_eq!(
            feed.helper(Os::Linux, Arch::X86_64)
                .and_then(|helper| helper.version.as_deref()),
            Some("0.1.1")
        );
    }

    /// A feed written before that field existed still reads, which is why [`SCHEMA`] does not move:
    /// the rule this module states is that adding an optional field is not a bump.
    #[test]
    fn a_feed_with_no_helpers_still_reads() {
        let feed: Feed = serde_json::from_value(document()).expect("a feed");

        assert!(feed.helpers.is_empty());
        assert_eq!(feed.schema, SCHEMA);
    }

    /// `notes_url` is what a person follows when somebody edited the draft's notes after the tag
    /// was signed (the T88 design, D13). A feed written before that field existed still reads.
    #[test]
    fn a_feed_with_no_notes_url_still_reads() {
        let mut document = document();
        document
            .as_object_mut()
            .expect("an object")
            .remove("notes_url");

        let feed: Feed = serde_json::from_value(document).expect("a feed");
        assert_eq!(feed.notes_url, None);
    }

    #[test]
    fn a_feed_from_before_installers_still_reads_and_has_none() {
        let feed: Feed = serde_json::from_value(serde_json::json!({
            "schema": 1, "generated_at": "2026-09-05T09:12:00Z", "version": "0.0.9",
            "published_at": "2026-09-05T09:12:00Z", "notes": "", "artifacts": []
        }))
        .expect("a feed without installers reads");

        assert!(
            feed.installer(Os::Macos, Arch::Aarch64, "pkg", false)
                .is_none()
        );
    }

    /// T182b, D5. A copy is offered the package of its own flavour: the headless one when it has no
    /// window, the window's otherwise — and a row naming no flavour is the window's.
    #[test]
    fn each_flavour_is_offered_its_own_package() {
        let row = |flavour: Option<&str>, name: &str| {
            let mut row = serde_json::json!({
                "os": "macos", "arch": "aarch64", "kind": "pkg",
                "url": format!("https://example.invalid/{name}"),
                "size": 1, "sha256": "00"
            });
            if let Some(flavour) = flavour {
                row["flavour"] = serde_json::json!(flavour);
            }
            row
        };
        let feed: Feed = serde_json::from_value(serde_json::json!({
            "schema": 1, "generated_at": "2026-09-25T09:12:00Z", "version": "0.0.8",
            "published_at": "2026-09-25T09:12:00Z", "notes": "", "artifacts": [],
            "installers": [
                row(None, "mixlab-0.0.8-macos-universal.pkg"),
                row(Some("headless"), "mixengine-0.0.8-macos-universal-headless.pkg"),
            ]
        }))
        .expect("a feed with both flavours reads");

        let url = |headless| {
            feed.installer(Os::Macos, Arch::Aarch64, "pkg", headless)
                .map(|installer| installer.url.clone())
                .expect("a row")
        };
        assert!(url(false).ends_with("mixlab-0.0.8-macos-universal.pkg"));
        assert!(url(true).ends_with("-headless.pkg"));
    }

    /// One universal `.pkg`, listed under both Mac rows as the payload is (the T88f design, D2).
    #[test]
    fn the_universal_pkg_is_found_under_either_mac_row() {
        let row = |arch: &str| {
            serde_json::json!({
                "os": "macos", "arch": arch, "kind": "pkg",
                "url": "https://example.invalid/mixlab-0.0.9-macos-universal.pkg",
                "size": 68_638_683,
                "sha256": "ce6d0802b50ac4f59108e4b2deebadbe404486a1cad5a35a7b0722923eb85072"
            })
        };
        let feed: Feed = serde_json::from_value(serde_json::json!({
            "schema": 1, "generated_at": "2026-09-05T09:12:00Z", "version": "0.0.9",
            "published_at": "2026-09-05T09:12:00Z", "notes": "", "artifacts": [],
            "installers": [row("x86_64"), row("aarch64")]
        }))
        .expect("a feed with installers reads");

        let installer = feed
            .installer(Os::Macos, Arch::Aarch64, "pkg", false)
            .expect("the aarch64 row");
        assert_eq!(installer.kind, "pkg");
        assert_eq!(installer.as_artifact().sha256, installer.sha256);
        assert_eq!(installer.as_artifact().url, installer.url);
        assert!(
            feed.installer(Os::Linux, Arch::X86_64, "deb", false)
                .is_none()
        );
    }

    /// The fixture MixLab's own reader is tested against (T187, spec D2). A field this reader
    /// starts needing that the fixture lacks, or a shape it stops accepting, fails here as well as
    /// there.
    #[test]
    fn the_fixture_mixlab_reads_is_a_feed_this_reader_reads() {
        let fixture = include_str!("../../../../packaging/testdata/latest.json");
        let feed: Feed = serde_json::from_str(fixture).expect("the shared fixture parses");

        assert_eq!(feed.schema, SCHEMA);
        assert_eq!(feed.version, "9.9.9");
        assert!(feed.artifact(Os::Windows, Arch::X86_64).is_some());
    }
}
