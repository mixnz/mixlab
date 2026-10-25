//! What a supervised service *is* — the vocabulary `mixengine-core` writes and
//! `mixengine-supervisor` runs.
//!
//! This lives in `proto` rather than in either of those crates because they are siblings that cannot
//! depend on each other, and because a [`ServiceSpec`] is not a supervisor implementation detail: a
//! row in `services` stores it, the GUI's Services screen edits it, and an `extension.toml` declares
//! one. The reasoning, and the rule it sets for the rest of the workspace, are in
//! `docs/decisions/0006-servicespec-in-proto-and-secret-free.md`.
//!
//! The consequence worth stating twice: **a spec cannot express a secret by value.** See
//! [`EnvValue`].
//!
//! Three fields describe behaviour this crate does not implement — [`ServiceSpec::limits`],
//! [`ServiceSpec::idle`] and [`ServiceSpec::logs`] are enforced by the daemon and the supervisor.
//! They were declared before anything enforced them, so that enforcement added an implementation
//! rather than a field, and a spec written then did not have to be revisited to gain one.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

mod policy;
mod validate;

pub use policy::{
    Backoff, HealthCheck, HealthProbe, IdleExemption, IdlePolicy, IdleProbe, IdleSource, LogPolicy,
    Priority, ReadyCheck, ReloadBehaviour, ReloadSignal, ResourceLimits, RestartPolicy,
    StopBehaviour,
};

/// A service's human-stable identity: `caddy`, `mariadb@main`, `php-fpm@8.3`.
///
/// Validated on construction *and* on deserialisation, because this string is not only an
/// identifier. It names a directory — `logs/services/<id>/`, `etc/<id>/` — so a value containing a
/// path separator, or one Windows refuses as a filename, is a broken install rather than a bad
/// lookup. The charset is narrow enough that the interesting cases cannot be spelled at all.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceId(String);

impl ServiceId {
    /// The longest an id may be, counted in bytes.
    ///
    /// Not a filesystem limit — every target allows 255 — but a limit on what belongs in a table
    /// column, a log prefix and a GUI tile. Anything approaching it is a naming mistake.
    pub const MAX_LEN: usize = 64;

    /// Names Windows refuses as a file or directory, with or without an extension.
    ///
    /// `con` is a plausible service name for a console tool and the failure it causes — a directory
    /// that cannot be created, on one OS only — is exactly the kind the cross-platform rule in
    /// `CLAUDE.md` exists to catch before merge rather than after.
    ///
    /// Matched against the *whole* id, because the whole id is the directory name. Windows refuses
    /// `con`; it does not refuse `mariadb@aux`, which is an ordinary name that merely contains one.
    const RESERVED: [&'static str; 24] = [
        "con", "prn", "aux", "nul", "com0", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
        "com8", "com9", "lpt0", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8",
        "lpt9",
    ];

    /// Parse an id, rejecting anything that cannot also be a directory name.
    ///
    /// The shape is `name` or `name@instance`. Both halves start with an ASCII letter or digit and
    /// continue with those plus `-`; an instance may also contain `.`, because it carries version
    /// numbers (`php-fpm@8.3`). Lowercase only, so two ids cannot differ by case alone and then
    /// collide on a case-insensitive filesystem, and never a trailing `.`, which Windows strips from
    /// a directory name and so would collide the same way.
    ///
    /// # Errors
    ///
    /// [`SpecError::ServiceId`] naming what is wrong with the value, phrased for whoever typed it.
    pub fn parse(value: impl Into<String>) -> Result<Self, SpecError> {
        let value = value.into();

        let reject = |reason: &str| {
            Err(SpecError::ServiceId {
                value: value.clone(),
                reason: reason.to_owned(),
            })
        };

        if value.is_empty() {
            return reject("it is empty");
        }
        if value.len() > Self::MAX_LEN {
            return reject(&format!("it is longer than {} characters", Self::MAX_LEN));
        }

        let (name, instance) = match value.split_once('@') {
            Some((_, instance)) if instance.contains('@') => {
                return reject("it has more than one `@`");
            }
            Some((name, instance)) => (name, Some(instance)),
            None => (value.as_str(), None),
        };

        for (part, label, extra) in [
            (name, "name", ""),
            (instance.unwrap_or("x"), "instance", "."),
        ] {
            if part.is_empty() {
                return reject(&format!("its {label} is empty"));
            }
            if !part.starts_with(|character: char| {
                character.is_ascii_lowercase() || character.is_ascii_digit()
            }) {
                return reject(&format!(
                    "its {label} must start with a lowercase letter or a digit"
                ));
            }
            if !part.chars().all(|character| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || character == '-'
                    || extra.contains(character)
            }) {
                return reject(&format!(
                    "its {label} may only contain lowercase letters, digits, `-`{}",
                    if extra.is_empty() { "" } else { " and `.`" }
                ));
            }
            if part.ends_with('.') {
                return reject(&format!(
                    "its {label} ends with `.`, which Windows strips from a directory name"
                ));
            }
        }

