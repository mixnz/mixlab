//! Handing a running service its new configuration without a restart: by the command its
//! spec names, or by the signal it names, and what a reload that could not be delivered
//! leaves on the row.

use mixengine_platform::process::{self, CAN_SIGNAL, Supervised};
use mixengine_proto::{Millis, ReloadBehaviour, ReloadSignal};
use mixengine_supervisor::Surroundings;

use super::Runner;

impl Runner {
    /// Hand the running process the configuration that was rewritten under it.
    ///
    /// `caddy reload`, and later `nginx -s reload`: a program shipped with the service, run in the
    /// service's own surroundings, that tells the process listening to re-read its file. Everything
    /// that decides *whether* to do this is the registry's — see
    /// [`Registry::hand_over`](super::super::Registry::hand_over) — and everything about *how* is the
    /// spec's, which is why this function is as short as it is.
    ///
    /// **Nothing here changes the service's state.** A reload that worked is not news: the process
    /// was running before it and is running after it, and a state change would announce a restart
    /// that did not happen. A reload that failed is not news about the *service* either — it is
    /// still up, still serving, still on the configuration it had — so it is a line in `daemon.log`
    /// rather than a degradation. What a user has then is a file on disk that the running process
    /// is not using, which the next start resolves and which nothing reports yet: `mix doctor` does
    /// not look for it.
    pub(super) async fn reload(&self, place: &Surroundings, supervised: &Supervised) {
        match self.spec.reload() {
            Some(ReloadBehaviour::Command {
                program,
                args,
                patience,
            }) => {
                self.reload_by_command(place, program, args, *patience)
                    .await
            }

            Some(ReloadBehaviour::Signal { signal, patience }) => {
                self.reload_by_signal(supervised, *signal, *patience).await;
            }

            // Asked of a service that has no way to be asked. Said at `warn` because the
            // alternative is a person editing an override, watching the daemon accept it, and
            // finding the old value still in force with nothing anywhere saying why.
            _ => tracing::warn!(
                service = self.spec.id().as_str(),
                "this service's configuration changed and it has no reload, so the running process \
                 is still using the previous one; it will be read at the next start"
            ),
        }
    }

    /// The [`ReloadBehaviour::Command`] half, which came first — unchanged, moved.
    async fn reload_by_command(
        &self,
        place: &Surroundings,
        program: &std::path::Path,
        args: &[String],
        patience: Millis,
    ) {
        match place.run(program, args, patience.as_duration()).await {
            Ok(ran) if ran.succeeded() => tracing::info!(
                service = self.spec.id().as_str(),
                "this service re-read its configuration without being restarted"
            ),

            // The two ways it did not work, kept apart because they send a reader to different
            // places: the program ran and refused, or it could not be run at all.
            Ok(ran) => tracing::warn!(
                service = self.spec.id().as_str(),
                program = %program.display(),
                detail = ran.complaint().unwrap_or("it failed without saying why"),
                "this service refused the configuration it was asked to re-read; the process is \
                 still running the previous one"
            ),

            Err(error) => tracing::warn!(
                service = self.spec.id().as_str(),
                program = %program.display(),
                %error,
                "the command that hands this service its configuration could not be run; the \
                 process is still running the previous one"
            ),
        }
    }

    /// The [`ReloadBehaviour::Signal`] half.
    ///
    /// **[`CAN_SIGNAL`] is read before anything is waited for**, which is [`CAN_ASK_TO_STOP`]'s
    /// lesson applied one method along: a system with no signals should say so at the moment it is
    /// asked, not after a patience spent on a delivery nobody attempted. A recipe on such a system
    /// returns no reload at all, so this arm is the belt to that braces.
    ///
    /// **The patience is a wait and not a check.** A signal has no exit status: the daemon cannot
    /// learn from the OS whether php-fpm liked the file it was told to re-read, only that the signal
    /// was delivered. What the wait buys is that the next configuration change does not arrive on
    /// top of a pool that is still cycling its workers — so it is spent *after* the delivery has
    /// been reported, not before.
    ///
    /// [`CAN_ASK_TO_STOP`]: mixengine_platform::process::CAN_ASK_TO_STOP
    async fn reload_by_signal(
        &self,
        supervised: &Supervised,
        signal: ReloadSignal,
        patience: Millis,
    ) {
        if !CAN_SIGNAL {
            tracing::warn!(
                service = self.spec.id().as_str(),
                "this service is reloaded by signal and this system has none, so the running \
                 process is still using its previous configuration; it will be read at the next \
                 start"
            );

            return;
        }

        let which = match signal {
            ReloadSignal::Hup => process::Signal::Hup,
            ReloadSignal::Usr1 => process::Signal::Usr1,
            ReloadSignal::Usr2 => process::Signal::Usr2,

            // The wire enum is `#[non_exhaustive]` and this crate is downstream of it, so a variant
            // added there without a mapping here is reported rather than silently dropped.
            other => {
                tracing::warn!(
                    service = self.spec.id().as_str(),
                    signal = ?other,
                    "this build cannot send the signal this service is reloaded with"
                );

                return;
            }
        };

        match supervised.signal(which) {
            Ok(()) => {
                // Said before the wait and not after it. The signal is delivered by the time
                // `signal` returns, so the news is true now; logging it on the far side of the
                // patience would stamp `daemon.log` ten seconds late and keep a reader waiting for
                // an answer the daemon already has.
                tracing::info!(
                    service = self.spec.id().as_str(),
                    signal = ?signal,
                    "this service was signalled to re-read its configuration"
                );

                tokio::time::sleep(patience.as_duration()).await;
            }

            Err(error) => tracing::warn!(
                service = self.spec.id().as_str(),
                signal = ?signal,
                %error,
                "this service could not be signalled to re-read its configuration; the process is \
                 still running the previous one"
            ),
        }
    }
}
