//! Moving a service that already has a port onto another one.
//!
//! **A port moves only when a person moves it**, and this is the check that move is held to: the
//! same one [`ports::allocate`] makes of a number it is about to hand out, made of a number a person
//! named instead. Not held by any row or extension, and bindable on the row's own address right
//! now — and a number lost to a program MixEngine does not manage is reported with that program's
//! name, as at create.
//!
//! Inside [`ports::hold`], so check and write are one critical section with every
//! `service.create`: a create cannot be handed the number this move is writing.

use mixengine_platform::Host;
use mixengine_proto::ServiceId;

use super::ports;
use crate::generate::{Recipe, Role, Source};
use crate::{Error, Result, ServiceError, Store};

/// What a [`claim`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claimed {
    /// The service was already on that port; nothing was written.
    Unchanged,

    /// The row now holds the new port, and held `from` before.
    Moved {
        /// The port the row held before.
        from: u16,
    },
}

/// Whether a person may move this recipe's services to another port.
///
/// **A server installed from a package, with a port of its own, that sites are not reached
/// through.** A front end's 80 and 443 are what every site is reached through; a php-fpm pool's port
/// is internal, named in site files and by nobody a person types to; a service an extension
/// declared has no recipe in the catalogue at all, and its ports are its manifest's.
#[must_use]
pub fn port_movable(recipe: &dyn Recipe) -> bool {
    matches!(recipe.source(), Source::Package)
        && recipe.preferred_port().is_some()
        && !matches!(recipe.role(), Role::FrontEnd(_))
}