        // On the whole id rather than on either half, because the whole id is the directory name:
        // Windows refuses `con`, not every name that happens to contain one, and `mariadb@aux` is
        // an ordinary directory. Windows matches the part before the first `.`, which needs no
        // handling here — a `.` only ever appears in an instance, so the part before it always
        // carries the `@` that makes the whole thing not a device name.
        if Self::RESERVED.contains(&value.as_str()) {
            return reject(&format!(
                "`{value}` is a reserved device name on Windows, so it cannot be a directory"
            ));
        }

        Ok(Self(value))
    }

    /// The id as it is written everywhere: on the wire, in the database, as a directory name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The part before `@` — the package this is an instance of.
    #[must_use]
    pub fn name(&self) -> &str {
        self.0
            .split_once('@')
            .map_or(self.0.as_str(), |(name, _)| name)
    }

    /// The part after `@`, when there is one.
    ///
    /// `None` means a service that exists once: there is exactly one Caddy, and `caddy@main` would
    /// be a distinction without a difference.
    #[must_use]
    pub fn instance(&self) -> Option<&str> {
        self.0.split_once('@').map(|(_, instance)| instance)
    }
}

impl fmt::Display for ServiceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for ServiceId {
    /// Validating, unlike most wire types.
    ///
    /// An unknown [`crate::ErrorCode`] is answered leniently because the sentence beside it still
    /// helps a person. There is no equivalent here: an id that cannot be a directory name will fail
    /// later, further from the cause, in the middle of starting something.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = <String as serde::Deserialize<'_>>::deserialize(deserializer)?;

        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

/// The application-side namespace every credential MixEngine owns is stored under.
///
/// One constant rather than a literal in each caller, because the pair `(service, key)` is an
/// address: a recipe that names a credential and a daemon that stores one have to agree letter for
/// letter, and the failure when they do not is a service that starts with an empty password.
///
/// The *key* half is where a service tells itself apart from another — `mariadb@main/root`, composed
/// by `mixengine_core::services::handoff::secret_key`.
///
/// **Published rather than internal, and here rather than in `mixengine-platform`**
/// (`docs/specs/2026-09-04-t84-the-window-in-the-registry-and-one-keyring-design.md`, D6). MixLab
/// reads credentials MixEngine stored, and the only way two applications agree on a namespace
/// without one of them hardcoding the other's is for the namespace to be part of the contract. It
/// is the `service` half of every [`EnvValue::Keyring`] this workspace builds and of every
/// [`SecretAddress`](crate::SecretAddress) it answers with. `mixengine_platform::KEYRING_SERVICE`
/// re-exports it, so the capability that takes the pair still names the constant beside the trait.
pub const KEYRING_SERVICE: &str = "mixengine";

/// One environment variable's value.
///
/// **This is the type that keeps a password out of a spec.** MariaDB's generated root password lives
/// in the OS keyring; a spec names it and the supervisor resolves it through the platform `Keyring`
/// capability at the moment it builds the child's `Command`, so the value exists nowhere that is
/// persisted, serialised or logged.
///
/// The shape is Kubernetes' `env.valueFrom.secretKeyRef` and systemd's `LoadCredential=`, both of
/// which exist because the obvious design leaks: `docker inspect` prints environment variables, and
/// so does `systemctl show`.
///
/// **Written tagged, read tagged or bare.** The canonical form — what the daemon sends and what the
/// database stores — always carries `from`. Deserialisation additionally accepts a plain string as a
/// [`Literal`](EnvValue::Literal), because `TZ = "UTC"` is what an `extension.toml` author writes
/// and the tagged spelling buys nothing for the variant that names nothing. The same asymmetry
/// [`crate::Millis`] has, and for the same reason.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(tag = "from", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum EnvValue {
    /// A value written out in full — a port, a path, a log level.
    ///
    /// **Non-secret by contract.** Nothing enforces it, which is why the other variant exists and is
    /// the one to reach for whenever the answer is "it depends what the password is".
    Literal {
        /// The value, exactly as the child will see it.
        value: String,
    },

