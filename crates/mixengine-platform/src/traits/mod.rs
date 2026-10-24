//! One file per capability. `Host` bundles them so callers take a single injected dependency.

mod access;
mod app_control;
mod autostart;
mod browsers;
mod connections;
mod desktop;
mod elevation;
mod firewall_rules;
mod home;
mod hosts;
mod installers;
mod keyring;
mod limits;
mod machine;
mod metrics;
mod network;
mod orphans;
mod path;
mod port_access;
mod ports;
mod redistributable;
mod reserved;
mod resolver;
mod trust;

pub use access::DirectoryAccess;
pub use app_control::{
    APP_CONTROL_REFUSAL, APPLICATION_CONTROL_BLOCKED, AppControl, AppControlState,
    refused_by_app_control,
};
pub use autostart::{
    AutostartMechanism, AutostartPlan, AutostartState, BootStart, ServiceInstaller,
};
pub use browsers::{BrowserChange, BrowserSurvey, BrowserTrust, DatabaseState};
pub use connections::ConnectionCount;
pub use desktop::{DesktopApps, InstalledApp, Located, Started};
pub use elevation::{Elevation, ElevationSupport, Raised};
pub use firewall_rules::FirewallRules;
pub use home::HomeDirs;
pub use hosts::HostsFile;
pub use installers::{Installers, pkgid_of};
pub use keyring::{KEYRING_SERVICE, Keyring};
pub use limits::{
    Enforcement, LimitMechanism, LimitSupport, MemoryMeasure, ResourceControl, WhenExceeded,
};
pub use machine::{
    Machine, MachineFacts, Probe, VisualCppVersion, avx, dotted_version, shared_libraries,
    visual_cpp_from_registry,
};
pub use metrics::{GroupReading, GroupRoot, ProcessMetrics};
pub use network::{Interface, NetworkInfo, choose as choose_interface};
pub use orphans::{OrphanGuarantee, orphan_guarantee};
pub use path::{PathIntegration, PathLocation, PathState};
pub use port_access::{PortAccess, PortAccessMethod, PortAccessState, PortBinding};
pub use ports::{PortHolder, PortOwner};
pub use redistributable::{
    RedistributableOutcome, Redistributables, VISUAL_CPP_PUBLISHER, names_the_redistributable,
};
pub use reserved::{PortRange, ReservedPorts};
pub use resolver::{ResolverConfig, ResolverMethod, ResolverState};
pub use trust::{TrustState, TrustStore, TrustStoreMethod};

/// Every OS capability MixEngine needs, in one injectable object.
///
/// The daemon is constructed with an `Arc<dyn Host>` rather than calling free functions, which is
/// what makes the whole system testable: `mock::Host` answers the same questions from memory and
/// records the mutations it was asked for.
///
/// Capabilities arrive one accessor at a time as the roadmap reaches them.
///
/// **The firewall is here to be read and never to be written.** Opening a port needs a token the
/// daemon does not have, so writing stays a
/// [`FirewallApply`](mixengine_proto::privileged::PrivilegedOp::FirewallApply) enqueued the way the
/// resolver's changes are and executed by `mixengine-elevate` out of [`crate::firewall`]; no
/// accessor below leads to it.
///
/// Reading is here because the interesting rule is not one of ours. Binding UDP 5353 for mDNS makes
/// Windows offer to write an every-port rule for `mixengined.exe`, and a daemon cannot learn about
/// a rule it never made from its own database. Reading needs no privilege and changes nothing — see
/// [`FirewallRules`].
pub trait Host: std::fmt::Debug + Send + Sync {
    /// Where this OS wants application data to live.
    fn home_dirs(&self) -> &dyn HomeDirs;

    /// Keeping other local users out of the directories MixEngine owns.
    fn directory_access(&self) -> &dyn DirectoryAccess;

    /// Where a password lives, since nothing MixEngine writes may hold one.
    fn keyring(&self) -> &dyn Keyring;

    /// Where this user's PATH is kept, so `<root>/bin` can go on it and come off again.
    fn path_integration(&self) -> &dyn PathIntegration;

    /// Whether this machine starts a daemon for this home at login.
    ///
    /// **Writes as well as reads, and needs no token for either**, which puts it beside
    /// [`path_integration`](Self::path_integration) rather than among the read-only capabilities:
    /// both write something outside `MIXENGINE_HOME` that belongs to this user rather than to the
    /// machine, and both are only ever written when somebody asks. See [`ServiceInstaller`], whose
    /// name is the operating system's word for "service" and not MixEngine's.
    fn service_installer(&self) -> &dyn ServiceInstaller;

