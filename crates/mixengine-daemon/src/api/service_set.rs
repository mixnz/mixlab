//! `service.set`: moving a service that exists to another port.
//!
//! **A port moves only when a person moves it**, and this is where they do. The check and the row
//! are [`mixengine_core::services::claim`]'s, under the allocation lock every `service.create`
//! takes; what is here is the order around it, which is the whole of what a move can get wrong:
//!
//! 1. Refuse what may not move, and a service already on its way down, before anything changes.
//! 2. Claim the port. A refusal leaves everything as it was.
//! 3. A live service is stopped on the plan its old graph gives, and started on a graph rendered
//!    after the claim — so its configuration is rendered with the new port while it is down, and
//!    never handed to a running process as a reload a port change cannot apply.
//! 4. A stopped one is rendered and stays stopped; an address held for an idle stop moves with it.
//! 5. Extensions whose configuration names this database are written again.
//! 6. A start that fails on the new port puts the row back on the old one and starts it there.
//!
//! The D-numbers are those of `docs/specs/2026-10-25-t210-moving-a-service-to-another-port-design.md`.

use mixengine_core::generate::{Catalogue, Role, Source};
use mixengine_core::services::{Claimed, ServiceRecord};
use mixengine_proto::{
    Error, ErrorCode, PortChange, ServiceId, ServiceSet, ServiceSetResult, ServiceState,
};

use super::Api;
use crate::error::ToWire as _;
use crate::services;

impl Api {
    /// `service.set` — today, the port.
    ///
    /// # Errors
    ///
    /// `invalid_argument` for a request naming no setting and for a service whose port does not
    /// move (D2); `precondition_failed` for a front end and for a service that is stopping;
    /// `already_exists` for a port a row or an extension holds, and `precondition_failed` for one a
    /// program MixEngine does not manage has bound (D3); and the start's own failure when the new
    /// port would not start, after the service has been put back on its old one (D5).
    pub(super) async fn service_set(&self, asked: &ServiceSet) -> Result<ServiceSetResult, Error> {
        let Some(port) = asked.port else {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("nothing to set on {}", asked.service),
            )
            .with_hint("`mix service set <service> --port <port>`"));
        };

        let id = &asked.service;
        let catalogue = services::catalogue();
        let record = mixengine_core::services::record(&self.store, id)
            .await
            .map_err(|error| error.to_wire())?;

        movable(&catalogue, id, &record)?;

        let live = is_live(record.state) || self.services.supervised().contains(id);

        // Read before the claim, while the row still holds the old port: the stop plan and what
        // comes back up are about the service as it is running now.
        let before = match live {
            true => Some(
                self.services
                    .graph()
                    .await
                    .map_err(|error| error.to_wire())?,
            ),
            false => None,
        };

        let claimed = mixengine_core::services::claim(&self.store, self.services.host(), id, port)
            .await
            .map_err(|error| error.to_wire())?;

        let Claimed::Moved { from } = claimed else {
            return Ok(ServiceSetResult {
                service: self.moved_summary(&catalogue, id).await?,
                port: Some(PortChange {
                    from: port,
                    to: port,
                }),
                restarted: false,
                not_followed: Vec::new(),
            });
        };

        let restarted = match before {
            Some(graph) => {
                self.restart_on_the_new_port(&graph, id, from, port).await?;
                true
            }
            None => {
                self.services
                    .graph()
                    .await
                    .map_err(|error| error.to_wire())?;
                services::hold::rehold(&self.services, id).await;
                false
            }
        };

        // D4 step 5: a `web-app` whose generated file names this database. Logged, never fatal,
        // on `configure`'s own rule — the move has happened either way.
        self.extensions.configure().await;

        Ok(ServiceSetResult {
            service: self.moved_summary(&catalogue, id).await?,
            port: Some(PortChange { from, to: port }),
            restarted,
            not_followed: not_followed(from),
        })
    }

    /// Stop on the old graph, start on one rendered after the claim, and go back if that fails.
    async fn restart_on_the_new_port(
        &self,
        before: &mixengine_core::services::ServiceGraph,
        id: &ServiceId,
        from: u16,
        to: u16,
    ) -> Result<(), Error> {
        let down = before
            .stop_plan([id])
            .map_err(|error| mixengine_core::Error::Graph(error).to_wire())?;
        let roots = services::restarted(Some(id), &down, &self.services.supervised());

        if let Some((refused, fault)) = self.services.stop(&down).await.failed {
            // The row has moved and the process has not: put the row back, so the two agree.
            self.back_on_the_old_port(id, from).await;

            return Err(Error::new(
                ErrorCode::PreconditionFailed,
                format!("{refused} would not stop: {}", fault.said(&refused)),
            )
            .with_hint(format!("{id} is still on {from}")));
        }

        let Some((failed, fault)) = self.start_on_a_fresh_graph(&roots).await? else {
            return Ok(());
        };

        // D5: back on the old port, and started there.
        self.back_on_the_old_port(id, from).await;
        let again = self.start_on_a_fresh_graph(&roots).await?;

        let hint = match again {
            None => format!("{id} is back on {from} and running; {to} was not changed"),
            Some((second, _)) => {
                format!("{id} is back on {from}, but {second} did not start there either")
            }
        };

        Err(Error::new(ErrorCode::PreconditionFailed, fault.said(&failed)).with_hint(hint))
    }

    /// Render, and start `roots` and what they need. The service that failed, if one did.
    async fn start_on_a_fresh_graph(
        &self,
        roots: &[ServiceId],
    ) -> Result<Option<(ServiceId, services::Fault)>, Error> {
        let graph = self
            .services
            .graph()
            .await
            .map_err(|error| error.to_wire())?;
        let up = graph
            .start_plan(roots.iter())
            .map_err(|error| mixengine_core::Error::Graph(error).to_wire())?;

        Ok(self.services.start(&graph, &up).await.failed)
    }

    /// Write the old port back. Logged loudly when even that fails: the row then names a port the
    /// person was told it is not on, and `mix service status` is where they will see it.
    async fn back_on_the_old_port(&self, id: &ServiceId, from: u16) {
        if let Err(error) =
            mixengine_core::services::claim(&self.store, self.services.host(), id, from).await
        {
            tracing::error!(
                service = id.as_str(),
                port = from,
                %error,
                "a moved service could not be put back on its old port"
            );
        }
    }

    /// The service as the listing reports it.
    async fn moved_summary(
        &self,
        catalogue: &Catalogue,
        id: &ServiceId,
    ) -> Result<mixengine_proto::ServiceSummary, Error> {
        let graph = self
            .services
            .graph()
            .await
            .map_err(|error| error.to_wire())?;
        let record = mixengine_core::services::record(&self.store, id).await.ok();

        Ok(super::service::summary(
            &graph,
            catalogue,
            id,
            record.as_ref(),
            &self.services.supervised(),
        ))
    }
}