    /// A value fetched from the OS keyring at spawn time.
    Keyring {
        /// The keyring service name the credential was stored under.
        service: String,
        /// The account or key within it.
        key: String,
    },
}

impl EnvValue {
    /// A literal value.
    ///
    /// Shorthand, because the struct variant reads badly at a call site that is passing `"8.3"`.
    #[must_use]
    pub fn literal(value: impl Into<String>) -> Self {
        Self::Literal {
            value: value.into(),
        }
    }

    /// A value the supervisor will fetch from the keyring.
    #[must_use]
    pub fn keyring(service: impl Into<String>, key: impl Into<String>) -> Self {
        Self::Keyring {
            service: service.into(),
            key: key.into(),
        }
    }
}

impl<'de> serde::Deserialize<'de> for EnvValue {
    /// Reads the tagged form this type writes, and a bare string besides — see [`EnvValue`].
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(EnvValueVisitor)
    }
}

/// The two shapes [`EnvValue`] accepts on the way in.
struct EnvValueVisitor;

impl<'de> serde::de::Visitor<'de> for EnvValueVisitor {
    type Value = EnvValue;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a literal value as a string, or a table with `from = \"literal\"` or `from = \"keyring\"`",
        )
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(EnvValue::literal(value))
    }

    fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(EnvValue::Literal { value })
    }

    /// Collected field by field rather than by `#[serde(tag = …)]` so that the one dangerous
    /// combination can be named.
    ///
    /// A `value` beside `from = "keyring"` is refused rather than ignored: it is the single mistake
    /// that puts a password into a spec, and an impl that dropped the field silently would let one
    /// sit in a manifest looking like it worked, then travel with everything the spec is copied
    /// into. Unknown fields are refused for the smaller version of the same reason — `secret = …`
    /// should not look accepted.
    fn visit_map<M: serde::de::MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
        use serde::de::Error as _;

        const FIELDS: &[&str] = &["from", "value", "service", "key"];

        let (mut from, mut value, mut service, mut key): (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = (None, None, None, None);

        while let Some(field) = map.next_key::<String>()? {
            let (slot, name) = match field.as_str() {
                "from" => (&mut from, "from"),
                "value" => (&mut value, "value"),
                "service" => (&mut service, "service"),
                "key" => (&mut key, "key"),
                other => return Err(M::Error::unknown_field(other, FIELDS)),
            };

            if slot.is_some() {
                return Err(M::Error::duplicate_field(name));
            }
            *slot = Some(map.next_value()?);
        }

        match from.as_deref() {
            None | Some("literal") => {
                if service.is_some() || key.is_some() {
                    return Err(M::Error::custom(
                        "a literal value names no keyring entry, so `service` and `key` mean nothing beside it",
                    ));
                }

                Ok(EnvValue::Literal {
                    value: value.ok_or_else(|| M::Error::missing_field("value"))?,
                })
            }
            Some("keyring") => {
                if value.is_some() {
                    return Err(M::Error::custom(
                        "`from = \"keyring\"` names a credential instead of carrying one, so a `value` beside it is a password written into a spec",
                    ));
                }

                Ok(EnvValue::Keyring {
                    service: service.ok_or_else(|| M::Error::missing_field("service"))?,
                    key: key.ok_or_else(|| M::Error::missing_field("key"))?,
                })
            }
            Some(other) => Err(M::Error::unknown_variant(other, &["literal", "keyring"])),
        }
    }
}

