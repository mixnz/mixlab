//! What a start repairs before the first client: installs an earlier home left, the services
//! installed runtimes should have, their activation, their generated `conf.d`, and jobs a
//! previous daemon never finished.

use std::sync::Arc;

use mixengine_core::{Paths, Store};

use super::Shared;
use crate::services::repairs::Occasion;
use crate::{adopt, background, services};

/// **What an earlier home left on disk, and marked, is recorded first.** Before the repairs below,
/// so a recorded PHP gets its pool from them like any other.
#[warn(clippy::cognitive_complexity)]
pub(super) async fn repair_home(
    shared: &Shared<'_>,
    background: &background::Background,
) -> mixengine_core::adopt::walk::Walked {
    let adopted = record_what_an_earlier_home_left(shared).await;

    // **Every installed runtime gets the service its recipe says it should have.** Idempotent and
    // run here as well as after an install, which is what gives a PHP installed by an earlier build
    // its pool with no data migration and repairs a home whose row somebody removed by hand.
    // Nothing here fails the start, on the same rule the two blocks around it follow: a runtime
    // with no service is one command away from having one, where refusing to start would leave the
    // user with no daemon at all.
    services::repairs::give_runtimes_their_pools(shared.store, Occasion::Start).await;

    hold_activation_in_background(shared, background);
    rewrite_every_conf_d(shared).await;
    close_abandoned_jobs(shared).await;

    adopted
}

/// Record what an earlier home left, and give `bin/` the commands of what was recorded.
#[warn(clippy::cognitive_complexity)]
async fn record_what_an_earlier_home_left(
    shared: &Shared<'_>,
) -> mixengine_core::adopt::walk::Walked {
    let adopted = adopt::offline(shared.store, shared.paths).await;

    // `bin/` was filled by `projections::refresh_bin` from the rows as they were, and a runtime
    // recorded here is one whose commands it does not hold yet.
    if !adopted.claimed.is_empty()
        && let Err(error) = shared.shims.runtimes_changed().await
    {
        tracing::warn!(%error, "bin/ could not be refreshed after recording installs");
    }

    adopted
}

/// **Both halves of on-demand activation, and spawned rather than awaited.** A port is allocated
/// for every service whose activator needs one, and then an address is held for each, for as long
/// as this daemon runs.
///
/// **Spawned because of where this sits**, and that is not a preference. The endpoint is bound and
/// the accept loop is still below, which is the window `serve`'s note on `bin/` warns about in as
/// many words: every moment spent here is a moment a queued client waits for its answer. Awaiting
/// these two was measured, before the pipe backlog turned that wait from a refusal into a queue,
/// failing three `test (windows-latest)` legs in a row, in three different suites, each on `All
/// pipe instances are busy` — because between them they are a full render pass and a bind per
/// service, which is precisely the "nineteen file copies" that block says must not go here.
///
/// Being a moment late costs nothing it could cost: nothing dials an activator until a site is
/// being served, and a site is served by a front end this daemon has not started yet.
#[warn(clippy::cognitive_complexity)]
fn hold_activation_in_background(shared: &Shared<'_>, background: &background::Background) {
    background.spawn(
        "activation ports",
        wake_what_was_left_down(
            Arc::clone(shared.services),
            shared.paths.clone(),
            shared.store.clone(),
        ),
    );
}

/// The spawned half of [`hold_activation_in_background`].
#[warn(clippy::cognitive_complexity)]
async fn wake_what_was_left_down(services: Arc<services::Registry>, paths: Paths, store: Store) {
    if !services::repairs::give_activation_ports(&store, Occasion::Start).await {
        return;
    }

    services::repairs::hold_activation_addresses(
        Arc::clone(&services),
        &paths,
        &store,
        Occasion::Start,
    )
    .await;

    // **What the last daemon left down is still down.** Without this a database stopped before a
    // restart is unreachable for ever: its row says stopped, nothing holds its address, and the
    // next client is refused by the kernel with no daemon anywhere in the story.
    //
    // **Every stop nobody meant to last, and not only the idle ones.** This walk always asked
    // `hold_if_wakeable`, and what that reads is what widened: a service the last daemon stopped on
    // its way out, or whose process went away with the machine, is one nothing decided to leave
    // down, so a connection may have it back. Which is the ordinary case on a laptop, and it used
    // to be the case that got nothing.
    //
    // Inside this task and not before it, for the reason the task is spawned.
    match mixengine_core::services::records(&store).await {
        Ok(records) => {
            for id in records.keys() {
                let Ok(service) = mixengine_proto::ServiceId::parse(id) else {
                    continue;
                };

                crate::services::hold::hold_if_wakeable(&services, &service).await;
            }

            tracing::debug!(
                services = services.holder().holding(),
                "holding the own address of each service nobody meant to leave down"
            );
        }
        Err(error) => tracing::warn!(
            %error,
            "the rows could not be read, so nothing the last daemon idled is wakeable"
        ),
    }
}

/// **And every installed runtime's ini set**, on the same policy as `bin/`: `etc/` is a projection
/// of the database, so it is rebuilt here rather than trusted, and a home whose `etc/php/` was
/// deleted is repaired by starting the daemon. Nothing here fails the start either.
#[warn(clippy::cognitive_complexity)]
async fn rewrite_every_conf_d(shared: &Shared<'_>) {
    match mixengine_core::runtimes::extensions::refresh_all(shared.store, shared.paths).await {
        Ok(moved) if moved.is_empty() => {
            tracing::debug!("every installed runtime's conf.d is up to date");
        }
        Ok(moved) => {
            tracing::info!(runtimes = ?moved, "rewrote the generated conf.d of installed runtimes");
        }
        Err(error) => tracing::warn!(%error, "could not rebuild every installed runtime's conf.d"),
    }
}

/// **The other half of recovery, and it needs no OS reading at all.** A service is a process that
/// can outlive the daemon that spawned it, which is why adoption asks the OS what survived; the
/// work behind a job is a task *inside* this process, so a row still saying `running` means one
/// thing only: the daemon doing it stopped. There is nothing to adopt and nothing to signal, only a
/// row to close, and it is closed as a failure because nobody asked for the work to stop.
///
/// Before the first client for the same reason as the rest of the start's repairs: a `job.list`
/// answered before this ran would show work nobody is doing.
#[warn(clippy::cognitive_complexity)]
async fn close_abandoned_jobs(shared: &Shared<'_>) {
    match mixengine_core::jobs::abandon(
        shared.store,
        mixengine_proto::Timestamp::from_system_time(std::time::SystemTime::now()),
    )
    .await
    {
        Ok(abandoned) if abandoned.is_empty() => {
            tracing::debug!("no jobs were left unfinished by a previous daemon");
        }
        Ok(abandoned) => tracing::info!(jobs = abandoned.len(), "closed jobs nobody is doing"),

        // Nothing here fails the start, on the same rule the service half follows: a row that could
        // not be closed leaves one job a user can see and act on, where refusing to start would
        // leave them with no daemon at all.
        Err(error) => tracing::warn!(%error, "could not close the jobs a previous daemon left"),
    }
}