/// D2: refuse what may not move, in the words that say why. `Ok` for what may.
///
/// Also the refusal for a service on its way down: a stop is under way, and moving the port under
/// it would have the stop's last word overwritten by a start nobody asked for.
pub(super) fn movable(
    catalogue: &Catalogue,
    id: &ServiceId,
    record: &ServiceRecord,
) -> Result<(), Error> {
    let Some(recipe) = catalogue.recipe(id.name()) else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{id}'s ports are set by the extension that declared it"),
        ));
    };

    if matches!(recipe.role(), Role::FrontEnd(_)) {
        return Err(Error::new(
            ErrorCode::PreconditionFailed,
            format!(
                "{id} listens on 80 and 443, which every site is reached through; they do not move"
            ),
        ));
    }

    if matches!(recipe.source(), Source::Runtime(_)) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "{id}'s port is internal: sites reach it through the front end, and nothing a \
                 person types names it"
            ),
        ));
    }

    if !mixengine_core::services::port_movable(recipe.as_ref()) || record.port.is_none() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{id} listens on no port"),
        ));
    }

    if record.state == ServiceState::Stopping {
        return Err(
            Error::new(ErrorCode::PreconditionFailed, format!("{id} is stopping"))
                .with_hint("try again once it has stopped"),
        );
    }

    Ok(())
}

/// Whether a move has to restart it: anything up or on its way up.
fn is_live(state: ServiceState) -> bool {
    matches!(
        state,
        ServiceState::Running
            | ServiceState::Starting
            | ServiceState::Restarting
            | ServiceState::Degraded
    )
}

/// D1: what the daemon cannot change, in the sentences a client prints as they are.
fn not_followed(from: u16) -> Vec<String> {
    vec![
        format!("a project's .env that names {from}"),
        format!("a saved connection in MixLab that names {from}"),
        format!("shell history and scripts that name {from}"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(state: ServiceState, port: Option<u16>) -> ServiceRecord {
        ServiceRecord {
            state,
            last_failure: None,
            stopped_by: mixengine_core::services::StoppedBy::Never,
            pid: None,
            port,
            pid_start_time: None,
            last_started_at: None,
            last_exit_code: None,
            autostart: false,
            version: None,
        }
    }

    fn id(text: &str) -> ServiceId {
        ServiceId::parse(text).expect("an id")
    }

    fn refused(id_text: &str, state: ServiceState) -> Error {
        movable(
            &services::catalogue(),
            &id(id_text),
            &record(state, Some(3306)),
        )
        .expect_err("refused")
    }

    #[test]
    fn a_database_may_move() {
        assert!(
            movable(
                &services::catalogue(),
                &id("mysql@8"),
                &record(ServiceState::Stopped, Some(3307))
            )
            .is_ok()
        );
    }

    #[test]
    fn a_front_end_is_refused() {
        let error = refused("caddy", ServiceState::Stopped);

        assert_eq!(error.code, ErrorCode::PreconditionFailed);
        assert!(error.message.contains("80 and 443"), "{}", error.message);
    }

    #[test]
    fn a_pool_is_refused() {
        let error = refused("php-fpm@8.3.33", ServiceState::Stopped);

        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("internal"), "{}", error.message);
    }

    #[test]
    fn a_service_an_extension_declared_is_refused() {
        let error = refused("mailpit", ServiceState::Stopped);

        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("extension"), "{}", error.message);
    }

    #[test]
    fn a_stopping_service_is_refused() {
        let error = refused("mysql@8", ServiceState::Stopping);

        assert_eq!(error.code, ErrorCode::PreconditionFailed);
    }
}