/// Everything needed to run and babysit one process.
///
/// Built through [`ServiceSpec::builder`], which is where the invariants are checked — see
/// [`ServiceSpecBuilder::build`].
///
/// **The fields are private and read through accessors**, which is what makes that sentence true
/// rather than merely intended: a public `program` could be reassigned to a relative path after
/// `build` refused one, and a public `depends_on` could be pushed onto until it named the service
/// itself. Nothing needs to mutate a spec — a changed service is a new spec, built and checked the
/// same way — so the ability to is only the ability to get it wrong. Private fields also leave
/// limits, idle stopping and log retention free to grow the struct, which is what
/// `#[non_exhaustive]` would otherwise be here for.
///
/// **Deserialisation does not run those checks.** A spec arriving from SQLite or from an
/// `extension.toml` is validated by whoever loads it, at the point where a bad one can be reported
/// against its source; a `Deserialize` impl that refused would fail with no idea which row or which
/// file it was reading. [`ServiceId`] is the exception, and validates either way, because it becomes
/// a directory name. What a loader calls is [`ServiceSpec::validate`] — the same function `build`
/// runs, so there is one definition of "usable" rather than a second one written out by hand.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceSpec {
    /// What this service is called, everywhere.
    id: ServiceId,

    /// The binary to execute. Absolute — a spec is never resolved against a `PATH`, because which
    /// `PATH` would be ambiguous exactly where it matters.
    program: PathBuf,

    /// Arguments, already split. Never a command line to be parsed, so a data directory with a space
    /// in it needs no quoting and no escaping rule to get wrong.
    args: Vec<String>,

    /// The child's entire environment.
    ///
    /// **Not inherited wholesale.** The daemon's own environment carries whatever launched it —
    /// a shell's `PATH`, another version manager's hooks, variables from a login session — and a
    /// service that behaves differently depending on how the daemon was started is a bug nobody can
    /// reproduce.
    env: BTreeMap<String, EnvValue>,

    /// The working directory. Also where a relative path in the service's own config resolves from,
    /// which is why it is explicit rather than inherited.
    cwd: PathBuf,

    /// The TCP ports this service will listen on, as the recipe that rendered it knows them.
    ///
    /// **Declared rather than derived, and empty is a real answer.** A `ReadyCheck` names an address
    /// only for the services that are checked by connecting to one — a database checked by running a
    /// query names none, and a web server's admin endpoint is not the port its sites are served on.
    /// The recipe wrote the number into the configuration and is the only thing that knows it.
    ///
    /// What reads this is the diagnosis of a failed start: the daemon asks the OS who is listening
    /// on each of these before it decides what to call the failure. Nothing reserves a port by
    /// declaring it here, and nothing binds one — see `service.create` for where a port is
    /// *allocated*.
    #[serde(default)]
    ports: Vec<u16>,

    /// When traffic may be routed to it.
    ready: ReadyCheck,

    /// Whether it is still fine, once it is ready. `None` for a service with nothing cheap to poll.
    health: Option<HealthCheck>,

    /// What to do when it exits.
    restart: RestartPolicy,

    /// How to ask it to stop.
    stop: StopBehaviour,

    /// How to hand it a configuration that changed while it is running. `None` for a service that
    /// reads its configuration once.
    reload: Option<ReloadBehaviour>,

    /// Services that must be ready before this one starts, and that stop after it.
    ///
    /// The edges of a DAG; a cycle among several specs is caught when they are assembled into a
    /// graph. The one case a single spec can see for itself — depending on itself — is rejected by
    /// [`ServiceSpecBuilder::build`].
    depends_on: Vec<ServiceId>,

    /// CPU and memory ceilings, enforced where the service runs.
    limits: ResourceLimits,

    /// Whether a memory watchdog may restart this service.
    ///
    /// **The recipe's answer and not a person's.** Whether a program survives being restarted under
    /// memory pressure is a property of the program: a php-fpm pool loses the requests in flight,
    /// which `pm.max_requests` already recycles workers underneath, and a database loses a
    /// transaction. `false` for everything that does not say otherwise, so the permission is opted
    /// into and never inherited.
    ///
    /// It does nothing on its own: nothing watches a service that has declared no `memory_mb`, and
    /// nothing watches any service on a machine whose kernel holds the ceiling itself.
    #[serde(default)]
    restart_over_memory: bool,

    /// When to stop it for being unused, which the daemon's idle sweep acts on.
    idle: Option<IdlePolicy>,

    /// How much of its output to keep, which the supervisor's log capture applies.
    logs: LogPolicy,
}

