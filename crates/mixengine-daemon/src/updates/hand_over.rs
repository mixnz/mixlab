//! Handing an update over to the installer that outlives this daemon, and finishing what it began.

use std::path::PathBuf;

use mixengine_core::install::NotAnArchive;
use mixengine_core::updates::{self, Feed};
use mixengine_proto::{Error, ErrorCode, Timestamp, UpdateHandedOver};

use crate::error::ToWire as _;
use mixengine_core::UpdateError;

use super::{Quiet, Updates, host, install_command, only_file_in, read};

impl Updates {
    /// `update.hand_over` — download and verify the next `.pkg`, open it in Installer.app, and
    /// write the handover down. **Nothing is stopped at any point**: a person who cancels the
    /// installer has lost nothing.
    ///
    /// # Errors
    ///
    /// `precondition_failed` when no feed has been read, the version is not the one offered, this
    /// copy is not the `.pkg`'s, or the release has no installer for this machine; whatever the
    /// download and the checksum reported; and the platform's error, with the command that installs
    /// the verified file without a window as its hint, when the installer would not open.
    pub(crate) async fn hand_over(&self, version: &str) -> Result<UpdateHandedOver, Error> {
        let checked = self.checked()?;

        if checked.feed.version != version {
            return Err(
                mixengine_core::Error::Update(UpdateError::UpdateNotOffered {
                    asked: version.to_owned(),
                    offered: Some(checked.feed.version.clone()),
                })
                .to_wire(),
            );
        }

        let updates::Placement::Installer { .. } = &self.placement else {
            return Err(Error::new(
                ErrorCode::PreconditionFailed,
                "this copy of MixEngine was not placed by an installer",
            )
            .with_hint("`mix self-update` updates it in place"));
        };

        let (os, arch) = host()?;
        let installer = checked
            .feed
            .installer(os, arch, self.installer_kind(), self.headless())
            .ok_or_else(|| {
                mixengine_core::Error::Update(UpdateError::InstallerUnavailable {
                    os: format!("{os:?}").to_lowercase(),
                    arch: format!("{arch:?}").to_lowercase(),
                })
                .to_wire()
            })?;

        let package = self.fetch_package(version, installer).await?;

        // The platform's error keeps its own code; the hint is the line that installs the verified
        // file without a window, which is also what over SSH somebody would need (measured on a
        // Mac).
        //
        // **No installer to open is not a failure.** A Linux machine with no desktop session has
        // the verified package and the command that installs it, which is the whole handover there.
        let opened = match self.host.installers().open(&package) {
            Ok(()) => true,
            Err(mixengine_platform::Error::UnsupportedPlatform { reason, .. }) => {
                tracing::info!(%reason, "no software installer to open the package in");
                false
            }
            Err(error) => return Err(error.to_wire().with_hint(install_command(&package))),
        };

        updates::records::set(
            &self.store,
            updates::records::HANDED_OVER,
            &updates::records::HandedOver {
                version: version.to_owned(),
                at: Timestamp::from_system_time(std::time::SystemTime::now()),
            },
        )
        .await
        .map_err(|error| error.to_wire())?;

        tracing::info!(%version, package = %package.display(), opened, "a package was handed over");

        Ok(UpdateHandedOver {
            version: version.to_owned(),
            package: package.display().to_string(),
            command: install_command(&package),
            opened,
        })
    }

    /// Download and verify the package, or reuse the one already verified for this version.
    ///
    /// **Reused**, because the ordinary way back into this is a person who cancelled Installer.app
    /// and pressed Update again, and 70 MB fetched twice is a cost with nothing bought by it. The
    /// file is hashed again first: it sits in the user's home, where any local process can change
    /// it.
    async fn fetch_package(
        &self,
        version: &str,
        installer: &updates::feed::InstallerArtifact,
    ) -> Result<PathBuf, Error> {
        let into = self.staging_for(version);

        if let Some(existing) = only_file_in(&into) {
            let hashed = existing.clone();
            // A digest or nothing: a file that cannot be hashed is fetched again, so the error is
            // dropped where it is made rather than carried out of the closure.
            let digest = tokio::task::spawn_blocking(move || {
                mixengine_core::install::sha256_of(&hashed).ok()
            })
            .await
            .ok()
            .flatten();

            if digest.as_deref() == Some(installer.sha256.as_str()) {
                return Ok(existing);
            }
        }

        // Anything else there is a half-finished or wrong download, and `install` refuses a
        // directory that exists: an update must not trip over its own leftovers.
        let _ = tokio::fs::remove_dir_all(&into).await;

        self.installer
            .install(
                &installer.as_artifact(),
                &into,
                None,
                NotAnArchive::OneFile,
                &Quiet,
            )
            .await
            .map_err(|error| error.to_wire())?;

        only_file_in(&into).ok_or_else(|| {
            Error::new(
                ErrorCode::Internal,
                format!("{} holds no package after the download", into.display()),
            )
        })
    }

    /// The version on disk, while a handover is recorded and the binary there says it is that
    /// version. [`None`] otherwise, including for a different version somebody installed.
    pub(super) async fn installed_version(&self) -> Option<String> {
        let handed: updates::records::HandedOver =
            read(&self.store, updates::records::HANDED_OVER).await?;
        let exe = self.daemon_exe.clone()?;
        let identity = mixengine_platform::install::file_identity(&exe)?;

        let cached = self.installed.lock().ok()?.clone();
        let on_disk = match cached {
            Some((seen, version)) if seen == identity => version,
            _ => {
                let version =
                    tokio::task::spawn_blocking(move || updates::installed::version_of(&exe))
                        .await
                        .ok()
                        .flatten();

                if let Ok(mut installed) = self.installed.lock() {
                    *installed = Some((identity, version.clone()));
                }

                version
            }
        };

        on_disk.filter(|version| *version == handed.version)
    }

    /// Forget a handover that no longer applies.
    ///
    /// Two ways: this daemon already is the version handed over (somebody installed and restarted
    /// another way), or the feed now offers a different release than the one handed over.
    pub(super) async fn settle_handover(&self, feed: Option<&Feed>) {
        let Some(handed) =
            read::<updates::records::HandedOver>(&self.store, updates::records::HANDED_OVER).await
        else {
            return;
        };

        let running = handed.version == env!("CARGO_PKG_VERSION");
        let superseded = feed.is_some_and(|feed| feed.version != handed.version);

        if running || superseded {
            self.forget(&handed.version).await;
        }
    }

    /// The version `update.finish` may finish, or why not.
    ///
    /// # Errors
    ///
    /// `precondition_failed` when nothing was handed over, or when the binary on disk is not yet
    /// the version that was.
    pub(crate) async fn finishable(&self) -> Result<String, Error> {
        let Some(handed) =
            read::<updates::records::HandedOver>(&self.store, updates::records::HANDED_OVER).await
        else {
            return Err(Error::new(
                ErrorCode::PreconditionFailed,
                "nothing has been handed to the installer",
            )
            .with_hint("`mix self-update` opens the next .pkg in Installer.app"));
        };

        match self.installed_version().await {
            Some(version) => Ok(version),
            None => Err(
                mixengine_core::Error::Update(UpdateError::UpdateNotInstalled {
                    version: handed.version,
                })
                .to_wire(),
            ),
        }
    }
}
