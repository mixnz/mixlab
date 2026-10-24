//! Whether this machine starts a daemon for this home at login — the only door onto it.
//!
//! **Written only when somebody asks**, which is [`crate::shims`]' rule and the same one: a logon
//! task, a LaunchAgent and a systemd user unit are all outside `MIXENGINE_HOME`, and
//! `docs/architecture/daemon-and-ipc.md` has no method that writes there on the daemon's own
//! initiative. `path.*` is the first of them and `autostart.*` is the second. So nothing here is
//! called at start-up, and this type holds no state of its own to refresh.
//!
//! **`enable` registers and does not start; `disable` removes and does not stop.** There is a daemon
//! running by the time either can be called — it is the one answering the call — and a person who
//! turned off "start at login" must not lose it.
//!
//! **`for_this_home` is composed here and not by a client.** There is one entry per user, so a
//! second home enabling replaces it; a client folding "is this entry mine" for itself would be
//! business logic in a client and a place for two clients to disagree.
//!
//! **The boot half belongs to the account, so MixEngine undoes only its own.** Linger starts every
//! unit an account has enabled, not only this one. The home records in its settings that it turned
//! linger on, and `disable` and `disable_at_boot` turn it off only on that record; any call that
//! reads linger off clears it, so a linger the account sets later is never mistaken for MixEngine's.

use std::path::PathBuf;
use std::sync::Arc;

use mixengine_core::updates::records;
use mixengine_platform::{AutostartPlan, AutostartState, BootStart, Host, ServiceInstaller};
use mixengine_proto::{AtBoot, AutostartMechanism, AutostartReport, Error, ErrorCode};

use crate::error::ToWire as _;

/// The settings row saying this home turned the account's linger on.
pub(crate) const LINGER_SETTING: &str = "autostart.linger_by_mixengine";

/// What a person can run when polkit will not let the account linger itself.
const LINGER_BY_HAND: &str = "the login entry is registered; to also start at boot, run: sudo \
                              loginctl enable-linger $USER";

/// The daemon this machine would start at login, and the machine that would start it.
#[derive(Debug)]
pub(crate) struct Autostart {
    /// The binary an entry names: this process's own image.
    ///
    /// The same value [`crate::shims::Shims`] is given, and taken once in `main` rather than read
    /// again here — a daemon that answered with a different path from the one it registered would
    /// be reporting about a program it is not.
    program: PathBuf,

    /// The home an entry names, as `--home`.
    root: PathBuf,

    /// The OS.
    host: Arc<dyn Host>,

    /// Where the home records that it turned the account's linger on.
    store: mixengine_core::Store,
}

impl Autostart {
    pub(crate) fn new(
        program: PathBuf,
        root: PathBuf,
        host: Arc<dyn Host>,
        store: mixengine_core::Store,
    ) -> Self {
        Self {
            program,
            root,
            host,
            store,
        }
    }

    /// `autostart.status` — what this machine would do at the next login, and at the next boot.
    pub(crate) async fn status(&self) -> Result<AutostartReport, Error> {
        let state = self.installer(|installer| installer.state()).await?;
        let boot = self.installer(|installer| installer.boot_state()).await?;

        self.report(state, boot, false).await
    }

    /// `autostart.enable` — register the entry, replacing whatever was there. Linger is untouched.
    pub(crate) async fn enable(&self) -> Result<AutostartReport, Error> {
        let plan = self.plan();
        let state = self
            .installer(move |installer| installer.enable(&plan))
            .await?;
        let boot = self.installer(|installer| installer.boot_state()).await?;

        self.report(state, boot, false).await
    }

    /// `autostart.disable` — take the entry away, and the linger this home turned on with it, and
    /// leave the running daemon alone.
    pub(crate) async fn disable(&self) -> Result<AutostartReport, Error> {
        let state = self.installer(|installer| installer.disable()).await?;
        let (boot, lingered) = self.release_linger().await?;

        self.report(state, boot, lingered).await
    }

    /// `autostart.enable_at_boot` — the entry first, then the account's linger.
    ///
    /// **In that order** so a refused linger leaves the login entry, which is useful on its own,
    /// rather than an account lingering for a unit that was never written.
    pub(crate) async fn enable_at_boot(&self) -> Result<AutostartReport, Error> {
        let plan = self.plan();
        let state = self
            .installer(move |installer| installer.enable(&plan))
            .await?;
        let before = self.installer(|installer| installer.boot_state()).await?;

        let after = self
            .installer(|installer| installer.enable_boot())
            .await
            .map_err(|error| match error.code {
                ErrorCode::ProcessFailed => error.with_hint(LINGER_BY_HAND),
                _ => error,
            })?;

        if before == BootStart::Off && after == BootStart::On {
            records::set(&self.store, LINGER_SETTING, &true)
                .await
                .map_err(|error| error.to_wire())?;
        }

        self.report(state, after, before != after).await
    }