impl ServiceSpec {
    /// What this service is called, everywhere.
    #[must_use]
    pub fn id(&self) -> &ServiceId {
        &self.id
    }

    /// The binary to execute. Always absolute.
    #[must_use]
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Its arguments, already split.
    #[must_use]
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// The child's entire environment — not merged with the daemon's.
    #[must_use]
    pub fn env(&self) -> &BTreeMap<String, EnvValue> {
        &self.env
    }

    /// The working directory. Always absolute.
    #[must_use]
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// The TCP ports it will listen on, as its recipe declared them. Empty for a service that
    /// binds nothing.
    #[must_use]
    pub fn ports(&self) -> &[u16] {
        &self.ports
    }

    /// When traffic may be routed to it.
    #[must_use]
    pub fn ready(&self) -> &ReadyCheck {
        &self.ready
    }

    /// Whether it is still fine, once it is ready.
    #[must_use]
    pub fn health(&self) -> Option<&HealthCheck> {
        self.health.as_ref()
    }

    /// What to do when it exits.
    #[must_use]
    pub fn restart(&self) -> RestartPolicy {
        self.restart
    }

    /// How to ask it to stop.
    #[must_use]
    pub fn stop(&self) -> &StopBehaviour {
        &self.stop
    }

    /// How to hand it a configuration that changed while it is running, if it can take one.
    #[must_use]
    pub fn reload(&self) -> Option<&ReloadBehaviour> {
        self.reload.as_ref()
    }

    /// Services that must be ready before this one starts, and that stop after it.
    #[must_use]
    pub fn depends_on(&self) -> &[ServiceId] {
        &self.depends_on
    }

    /// CPU and memory ceilings, enforced where the service runs.
    #[must_use]
    pub fn limits(&self) -> ResourceLimits {
        self.limits
    }

    /// Whether a memory watchdog may restart this service. See the field.
    pub const fn restart_over_memory(&self) -> bool {
        self.restart_over_memory
    }

    /// When to stop it for being unused, which the daemon's idle sweep acts on.
    #[must_use]
    pub fn idle(&self) -> Option<&IdlePolicy> {
        self.idle.as_ref()
    }

    /// How much of its output to keep, which the supervisor's log capture applies.
    #[must_use]
    pub fn logs(&self) -> LogPolicy {
        self.logs
    }

