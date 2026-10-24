//! What the machine's hosts file currently says MixEngine put in it.

use std::path::PathBuf;

use mixengine_proto::privileged::HostEntry;

use crate::Result;

/// The managed block, read.
///
/// **Read-only, and that is the whole trait.** Adding and removing entries cannot live here: they
/// need an administrative token, and a capability the daemon can call is by definition one it holds
/// no token for. The write is
/// [`PrivilegedOp::HostsApply`](mixengine_proto::privileged::PrivilegedOp::HostsApply), applied by
/// `mixengine-elevate`. Reading needs no privilege on any of the three systems.
///
/// Three callers, one of them already here: the producer decides whether a change is worth a prompt
/// by comparing this against what the database says, `domain.dns_status` answers "is there a hosts
/// entry?" with it, and `mix doctor` reconciles against it.
pub trait HostsFile: std::fmt::Debug + Send + Sync {
    /// Where this OS keeps the file, whether or not it is there.
    fn path(&self) -> PathBuf;

    /// The entries in one of MixEngine's blocks — the block tagged `tag`, which is one home's, or the
    /// legacy block for [`None`] — or why it cannot be read.
    ///
    /// An empty vector is a machine with no such block, which is not a failure.
    ///
    /// # Errors
    ///
    /// [`Error::Io`](crate::Error::Io) when the file cannot be read, and
    /// [`Error::MalformedBlock`](crate::Error::MalformedBlock) when the block cannot be read without
    /// guessing at what somebody else meant.
    fn managed(&self, tag: Option<&str>) -> Result<Vec<HostEntry>>;

    /// Every tag a block in the file carries — one per home that has written one (ADR 0066).
    ///
    /// # Errors
    ///
    /// [`Error::Io`](crate::Error::Io) when the file cannot be read.
    fn tags(&self) -> Result<Vec<String>>;
}