    /// `autostart.disable_at_boot` — keep the login entry, release this home's linger.
    pub(crate) async fn disable_at_boot(&self) -> Result<AutostartReport, Error> {
        let state = self.installer(|installer| installer.state()).await?;
        let (boot, lingered) = self.release_linger().await?;

        self.report(state, boot, lingered).await
    }

    /// Whether the account lingers because this home turned it on — what uninstall has to undo even
    /// when the unit is already gone, since `disable` is what takes it away.
    pub(crate) async fn holds_linger(&self) -> Result<bool, Error> {
        let boot = self.installer(|installer| installer.boot_state()).await?;

        Ok(boot == BootStart::On && self.linger_is_ours().await?)
    }

    /// Turn linger off if, and only if, this home turned it on. Answers the reading afterwards and
    /// whether anything changed.
    async fn release_linger(&self) -> Result<(BootStart, bool), Error> {
        let before = self.installer(|installer| installer.boot_state()).await?;

        if before != BootStart::On || !self.linger_is_ours().await? {
            return Ok((before, false));
        }

        let after = self.installer(|installer| installer.disable_boot()).await?;
        records::clear(&self.store, LINGER_SETTING)
            .await
            .map_err(|error| error.to_wire())?;

        Ok((after, after != before))
    }

    /// Whether this home's record says it turned the account's linger on.
    async fn linger_is_ours(&self) -> Result<bool, Error> {
        Ok(records::get::<bool>(&self.store, LINGER_SETTING)
            .await
            .map_err(|error| error.to_wire())?
            .unwrap_or(false))
    }

    /// One platform call, off the runtime's threads — a tool to run and a file to write.
    async fn installer<T, F>(&self, work: F) -> Result<T, Error>
    where
        F: FnOnce(&dyn ServiceInstaller) -> mixengine_platform::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let host = Arc::clone(&self.host);

        crate::api::on_a_blocking_thread(move || {
            work(host.service_installer()).map_err(|error| error.to_wire())
        })
        .await
    }

    /// What an entry is asked to start: this daemon, for this home.
    fn plan(&self) -> AutostartPlan {
        AutostartPlan {
            program: self.program.clone(),
            home: self.root.clone(),
        }
    }

    /// The wire answer, with the two things a client may not work out for itself.
    ///
    /// **Compared on the `--home` the entry carries and never on the program.** An update moves
    /// `mixengined` and leaves the home exactly where it was, so a comparison on the binary's path
    /// would call an entry somebody else's the day after an update — which is the opposite of what
    /// this field is for.
    ///
    /// **A record whose linger reads off is cleared here**, whichever call is answering — the one
    /// write a status makes, to MixEngine's own settings and never to the machine. A machine that
    /// cannot say (`Unsupported`) keeps the record: not knowing is not having been turned off.
    async fn report(
        &self,
        state: AutostartState,
        boot: BootStart,
        lingered: bool,
    ) -> Result<AutostartReport, Error> {
        let ours = self.linger_is_ours().await?;

        if ours && boot == BootStart::Off {
            records::clear(&self.store, LINGER_SETTING)
                .await
                .map_err(|error| error.to_wire())?;
        }

        let named = state
            .command
            .iter()
            .skip_while(|word| *word != "--home")
            .nth(1)
            .map(PathBuf::from);

        let for_this_home = named.is_some_and(|home| home == self.root);

        // A linger with no entry for this home starts the account's manager and nothing of this
        // daemon's, so it reads off: the question is whether *this* daemon comes back at boot.
        let starts = state.enabled && for_this_home;

        Ok(AutostartReport {
            mechanism: mechanism(state.mechanism),
            location: state.location,
            enabled: state.enabled,
            changed: state.changed || lingered,
            command: state.command,
            for_this_home,
            at_boot: match (boot, ours) {
                (BootStart::Unsupported, _) => AtBoot::Unsupported,
                (BootStart::Off, _) => AtBoot::Off,
                (BootStart::On, _) if !starts => AtBoot::Off,
                (BootStart::On, true) => AtBoot::On,
                (BootStart::On, false) => AtBoot::OnByAccount,
            },
        })
    }
}