    /// Start describing a service.
    ///
    /// The two arguments are the two things with no possible default. Everything else has one, apart
    /// from [`cwd`](ServiceSpecBuilder::cwd) and [`ready`](ServiceSpecBuilder::ready), which
    /// [`ServiceSpecBuilder::build`] insists on: a wrong working directory is a subtle bug, and a
    /// missing readiness check would silently become "assume it worked".
    pub fn builder(id: ServiceId, program: impl Into<PathBuf>) -> ServiceSpecBuilder {
        ServiceSpecBuilder {
            id,
            program: program.into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            ports: Vec::new(),
            ready: None,
            health: None,
            restart: RestartPolicy::default(),
            stop: StopBehaviour::default(),
            reload: None,
            depends_on: Vec::new(),
            limits: ResourceLimits::default(),
            restart_over_memory: false,
            idle: None,
            logs: LogPolicy::default(),
        }
    }
}

/// Builds a [`ServiceSpec`], checking it once at the end.
///
/// Every setter overwrites; none can fail. Collecting the mistakes and reporting them from
/// [`build`](ServiceSpecBuilder::build) keeps the call site readable, and means a spec that exists is
/// a spec that was checked.
#[derive(Debug, Clone)]
#[must_use = "a builder does nothing until `build` is called"]
pub struct ServiceSpecBuilder {
    id: ServiceId,
    program: PathBuf,
    args: Vec<String>,
    env: BTreeMap<String, EnvValue>,
    cwd: Option<PathBuf>,
    ports: Vec<u16>,
    ready: Option<ReadyCheck>,
    health: Option<HealthCheck>,
    restart: RestartPolicy,
    stop: StopBehaviour,
    reload: Option<ReloadBehaviour>,
    depends_on: Vec<ServiceId>,
    limits: ResourceLimits,
    restart_over_memory: bool,
    idle: Option<IdlePolicy>,
    logs: LogPolicy,
}

impl ServiceSpecBuilder {
    /// Append one argument.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Append several arguments.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Set an environment variable to a literal value.
    ///
    /// For anything that is not a secret. Use [`env_from_keyring`](Self::env_from_keyring) when it
    /// is — see [`EnvValue`].
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), EnvValue::literal(value));
        self
    }

    /// Set an environment variable to a credential the supervisor fetches at spawn time.
    pub fn env_from_keyring(
        mut self,
        key: impl Into<String>,
        service: impl Into<String>,
        entry: impl Into<String>,
    ) -> Self {
        self.env
            .insert(key.into(), EnvValue::keyring(service, entry));
        self
    }

    /// Set the working directory. Required.
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Declare the TCP ports this service will listen on. Defaults to none.
    ///
    /// For a recipe to say what it wrote into the configuration, so that a failed start can be
    /// diagnosed against it — see [`ServiceSpec::ports`].
    pub fn ports(mut self, ports: impl IntoIterator<Item = u16>) -> Self {
        self.ports = ports.into_iter().collect();
        self
    }

    /// Set the readiness check. Required.
    pub fn ready(mut self, ready: ReadyCheck) -> Self {
        self.ready = Some(ready);
        self
    }

    /// Set the periodic health check.
    pub fn health(mut self, health: HealthCheck) -> Self {
        self.health = Some(health);
        self
    }

    /// Set the restart policy. Defaults to [`RestartPolicy::default`].
    pub fn restart(mut self, restart: RestartPolicy) -> Self {
        self.restart = restart;
        self
    }

    /// Set the stop behaviour. Defaults to [`StopBehaviour::default`].
    pub fn stop(mut self, stop: StopBehaviour) -> Self {
        self.stop = stop;
        self
    }

    /// Set how a configuration that changed reaches it. Defaults to no reload at all, which is a
    /// service a changed configuration only reaches by being restarted.
    pub fn reload(mut self, reload: ReloadBehaviour) -> Self {
        self.reload = Some(reload);
        self
    }

    /// Append a dependency that must be ready first.
    pub fn depends_on(mut self, id: ServiceId) -> Self {
        self.depends_on.push(id);
        self
    }

    /// Set the resource limits. Defaults to uncapped at normal priority.
    pub fn limits(mut self, limits: ResourceLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Let a memory watchdog restart this service when it stays over its ceiling.
    ///
    /// Defaults to `false`. Set from `Recipe::restart_over_memory_default`, which is the only caller
    /// that should have an opinion: this is a fact about the program, not a preference about it.
    pub const fn restart_over_memory(mut self, may: bool) -> Self {
        self.restart_over_memory = may;
        self
    }

    /// Set the idle policy. Defaults to never idle-stopping.
    pub fn idle(mut self, idle: IdlePolicy) -> Self {
        self.idle = Some(idle);
        self
    }

    /// Set the log policy. Defaults to [`LogPolicy::default`].
    pub fn logs(mut self, logs: LogPolicy) -> Self {
        self.logs = logs;
        self
    }

    /// Check everything and produce the spec.
    ///
    /// Only the two fields with no default are checked here, because only the builder can tell a
    /// field that was left out from one that was set to something unusable — once the spec exists,
    /// `cwd` is a `PathBuf` either way. Everything else is [`ServiceSpec::validate`], which is also
    /// what a spec that arrived by `Deserialize` is put through.
    ///
    /// # Errors
    ///
    /// [`SpecError`], naming the field at fault.
    pub fn build(self) -> Result<ServiceSpec, SpecError> {
        let id = self.id;
        let missing = |field: &str| SpecError::Missing {
            id: id.clone(),
            field: field.to_owned(),
        };

        let cwd = self.cwd.ok_or_else(|| missing("cwd"))?;
        let ready = self.ready.ok_or_else(|| missing("ready"))?;

        // Here rather than in the setter, for the reason every other check in this function is
        // here: a setter that could fail would make the builder's chain unusable, and collecting
        // the checks at the end is what makes "a spec that exists is a spec that was checked" true.
        self.limits
            .validate()
            .map_err(|reason| SpecError::Invalid {
                id: id.clone(),
                field: "limits".to_owned(),
                reason: reason.to_owned(),
            })?;

        let spec = ServiceSpec {
            id,
            program: self.program,
            args: self.args,
            env: self.env,
            cwd,
            ports: self.ports,
            ready,
            health: self.health,
            restart: self.restart,
            stop: self.stop,
            reload: self.reload,
            depends_on: self.depends_on,
            limits: self.limits,
            restart_over_memory: self.restart_over_memory,
            idle: self.idle,
            logs: self.logs,
        };
        spec.validate()?;

        Ok(spec)
    }
}

