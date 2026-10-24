//! The loops a start leaves running on a clock of their own: certificate renewal, the idle
//! sweep, the sampler, and the watchdog that wakes on the sampler's minutes.

use std::sync::Arc;

use super::Shared;

/// **And from here a clock keeps doing it.** The issuance that ends
/// [`machine::certificates`](super::machine::certificates) was the only renewal this daemon had: a
/// leaf lives 90 days, so a machine switched off and on again stays current forever, and one whose
/// daemon runs for three months reaches a red padlock with no defect anywhere in the code.
///
/// Here rather than beside the DNS server so that it reads as what it is — the standing version
/// of that issuance — and before `api::Api::new`, which takes `events`.
pub(super) fn start_clocks(shared: &Shared<'_>) -> crate::metrics::sampler::Handle {
    let Shared {
        paths,
        store,
        config,
        host,
        events,
        services,
        shutdown,
        ..
    } = *shared;

    crate::certs::renewal::start(
        crate::certs::Certificates::issuing(paths, Arc::clone(host), store.clone()),
        Arc::clone(services),
        events.clone(),
        std::time::Duration::from_secs(config.certs.renew_check_seconds),
        shutdown.clone(),
    );

    // **And a second clock stops what nothing is using.** Beside the renewal one because they are
    // the same shape and have the same two constraints: after recovery, so a reading is never taken
    // of a service this daemon has not decided about yet, and before the API is served, so the
    // first sweep does not race the first client.
    //
    // **It sweeps what has a policy, and by default nothing does** (ADR 0041): a service is swept
    // where somebody ran `mix service idle`, or while the home saves resources, when php-fpm pools
    // and the servers a connection can start get a number of their own. Each of those has an
    // address held for it, and the request or connection that finds it down is what wakes it; a
    // service nothing can wake gets no number, since stopping it would be a refusal with no way
    // back.
    crate::services::idle::start(
        crate::services::idle::Sweeper::new(
            Arc::clone(services),
            store.clone(),
            std::time::Duration::from_secs(config.services.idle_check_seconds),
        ),
        std::time::Duration::from_secs(config.services.idle_check_seconds),
        shutdown.clone(),
    );

    // **And a third clock measures what all of that is costing.** Beside the two above because it
    // is the same shape, and after them for one reason of its own: what it measures is read out of
    // the `services` rows, so a sweep that stopped something is reflected in the next reading
    // rather than argued with.
    //
    // **Two rates in one loop.** A reading a minute while nobody is watching, which is what the
    // 24-hour history is made of, and a reading a second while a client holds `GET /metrics` open.
    // The handle below is what the API subscribes through, and subscribing is what raises the rate:
    // there is no method that turns sampling on, because a client that crashed could never turn it
    // off again.
    let watchers = crate::metrics::watchers::Watchers::new();
    let sampler = crate::metrics::sampler::Sampler::new(
        store.clone(),
        Arc::clone(host),
        watchers,
        &config.metrics,
    );
    let metrics = sampler.handle();

    // **And a fourth loop watches a ceiling this machine may not be able to hold.** Beside the
    // three above because it belongs to the same family, and unlike all three of them it has *no
    // clock*: it wakes when the sampler finishes a minute, so its rate is that loop's and there is
    // still exactly one thing on this machine reading the process table.
    //
    // Subscribed before the sampler is spawned, so the first minute it finishes has a reader.
    crate::services::watchdog::start(
        crate::services::watchdog::Watchdog::new(
            Arc::clone(services),
            config.services.memory_over_minutes,
        ),
        sampler.minutes(),
        shutdown.clone(),
    );

    crate::metrics::sampler::start(sampler, shutdown.clone());

    metrics
}
