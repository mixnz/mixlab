//! Sharing a site on the local network: the URL and the mDNS name it answers to, and its sharing
//! written and read.

use crate::{Error, ProjectError, Result, Store};

use super::rows::record;
use super::{Sharing, SiteRecord};

/// The URL a phone opens, for a site shared at `address` and answered on `port`.
///
/// **HTTP.** The certificate does cover this address, but a phone will not trust this home's
/// authority until it has installed it from the address the authority is downloaded at, and a URL a
/// person is told to open must be one that opens.
///
/// The port is omitted when it is 80, because a URL a person reads off a terminal and types into a
/// phone is a URL worth keeping short.
#[must_use]
pub fn shared_url(address: std::net::Ipv4Addr, port: u16) -> String {
    match port {
        80 => format!("http://{address}"),
        other => format!("http://{address}:{other}"),
    }
}

/// The mDNS name a shared site answers to: `<slug>-mixengine.local`.
///
/// **One label before `.local`, and that is not a style choice.** mDNS conventions single-label
/// host names under `.local` (RFC 6762 section 3) and Windows' resolver enforces the convention:
/// measured on 2026-08-31, `blog-mixengine.local` resolved to the shared address while
/// `blog.mixengine.local` answered *DNS name does not exist* — same responder, same interface, same
/// minute. That measurement is where the roadmap's and the feature spec's own spelling of this name
/// was overturned.
///
/// **Not `<slug>.local`**, which would also resolve. The flat `.local` namespace is shared with
/// every printer and phone on the Wi-Fi; `-mixengine` is what makes this our name rather than a
/// claim on somebody else's.
///
/// [`None`] where the primary domain has no usable label, which is what stops this ever answering
/// `-mixengine.local`.
#[must_use]
pub fn shared_name(primary: &str) -> Option<String> {
    let label = primary.split('.').next().unwrap_or(primary);

    crate::domains::slug(label).map(|slug| format!("{slug}-mixengine.local"))
}

/// The shared site already answering to `name`, if one is.
///
/// **Only shared sites collide.** The name is on the network while a site is shared and nowhere
/// else, so `blog.test` and `blog.dev` are an ordinary pair right up until somebody shares the
/// second one. `except` is the site being asked about, which must not collide with itself when it
/// is re-shared or when its domains are updated.
///
/// What comes back is the namesake's primary domain, because the refusal names the site somebody
/// has to unshare.
#[must_use]
pub fn name_taken<'a>(records: &'a [SiteRecord], except: i64, name: &str) -> Option<&'a str> {
    records
        .iter()
        .filter(|record| record.id != except && record.sharing.is_some())
        .find(|record| {
            record
                .domains
                .first()
                .and_then(|primary| shared_name(primary))
                .is_some_and(|taken| taken == name)
        })
        .and_then(|record| record.domains.first().map(String::as_str))
}

/// The port this home's front end answers HTTP on, or 80 where it declares none.
///
/// **The answer port and not the bound one**: on macOS a front end listens on 8080 behind a
/// packet-filter redirect, and what a browser asks for is still 80. A LAN URL is what somebody
/// types, so it carries the number they type.
///
/// # Errors
///
/// [`Error::Database`] when the tables cannot be read.
pub async fn web_port(store: &Store, catalogue: &crate::generate::Catalogue) -> Result<u16> {
    let Some(front_end) = crate::services::front_end::held_by(store, catalogue).await? else {
        return Ok(80);
    };

    let port = sqlx::query_scalar!("SELECT port FROM services WHERE id = ?", front_end)
        .fetch_optional(store.pool())
        .await
        .map_err(|source| store.failure("read", source))?
        .flatten();

    Ok(port.and_then(|port| u16::try_from(port).ok()).unwrap_or(80))
}

/// The three sharing columns as one value, or the refusal for a row the trigger should have made
/// impossible.
///
/// **An unparsable address is a refusal and not a shrug.** The alternative — treating it as not
/// shared — would silently un-share a site whose listener is up and whose firewall rule is open,
/// which is the one direction this must never fail in.
pub(super) fn read_sharing(
    site: i64,
    interface: Option<String>,
    address: Option<String>,
    since: Option<i64>,
    until: Option<i64>,
) -> Result<Option<Sharing>> {
    let (Some(interface), Some(address), Some(since)) = (interface, address, since) else {
        return Ok(None);
    };

    let parsed = address.parse::<std::net::Ipv4Addr>().map_err(|_| {
        Error::Project(ProjectError::UnreadableSiteRow {
            site,
            column: "shared_address",
            value: address.clone(),
        })
    })?;

    Ok(Some(Sharing {
        interface,
        address: parsed,
        since: mixengine_proto::Timestamp(since),
        until: until.map(mixengine_proto::Timestamp),
    }))
}

/// Write, or clear, a site's sharing.
///
/// All three columns move together, which is what the trigger in `0001_initial.sql` holds and
/// what [`Sharing`] makes unrepresentable otherwise. [`None`] is the unshare.
///
/// # Errors
///
/// [`Error::Database`] when the row cannot be written.
pub async fn set_sharing(store: &Store, id: i64, sharing: Option<&Sharing>) -> Result<SiteRecord> {
    let interface = sharing.map(|sharing| sharing.interface.clone());
    let address = sharing.map(|sharing| sharing.address.to_string());
    let since = sharing.map(|sharing| sharing.since.0);
    let until = sharing
        .and_then(|sharing| sharing.until)
        .map(|until| until.0);

    sqlx::query!(
        "UPDATE sites
         SET shared_interface = ?1, shared_address = ?2, shared_since = ?3, shared_until = ?4
         WHERE id = ?5",
        interface,
        address,
        since,
        until,
        id
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?;

    record(store, id).await
}