/// Move `service` to `port`, if that port is free to take.
///
/// # Errors
///
/// [`ServiceError::PortInvalid`] for port 0; [`Error::NotFound`] when no such service is declared;
/// [`ServiceError::ListensOnNoPort`] for a row with no port; [`ServiceError::PortHeld`] when a row
/// or an extension holds `port`, naming it; [`ServiceError::PortUnavailable`] when the port cannot
/// be bound on the row's address, naming what holds it as far as the OS will say; and the store's
/// own failure when the row cannot be read or written.
pub async fn claim(
    store: &Store,
    host: &dyn Host,
    service: &ServiceId,
    port: u16,
) -> Result<Claimed> {
    if port == 0 {
        return Err(Error::Service(ServiceError::PortInvalid { port }));
    }

    let _allocating = ports::hold().await;

    let id = service.as_str();
    let row = sqlx::query!(
        r#"SELECT port AS "port: i64", bind_addr FROM services WHERE id = ?"#,
        id
    )
    .fetch_optional(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?
    .ok_or_else(|| Error::NotFound {
        kind: "service",
        id: id.to_owned(),
    })?;

    let Some(from) = row.port.and_then(|port| u16::try_from(port).ok()) else {
        return Err(Error::Service(ServiceError::ListensOnNoPort {
            service: service.clone(),
        }));
    };

    if from == port {
        return Ok(Claimed::Unchanged);
    }

    if let Some((_, holder)) = ports::holders(store)
        .await?
        .into_iter()
        .find(|(held, _)| *held == i64::from(port))
    {
        return Err(Error::Service(ServiceError::PortHeld { port, holder }));
    }

    let bind = ports::bind_address(Some(row.bind_addr.as_str()));

    if !ports::bindable(bind, port) {
        let owner = ports::moved(host, port);

        return Err(Error::Service(ServiceError::PortUnavailable {
            port,
            pid: owner.pid,
            program: owner.program,
        }));
    }

    super::set_port(store, service, port).await?;

    Ok(Claimed::Moved { from })
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, TcpListener};
    use std::time::Duration;

    use mixengine_platform::{PortHolder, mock};
    use mixengine_proto::ServiceId;

    use super::*;
    use crate::services::ports::tests::{HOME, a_free_run, declared, held_by_an_extension, store};

    fn one() -> ServiceId {
        ServiceId::parse("one@main").expect("an id")
    }

    async fn port_of(store: &Store, id: &str) -> Option<i64> {
        sqlx::query_scalar::<_, Option<i64>>("SELECT port FROM services WHERE id = ?")
            .bind(id)
            .fetch_one(store.pool())
            .await
            .expect("a row")
    }

    /// The ordinary case: a free number, and the row says so afterwards.
    #[tokio::test]
    async fn claim_moves_a_row_to_a_free_port() {
        let (_home, store) = store().await;
        let first = a_free_run(2);
        declared(&store, "one", first).await;

        let claimed = claim(&store, &mock::Host::with_home(HOME), &one(), first + 1)
            .await
            .expect("a move");

        assert_eq!(claimed, Claimed::Moved { from: first });
        assert_eq!(
            port_of(&store, "one@main").await,
            Some(i64::from(first + 1))
        );
    }

    /// Asking for the port it already has is an answer, not a refusal by its own row.
    #[tokio::test]
    async fn claim_on_the_current_port_changes_nothing() {
        let (_home, store) = store().await;
        let first = a_free_run(1);
        declared(&store, "one", first).await;

        let claimed = claim(&store, &mock::Host::with_home(HOME), &one(), first)
            .await
            .expect("an answer");

        assert_eq!(claimed, Claimed::Unchanged);
    }

    /// Another row's port is refused by that row's name, even with nothing listening on it.
    #[tokio::test]
    async fn claim_refuses_a_port_another_row_holds_and_names_it() {
        let (_home, store) = store().await;
        let first = a_free_run(2);
        declared(&store, "one", first).await;
        declared(&store, "two", first + 1).await;

        let refused = claim(&store, &mock::Host::with_home(HOME), &one(), first + 1)
            .await
            .expect_err("a port another row holds");

        assert!(
            matches!(
                &refused,
                crate::Error::Service(ServiceError::PortHeld { port, holder })
                    if *port == first + 1 && holder == "two@main"
            ),
            "{refused:?}"
        );
        assert_eq!(port_of(&store, "one@main").await, Some(i64::from(first)));
    }

    /// An extension's port is held as surely as a row's.
    #[tokio::test]
    async fn claim_refuses_a_port_an_extension_holds() {
        let (_home, store) = store().await;
        let first = a_free_run(2);
        declared(&store, "one", first).await;
        held_by_an_extension(&store, "mailpit", "smtp_port", first + 1).await;

        let refused = claim(&store, &mock::Host::with_home(HOME), &one(), first + 1)
            .await
            .expect_err("a port an extension holds");

        assert!(
            matches!(
                &refused,
                crate::Error::Service(ServiceError::PortHeld { holder, .. }) if holder == "mailpit"
            ),
            "{refused:?}"
        );
    }

    /// And so is another service's activator.
    #[tokio::test]
    async fn claim_refuses_an_activation_port() {
        let (_home, store) = store().await;
        let first = a_free_run(3);
        declared(&store, "one", first).await;
        declared(&store, "two", first + 1).await;
        sqlx::query("UPDATE services SET activation_port = ? WHERE id = 'two@main'")
            .bind(i64::from(first + 2))
            .execute(store.pool())
            .await
            .expect("an activation port");

        let refused = claim(&store, &mock::Host::with_home(HOME), &one(), first + 2)
            .await
            .expect_err("an activator's port");

        assert!(
            matches!(
                &refused,
                crate::Error::Service(ServiceError::PortHeld { holder, .. }) if holder == "two@main"
            ),
            "{refused:?}"
        );
    }

    /// A program MixEngine does not manage is named, and the row is left where it was.
    #[tokio::test]
    async fn claim_refuses_a_port_something_else_has_bound() {
        let (_home, store) = store().await;
        let first = a_free_run(2);
        declared(&store, "one", first).await;
        let _squatter = TcpListener::bind((Ipv4Addr::LOCALHOST, first + 1)).expect("a squatter");
        let host = mock::Host::with_a_port_held(
            HOME,
            first + 1,
            PortHolder {
                pid: Some(4120),
                name: Some("mysqld.exe".to_owned()),
            },
        );

        let refused = claim(&store, &host, &one(), first + 1)
            .await
            .expect_err("a port somebody is listening on");

        assert!(
            matches!(
                &refused,
                crate::Error::Service(ServiceError::PortUnavailable { port, pid: Some(4120), program: Some(program) })
                    if *port == first + 1 && program == "mysqld.exe"
            ),
            "{refused:?}"
        );
        assert_eq!(port_of(&store, "one@main").await, Some(i64::from(first)));
    }

    /// Zero is a request for "any port", which no client could ever be told.
    #[tokio::test]
    async fn claim_refuses_port_zero() {
        let (_home, store) = store().await;
        declared(&store, "one", a_free_run(1)).await;

        let refused = claim(&store, &mock::Host::with_home(HOME), &one(), 0)
            .await
            .expect_err("port zero");

        assert!(
            matches!(
                &refused,
                crate::Error::Service(ServiceError::PortInvalid { port: 0 })
            ),
            "{refused:?}"
        );
    }

    /// The bind is tried on the address the row listens on, not on loopback by assumption.
    #[tokio::test]
    async fn claim_tests_the_rows_own_bind_address() {
        let (_home, store) = store().await;
        let first = a_free_run(2);
        declared(&store, "one", first).await;
        sqlx::query("UPDATE services SET bind_addr = '0.0.0.0' WHERE id = 'one@main'")
            .execute(store.pool())
            .await
            .expect("a bind address");
        let _squatter = TcpListener::bind((Ipv4Addr::UNSPECIFIED, first + 1)).expect("a squatter");

        let refused = claim(&store, &mock::Host::with_home(HOME), &one(), first + 1)
            .await
            .expect_err("a port taken on the row's own address");

        assert!(
            matches!(
                &refused,
                crate::Error::Service(ServiceError::PortUnavailable { .. })
            ),
            "{refused:?}"
        );
    }

    /// An unknown id is a `NotFound`, the same refusal every other setter gives.
    #[tokio::test]
    async fn claim_on_a_service_nobody_declared_is_not_found() {
        let (_home, store) = store().await;

        let refused = claim(&store, &mock::Host::with_home(HOME), &one(), a_free_run(1))
            .await
            .expect_err("no such row");

        assert!(
            matches!(refused, crate::Error::NotFound { .. }),
            "{refused:?}"
        );
    }

    /// **Check and write are one critical section with `service.create`'s allocation**, or a
    /// create could be handed the number this move is about to write.
    #[tokio::test]
    async fn claim_waits_for_a_create_in_flight() {
        let (_home, store) = store().await;
        let first = a_free_run(2);
        declared(&store, "one", first).await;

        let held = crate::services::ports::hold().await;
        let moving = tokio::spawn({
            let store = store.clone();
            async move { claim(&store, &mock::Host::with_home(HOME), &one(), first + 1).await }
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!moving.is_finished(), "the move did not wait for the lock");

        drop(held);
        let claimed = moving.await.expect("the task").expect("a move");
        assert_eq!(claimed, Claimed::Moved { from: first });
    }

    /// Which catalogue rows a person may move: every server with a port of its own, and nothing
    /// that sites are reached through or that a runtime owns.
    #[test]
    fn only_a_packaged_server_with_a_port_may_move() {
        let catalogue = crate::generate::Catalogue::builtin();
        let movable =
            |package: &str| port_movable(catalogue.recipe(package).expect("a recipe").as_ref());

        for package in [
            "mariadb",
            "mysql",
            "postgres",
            "redis",
            "memcached",
            "mongodb",
        ] {
            assert!(movable(package), "{package} should be movable");
        }
        for package in ["caddy", "nginx", "php-fpm"] {
            assert!(!movable(package), "{package} should not be movable");
        }
    }
}
