//! `index.json`: the package index as one document — roadmap task **T196**.
//!
//! What every MixEngine before T196 read, and what a source with no schema 2 set still serves (the
//! T196 design's D9). It is published beside the schema 2 set for as long as the packaging
//! repository generates it, and the two say the same thing: [`Index::from`] is where this one
//! becomes the view every caller holds.

use serde::Deserialize;

use super::format::{Index, Package, Timestamp};

/// The schema this document is.
///
/// Bumped only for a change an existing client *cannot* read. Adding an optional field is not one —
/// see [`super::format`]'s note on unknown fields.
pub const SCHEMA: u32 = 1;

/// The published document.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Document {
    /// The document version. Checked against [`SCHEMA`] before anything else is believed.
    pub schema: u32,

    /// When the publishing pipeline generated this document — what makes a rollback detectable.
    pub generated_at: Timestamp,

    /// Every version of every runtime and service, in the generator's order.
    pub packages: Vec<Package>,
}

impl super::Document for Document {
    const SCHEMA: u32 = SCHEMA;
    const LABEL: &'static str = "package index";
    const CACHE_FILE: &'static str = "index.json";

    fn schema(&self) -> u32 {
        self.schema
    }

    fn generated_at(&self) -> Timestamp {
        self.generated_at
    }
}

impl From<Document> for Index {
    fn from(document: Document) -> Self {
        Self::from_packages(document.generated_at, document.packages)
    }
}
