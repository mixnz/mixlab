//! A hosts file that exists only in memory.

use std::path::PathBuf;

use mixengine_proto::privileged::HostEntry;

use crate::{Error, Result};

/// The tag [`Hosts::tags`] lists this home's own block under.
const OWN: &str = "0000000000000000";

/// The blocks a test says the machine is holding.
///
/// **One answer for every home's tag** (ADR 0066): a daemon under test asks only about its own block,
/// so `holding` is that home's, whatever its tag. The legacy block and the list of tags are set
/// apart, because those are the questions about other homes.
#[derive(Debug)]
pub(crate) struct Hosts {
    /// This home's block, already parsed — or why it cannot be read.
    managed: std::result::Result<Vec<HostEntry>, String>,

    /// The untagged block every home shared before each kept its own.
    legacy: Vec<HostEntry>,

    /// Every tag a block in the file carries.
    tags: Vec<String>,
}

impl Default for Hosts {
    /// A machine with an empty block, which is what a fresh mock should say.
    fn default() -> Self {
        Self {
            managed: Ok(Vec::new()),
            legacy: Vec::new(),
            tags: Vec::new(),
        }
    }
}

impl Hosts {
    /// A machine whose block holds `lines`, each written the way a hosts file writes one:
    /// `"127.0.0.1 blog.test"`.
    ///
    /// Parsed through [`crate::hosts::parse`] rather than taken as structs, so a fixture cannot
    /// describe a block the engine could not have produced.
    pub(crate) fn holding<'a>(lines: impl IntoIterator<Item = &'a str>) -> Self {
        let mut text = format!("{}\n", crate::hosts::BEGIN_MARKER);

        for line in lines {
            text.push_str(line);
            text.push('\n');
        }

        text.push_str(crate::hosts::END_MARKER);
        text.push('\n');

        Self {
            managed: crate::hosts::parse(&text, None).map_err(|error| error.to_string()),
            ..Self::default()
        }
    }

    /// A machine whose hosts file cannot be read, with `reason`.
    pub(crate) fn refusing(reason: &str) -> Self {
        Self {
            managed: Err(reason.to_owned()),
            ..Self::default()
        }
    }

    /// This machine, with a legacy block holding `lines`.
    pub(crate) fn with_legacy<'a>(self, lines: impl IntoIterator<Item = &'a str>) -> Self {
        Self {
            legacy: Self::holding(lines).managed.unwrap_or_default(),
            ..self
        }
    }

    /// This machine, with blocks under `tags`.
    pub(crate) fn with_tags<'a>(self, tags: impl IntoIterator<Item = &'a str>) -> Self {
        Self {
            tags: tags.into_iter().map(str::to_owned).collect(),
            ..self
        }
    }
}

impl crate::HostsFile for Hosts {
    fn path(&self) -> PathBuf {
        PathBuf::from("/mock/hosts")
    }

    fn managed(&self, tag: Option<&str>) -> Result<Vec<HostEntry>> {
        match tag {
            Some(_) => self
                .managed
                .clone()
                .map_err(|reason| Error::MalformedBlock { reason }),
            None => Ok(self.legacy.clone()),
        }
    }

    /// The tags set with [`Hosts::with_tags`], and — when this home's block holds anything — one
    /// standing in for it. A real file lists the home's own tag among the others, and the mock's own
    /// block answers to every tag, so any one tag finds it.
    fn tags(&self) -> Result<Vec<String>> {
        let mut tags = self.tags.clone();

        if self.managed.as_ref().is_ok_and(|own| !own.is_empty()) {
            tags.insert(0, OWN.to_owned());
        }

        Ok(tags)
    }
}
