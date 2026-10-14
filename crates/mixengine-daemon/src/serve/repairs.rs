//! What a start repairs before the first client: installs an earlier home left, the services
//! installed runtimes should have, their activation, their generated `conf.d`, and jobs a
//! previous daemon never finished.

use std::sync::Arc;

use super::Shared;
use crate::{adopt, background, services};

/// **What an earlier home left on disk, and marked, is recorded first** — roadmap task T182f.
/// Before the repairs below, so a recorded PHP gets its pool from them like any other.
pub(super) async fn repair_home(
    shared: &Shared<'_>,
    background: &mut background::Background,
) -> mixengine_core::adopt::walk::Walked {
    let Shared {
        paths,
        store,
        services,
        shims,
        ..
    } = *shared;

    let adopted = adopt::offline(store, paths).await;

    // `bin/` was filled a few hundred lines up from the rows as they were, and a runtime recorded
    // here is one whose commands it does not hold yet (T185b).
    if !adopted.claimed.is_empty()
        && let Err(error) = shims.runtimes_changed().await
    {
        tracing::warn!(%error, "bin/ could not be refreshed after recording installs");
    }

    // **Every installed runtime gets the service its recipe says it should have** — roadmap task
    // T32. Idempotent and run here as well as after an install, which is what gives a PHP installed
    // by an earlier build its pool with no data migration and repairs a home whose row somebody
    // removed by hand. Nothing here fails the start, on the same rule the two blocks around it
    // follow: a runtime with no service is one command away from having one, where refusing to start
    // would leave the user with no daemon at all.
    match mixengine_core::services::pools::ensure(
        store,
        mixengine_platform::host().as_ref(),
        &services::catalogue(),
    )
    .await
    {
        Ok(created) if created.is_empty() => {
            tracing::debug!("every installed runtime already has the service it needs");
        }
        Ok(created) => tracing::info!(pools = ?created, "installed runtimes were given services"),
        Err(error) => tracing::warn!(%error, "could not give every installed runtime its service"),
    }

    // **Both halves of on-demand activation, and spawned rather than awaited** — roadmap task T70.
    // A port is allocated for every service whose activator needs one, and then an address is held
    // for each, for as long as this daemon runs.
    //
    // **Spawned because of where this sits**, and that is not a preference. The endpoint is bound
    // and the accept loop is still below, which is the window the `bin/` block far above warns
    // about in as many words: every moment spent here is a moment a queued client waits for its
    // answer. Awaiting these two was measured, before T170's backlog turned that wait from a
    // refusal into a queue, failing three `test (windows-latest)` legs in a row, in three different
    // suites, each on `All pipe instances are busy` — because between them they are a full render
    // pass and a bind per service, which is precisely the "nineteen file copies" that block says
    // must not go here.
    //
    // Being a moment late costs nothing it could cost: nothing dials an activator until a site is
    // being served, and a site is served by a front end this daemon has not started yet.
    background.spawn("activation ports", {
        let services = Arc::clone(services);
        let paths = paths.clone();
        let store = store.clone();

        async move {
            let host = mixengine_platform::host();

            match mixengine_core::services::activation::ensure(
                &store,
                host.as_ref(),
                &crate::services::catalogue(),
            )
            .await
            {
                Ok(given) if given.is_empty() => {
                    tracing::debug!("every service that can be woken already has its port");
                }
                Ok(given) => {
                    tracing::info!(services = ?given, "services were given activation ports")
                }
                Err(error) => {
                    tracing::warn!(%error, "could not give every wakeable service a port");
                    return;
                }
            }

            match crate::services::activate::hold_all(
                Arc::clone(&services),
                &paths,
                &store,
                host.as_ref(),
            )
            .await
            {
                Ok(held) if held.is_empty() => {
                    tracing::debug!("no service in this home can be started by a request");
                }
                Ok(held) => {
                    tracing::info!(services = ?held, "holding an address for each of these")
                }
                Err(error) => {
                    tracing::warn!(%error, "could not hold an address for every wakeable service");
                }
            }

            // **What the last daemon left down is still down** — roadmap task **T70a**. Without
            // this a database stopped before a restart is unreachable for ever: its row says
            // stopped, nothing holds its address, and the next client is refused by the kernel with
            // no daemon anywhere in the story.
            //
            // **Every stop nobody meant to last, and not only the idle ones** — roadmap task
            // **T123**. This walk always asked `hold_if_wakeable`, and what that reads is what
            // widened: a service the last daemon stopped on its way out, or whose process went away
            // with the machine, is one nothing decided to leave down, so a connection may have it
            // back. Which is the ordinary case on a laptop, and it used to be the case that got
            // nothing.
            //
            // Inside this task and not before it, for the block above's reason.
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
    });

    // **And every installed runtime's ini set** — roadmap task T28, on the same policy as `bin/`
    // above: `etc/` is a projection of the database, so it is rebuilt here rather than trusted, and
    // a home whose `etc/php/` was deleted is repaired by starting the daemon. Nothing here fails the
    // start either.
    match mixengine_core::runtimes::extensions::refresh_all(store, paths).await {
        Ok(moved) if moved.is_empty() => {
            tracing::debug!("every installed runtime's conf.d is up to date");
        }
        Ok(moved) => {
            tracing::info!(runtimes = ?moved, "rewrote the generated conf.d of installed runtimes");
        }
        Err(error) => tracing::warn!(%error, "could not rebuild every installed runtime's conf.d"),
    }

    // **The other half of recovery, and it needs no OS reading at all** — roadmap task T22. A
    // service is a process that can outlive the daemon that spawned it, which is why the step above
    // asks the OS what survived; the work behind a job is a task *inside* this process, so a row
    // still saying `running` means one thing only: the daemon doing it stopped. There is nothing to
    // adopt and nothing to signal, only a row to close, and it is closed as a failure because nobody
    // asked for the work to stop.
    //
    // Before the first client for the same reason as above: a `job.list` answered before this ran
    // would show work nobody is doing.
    match mixengine_core::jobs::abandon(
        store,
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

    adopted
}