/// A spec that could not be built, or an identifier that could not be parsed.
///
/// This crate's own `thiserror` enum, separate from [`crate::Error`]: that one is the wire failure,
/// and `docs/architecture/daemon-and-ipc.md` reserves deciding a code and writing a hint for the
/// daemon boundary. Every variant here maps to `invalid_argument` there.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SpecError {
    /// A service id does not have the shape of one.
    #[error("`{value}` is not a valid service id: {reason}")]
    ServiceId {
        /// What was offered.
        value: String,
        /// Why it was refused, as a clause that completes the sentence.
        reason: String,
    },

    /// A field with no default was never set.
    #[error("service `{id}` needs a `{field}`")]
    Missing {
        /// The service being described.
        id: ServiceId,
        /// The field that was left out.
        field: String,
    },

    /// A field was set to something that cannot work.
    #[error("service `{id}` has an unusable `{field}`: {reason}")]
    Invalid {
        /// The service being described.
        id: ServiceId,
        /// The field at fault.
        field: String,
        /// Why it cannot work, as a clause that completes the sentence.
        reason: String,
    },
}

/// Builds a path the tests below can rely on being absolute on every target.
///
/// Test-only: it exists because `/opt/mixengine/caddy` is not absolute on Windows and
/// `C:\MixEngine\caddy` is not a path on Unix, and a spec insists on absolute paths.
#[cfg(test)]
fn absolute(relative: &str) -> PathBuf {
    let root = if cfg!(windows) {
        r"C:\MixEngine"
    } else {
        "/opt/mixengine"
    };

    std::path::Path::new(root).join(relative)
}

#[cfg(test)]
mod tests;