/// The platform's answer, in the wire's vocabulary.
///
/// Two enums rather than one, because `mixengine-platform` may not depend on the wire types and the
/// wire may not depend on the platform — the arrangement every other capability here has.
fn mechanism(mechanism: mixengine_platform::AutostartMechanism) -> AutostartMechanism {
    match mechanism {
        mixengine_platform::AutostartMechanism::LogonTask => AutostartMechanism::LogonTask,
        mixengine_platform::AutostartMechanism::LaunchAgent => AutostartMechanism::LaunchAgent,
        mixengine_platform::AutostartMechanism::SystemdUser => AutostartMechanism::SystemdUser,
        mixengine_platform::AutostartMechanism::None => AutostartMechanism::None,
    }
}

#[cfg(test)]
mod tests {
    use mixengine_platform::{BootStart, mock};

    use super::*;

    const ROOT: &str = "/tmp/mine";

    /// A real database in a directory the test owns, which is where the linger record lives.
    async fn store() -> (tempfile::TempDir, mixengine_core::Store) {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let store = mixengine_core::Store::open(&directory.path().join("mixengine.db"))
            .await
            .expect("a database");
        (directory, store)
    }

    /// This home's `Autostart` on `host`, with a database of its own.
    async fn autostart(host: mock::Host) -> (tempfile::TempDir, Autostart, Arc<mock::Host>) {
        let (directory, store) = store().await;
        let host = Arc::new(host);
        let autostart = Autostart::new(
            PathBuf::from("/usr/bin/mixengined"),
            PathBuf::from(ROOT),
            Arc::clone(&host) as Arc<dyn Host>,
            store,
        );
        (directory, autostart, host)
    }

    #[tokio::test]
    async fn an_entry_this_daemon_registered_is_this_home_s() {
        let (_directory, autostart, _host) = autostart(mock::Host::with_home(ROOT)).await;

        let enabled = autostart.enable().await.expect("register");

        assert!(enabled.enabled);
        assert!(enabled.changed);
        assert!(enabled.for_this_home);
        assert_eq!(enabled.at_boot, AtBoot::Off, "enable leaves linger alone");
    }

    /// The half-state the field exists to name: an entry that is there and is somebody else's.
    #[tokio::test]
    async fn an_entry_naming_another_home_is_registered_and_not_this_home_s() {
        let (_directory, mine, host) = autostart(mock::Host::with_home(ROOT)).await;
        let (_theirs_directory, theirs_store) = store().await;

        // Registered from the other home, through the same machine.
        let elsewhere = Autostart::new(
            PathBuf::from("/usr/bin/mixengined"),
            PathBuf::from("/tmp/theirs"),
            host as Arc<dyn Host>,
            theirs_store,
        );
        elsewhere.enable().await.expect("register");

        let status = mine.status().await.expect("read");

        assert!(status.enabled);
        assert!(!status.for_this_home);
        assert!(!status.changed, "a status never claims a write");
    }

    #[tokio::test]
    async fn nothing_registered_is_not_this_home_s_either() {
        let (_directory, autostart, _host) = autostart(mock::Host::with_home(ROOT)).await;

        let status = autostart.status().await.expect("read");

        assert!(!status.enabled);
        assert!(!status.for_this_home);
        assert!(status.command.is_empty());
    }

    #[tokio::test]
    async fn a_machine_with_no_mechanism_answers_a_status_and_refuses_both_enables() {
        let (_directory, autostart, _host) =
            autostart(mock::Host::without_an_autostart_mechanism(ROOT)).await;

        let status = autostart.status().await.expect("a status reports");
        assert_eq!(status.mechanism, AutostartMechanism::None);
        assert_eq!(status.at_boot, AtBoot::Unsupported);

        for refused in [
            autostart.enable().await.expect_err("an enable refuses"),
            autostart
                .enable_at_boot()
                .await
                .expect_err("so does one at boot"),
        ] {
            assert_eq!(refused.code, ErrorCode::UnsupportedPlatform);
        }
    }

    #[tokio::test]
    async fn boot_start_is_ours_to_undo_when_we_turned_it_on() {
        let (_directory, autostart, host) = autostart(mock::Host::with_home(ROOT)).await;

        let on = autostart.enable_at_boot().await.expect("enable at boot");
        assert!(on.enabled && on.changed && on.for_this_home, "{on:?}");
        assert_eq!(on.at_boot, AtBoot::On);

        let off = autostart.disable_at_boot().await.expect("disable at boot");
        assert_eq!(off.at_boot, AtBoot::Off);
        assert!(off.changed, "{off:?}");
        assert!(off.enabled, "the login entry stays: {off:?}");
        assert!(
            host.autostart_operations()
                .contains(&mock::AutostartOp::BootDisabled)
        );
    }

