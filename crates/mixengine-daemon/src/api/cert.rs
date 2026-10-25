//! `cert.ca_status` — what this home's certificate authority is. The other `cert.*` methods
//! are arms in [`super::rpc`] over [`crate::certs`].

use mixengine_proto::{CaStatus, Error};

use super::Api;

impl Api {
    /// What this home's certificate authority is.
    ///
    /// Reads, and never makes: the making happens once at start, so that the trust-store install
    /// falls inside the same first-run elevation batch as the resolver and the port grant. A method
    /// that generated on demand would move that install behind a prompt of its own.
    pub(super) async fn ca_status(&self) -> Result<CaStatus, Error> {
        self.certificates.status().await
    }
}