    /// Who is already listening on a port a service is about to want.
    fn port_owner(&self) -> &dyn PortOwner;

    /// How busy a port is, which is how a service is found to have nothing to do.
    fn connections(&self) -> &dyn ConnectionCount;

    /// Whether this machine will let an unprivileged front end answer on 80 and 443.
    ///
    /// Reading only: the grant needs a token this process does not have — see [`PortAccess`].
    fn port_access(&self) -> &dyn PortAccess;

    /// Raising the OS elevation prompt on the one-shot helper.
    fn elevation(&self) -> &dyn Elevation;

    /// What the machine's hosts file currently says MixEngine put in it.
    ///
    /// Reading only: the write needs a token this process does not have — see [`HostsFile`].
    fn hosts_file(&self) -> &dyn HostsFile;

    /// Whether a managed TLD arrives at this daemon's own DNS server.
    ///
    /// Reading only: the wiring needs a token this process does not have — see
    /// [`ResolverConfig`].
    fn resolver(&self) -> &dyn ResolverConfig;

    /// Whether this machine trusts MixEngine's own certificate authority.
    ///
    /// **Reads only**, as [`resolver`](Self::resolver) does and for its reason: the write needs a
    /// token the daemon does not have, and belongs to `mixengine-elevate`.
    fn trust_store(&self) -> &dyn TrustStore;

    /// Whether Firefox and Chrome trust that same authority.
    ///
    /// **Beside [`trust_store`](Self::trust_store) rather than inside it**: browsers on Linux read
    /// NSS databases and not the system store at all, there are N of them, and one `bool` cannot
    /// answer for both. Unlike its neighbour this one **writes as well as reads**, because these
    /// databases belong to the user and no token is needed for them.
    fn browsers(&self) -> &dyn BrowserTrust;

    /// What this machine will enforce of a service's declared limits.
    ///
    /// **Reads only**, as [`port_access`](Self::port_access) and [`resolver`](Self::resolver) do:
    /// applying a limit is done to a child through [`process`](crate::process), not asked of the
    /// machine. See [`ResourceControl`].
    fn resource_control(&self) -> &dyn ResourceControl;

    /// What each supervised process group is spending right now.
    ///
    /// **Reads only**, as [`resource_control`](Self::resource_control) does and beside it for the
    /// contrast: that one answers what this machine will enforce, once; this one is asked on a timer
    /// for as long as the daemon runs. See [`ProcessMetrics`].
    fn process_metrics(&self) -> &dyn ProcessMetrics;

    /// Which of this machine's networks a site could be shared on.
    ///
    /// **Reads only**, like [`reserved_ports`](Self::reserved_ports) beside it: sharing binds a
    /// listener this daemon already owns, so nothing here changes the machine.
    fn network(&self) -> &dyn NetworkInfo;

    /// What this system has taken out of circulation.
    ///
    /// The third of three port capabilities, and the one about the operating system rather than
    /// about another program or about privilege — see [`ReservedPorts`].
    fn reserved_ports(&self) -> &dyn ReservedPorts;

    /// What this machine will refuse to load, for want of a signature.
    ///
    /// **Reads only**, and about the operating system rather than about anything MixEngine
    /// installed — the same shape as [`reserved_ports`](Self::reserved_ports) above. See
    /// [`AppControl`].
    fn app_control(&self) -> &dyn AppControl;

    /// What this machine offers the programs MixEngine installs.
    ///
    /// **Reads only**, and read again every time it is asked; see [`Machine`].
    fn machine(&self) -> &dyn Machine;

    /// Installing a Visual C++ runtime this machine lacks.
    ///
    /// **Starts a program that asks Windows for administrator rights**, after a person agreed, and
    /// only a file Microsoft signed; see [`Redistributables`] and ADR 0037.
    fn redistributables(&self) -> &dyn Redistributables;

    /// Who installed this copy, and the system installer a `.pkg` update is handed to. See
    /// [`Installers`] and ADR 0050.
    fn installers(&self) -> &dyn Installers;

    /// Inbound firewall rules naming a program.
    ///
    /// **Reads only**, like [`network`](Self::network) and [`reserved_ports`](Self::reserved_ports)
    /// beside it, and the one direction of the firewall a daemon is allowed. See [`FirewallRules`].
    fn firewall_rules(&self) -> &dyn FirewallRules;

    /// A desktop application somebody installed, found and started.
    ///
    /// **Starts a process, and is the only capability that does**: see [`DesktopApps`] for why it
    /// is here rather than in [`process`](crate::process).
    fn desktop_apps(&self) -> &dyn DesktopApps;
}