    #[tokio::test]
    async fn an_account_that_already_lingered_keeps_lingering() {
        let (_directory, autostart, host) =
            autostart(mock::Host::with_linger(ROOT, BootStart::On)).await;

        let enabled = autostart.enable_at_boot().await.expect("enable at boot");
        assert_eq!(enabled.at_boot, AtBoot::OnByAccount);

        let kept = autostart.disable_at_boot().await.expect("disable at boot");
        assert_eq!(kept.at_boot, AtBoot::OnByAccount);
        assert!(!kept.changed, "{kept:?}");

        let gone = autostart.disable().await.expect("disable");
        assert!(!gone.enabled, "{gone:?}");
        assert_eq!(
            gone.at_boot,
            AtBoot::Off,
            "with no entry nothing starts at boot, whatever the linger: {gone:?}"
        );
        assert_eq!(
            host.service_installer().boot_state().expect("read"),
            BootStart::On,
            "the account's linger is still on"
        );
        assert!(
            !host
                .autostart_operations()
                .contains(&mock::AutostartOp::BootDisabled),
            "MixEngine turned off a linger it never turned on"
        );
    }

    #[tokio::test]
    async fn disable_takes_our_linger_with_the_entry() {
        let (_directory, autostart, _host) = autostart(mock::Host::with_home(ROOT)).await;
        autostart.enable_at_boot().await.expect("enable at boot");

        let gone = autostart.disable().await.expect("disable");

        assert!(!gone.enabled && gone.changed, "{gone:?}");
        assert_eq!(gone.at_boot, AtBoot::Off);
    }

    #[tokio::test]
    async fn a_refused_linger_leaves_the_login_entry_and_names_sudo() {
        let (_directory, autostart, _host) = autostart(mock::Host::refusing_linger(ROOT)).await;

        let refused = autostart
            .enable_at_boot()
            .await
            .expect_err("polkit said no");
        let hint = refused.hint.clone().unwrap_or_default();
        assert!(hint.contains("sudo loginctl enable-linger"), "{refused:?}");

        let after = autostart.status().await.expect("read");
        assert!(after.enabled && after.for_this_home, "{after:?}");
        assert_eq!(after.at_boot, AtBoot::Off);
    }

    /// The entry names another home: what starts at boot is that home's daemon, not this one.
    #[tokio::test]
    async fn an_entry_for_another_home_does_not_start_this_one_at_boot() {
        let (_directory, mine, host) =
            autostart(mock::Host::with_linger(ROOT, BootStart::On)).await;
        let (_theirs_directory, theirs_store) = store().await;
        let elsewhere = Autostart::new(
            PathBuf::from("/usr/bin/mixengined"),
            PathBuf::from("/tmp/theirs"),
            host as Arc<dyn Host>,
            theirs_store,
        );
        elsewhere.enable().await.expect("register");

        assert_eq!(mine.status().await.expect("read").at_boot, AtBoot::Off);
    }

    /// The unit was deleted by hand and the linger MixEngine turned on was left: uninstall has to be
    /// able to see it, and `disable` still takes it away.
    #[tokio::test]
    async fn a_linger_we_set_outlives_a_unit_removed_by_hand_until_disable() {
        let (_directory, autostart, host) = autostart(mock::Host::with_home(ROOT)).await;
        autostart.enable_at_boot().await.expect("enable at boot");
        host.service_installer()
            .disable()
            .expect("the unit, removed by hand");

        assert!(autostart.holds_linger().await.expect("ask"));

        autostart.disable().await.expect("disable");
        assert_eq!(
            host.service_installer().boot_state().expect("read"),
            BootStart::Off
        );
        assert!(!autostart.holds_linger().await.expect("ask again"));
    }

    #[tokio::test]
    async fn an_account_s_own_linger_is_not_ours_to_hold() {
        let (_directory, autostart, _host) =
            autostart(mock::Host::with_linger(ROOT, BootStart::On)).await;

        assert!(!autostart.holds_linger().await.expect("ask"));
    }

    /// Somebody ran `loginctl disable-linger` by hand: the record is cleared at the next read, so a
    /// linger the account sets later is never taken for MixEngine's.
    #[tokio::test]
    async fn a_linger_turned_off_by_hand_is_no_longer_ours() {
        let (_directory, autostart, host) = autostart(mock::Host::with_home(ROOT)).await;
        autostart.enable_at_boot().await.expect("enable at boot");

        host.service_installer()
            .disable_boot()
            .expect("the person, by hand");
        assert_eq!(autostart.status().await.expect("read").at_boot, AtBoot::Off);

        host.service_installer()
            .enable_boot()
            .expect("the person again");
        assert_eq!(
            autostart.status().await.expect("read").at_boot,
            AtBoot::OnByAccount
        );
    }
}
