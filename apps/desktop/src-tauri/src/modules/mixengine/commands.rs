//! The commands this module exposes to the frontend.
//!
//! There is no business logic here: each command is one JSON-RPC call or one question about state,
//! and the logic stays on the daemon's side. That is the `CLAUDE.md` rule MixEngine holds every one
//! of its clients to, and it is why the screens on this side can draw from data instead of
//! inferring it themselves.

use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::State;

use crate::error::AppError;

use super::state::MixEngineState;
use super::{events, health, rpc};

/// Whether the daemon is running, silent, stopped or not installed — and if not installed, where
/// we looked.
#[tauri::command]
pub async fn mixengine_presence() -> health::PresenceReport {
    health::presence().await
}

/// Where the daemon keeps its growing directories, and whether that can still be changed — roadmap
/// task **T146**.
///
/// `Value`, for the same reason as the two reads just below: Rust reads no field of this answer.
#[tauri::command]
pub async fn mixengine_storage() -> Result<Value, AppError> {
    health::storage().await
}

/// Starts the daemon. Returns the endpoint it prints once it is ready.
///
/// `chosen` is the four directories the user just picked at the gate, if any.
#[tauri::command]
pub async fn mixengine_start(chosen: Option<health::ChosenPaths>) -> Result<String, AppError> {
    health::start_daemon(chosen).await
}

/// Both reads below return a bare `Value`, not decoded into a struct of MixLab's own.
///
/// Rust reads no field in these two answers — it forwards them. A struct here would be a second
/// hand-written copy of a contract that already has a generated one in
/// `src/modules/mixengine/api/types/`, and the first hand-written copy was wrong from the start: it
/// lacked `last_started_at` and `last_exit_code`, so a frontend typed as `ServiceSummary` would get
/// `undefined` for fields the contract says are there.
#[tauri::command]
pub async fn mixengine_status() -> Result<Value, AppError> {
    rpc::call("daemon.status", json!({})).await
}

#[tauri::command]
pub async fn mixengine_services() -> Result<Value, AppError> {
    rpc::call("service.list", json!({})).await
}

/// The method and params of an action on a service. Split out of the command so it can be tested:
/// a mistake here does not produce an error, it produces the right action on the wrong target.
///
/// **The field is `service`.** `ServiceTarget` — the params shared by all three methods — declares
/// `service?: ServiceId | null` and says plainly that absence means *every declared service*.
/// Sending `id` once turned every Start click on a row into a start-everything, without a single
/// error.
///
/// **`wait: true`, even though `ServiceTarget`'s doc says "A GUI sends `false`."** That advice
/// assumes a GUI drawn *entirely* from the stream. This Dashboard is not: `act()` calls `reload()`
/// as soon as the call returns. With `wait: false` the call returns at once, so `reload()` reads
/// `service.list` **while the plan is still running** and overwrites the state the stream just
/// applied with older figures — the row jumps back to its old state and only corrects itself when
/// the next event arrives.
///
/// With `wait: true`, the call returns once the plan is done: `reload()` reads the truth, and the
/// "starting…" label stays up for that whole time instead of vanishing at once. Waiting here does
/// not freeze the window — it is `async`, and only that row's buttons are locked.
///
/// Switching to `false` is only right once `act()` stops calling `reload()` itself and the stream
/// becomes the only path.
fn service_action_call(id: &str, action: &str) -> Result<(&'static str, Value), AppError> {
    let method = match action {
        "start" => "service.start",
        "stop" => "service.stop",
        "restart" => "service.restart",
        // Not the user's mistake: the frontend is the only caller of this command, so an unknown
        // `action` is a programming error and goes out as a protocol error.
        other => {
            return Err(err!(
                "error.mixengineProtocol",
                message = format!("no service action `{other}`")
            ))
        }
    };
    Ok((method, json!({ "service": id, "wait": true })))
}

/// `start`, `stop` or `restart` a service.
///
/// These three methods are MixEngine's only exception that take `wait` instead of returning a job.
#[tauri::command]
pub async fn mixengine_service_action(id: String, action: String) -> Result<Value, AppError> {
    let (method, params) = service_action_call(&id, &action)?;
    rpc::call(method, params).await
}

/// `service.start` with a **project scope** — T125: every service this project needs, in
/// dependency order.
///
/// **The frontend must not work out that set of services itself.** That would be business logic in
/// a client (`CLAUDE.md`), and on a second apply it is wrong as well: the web server a site needs
/// is the one the plan *found*, not the one it created. So the question goes to the daemon
/// verbatim, and the daemon is what reads `site_service_links` along with the home's front end.
///
/// **Before T125 this was `mixengine_service_start_all`, sending an empty target** — meaning *every
/// service this home declares*. A home with four PHP versions and three databases started all eight
/// services to bring up one site, and that is exactly why this command takes a parameter.
///
/// A command of its own rather than `mixengine_service_action` with an empty `id`: "one service"
/// and "what a project needs" are two different questions, and an empty `id` is where a typo turns
/// into starting the whole machine.
#[tauri::command]
pub async fn mixengine_service_start_project(project: String) -> Result<Value, AppError> {
    rpc::call("service.start", service_start_project_params(&project)).await
}

/// Params for [`mixengine_service_start_project`]. Split out so it can be tested: a mistake here
/// does not produce an error, it produces the right start on the wrong scope.
///
/// **`ProjectRef` is an externally tagged enum**, so the project name travels in `{"name": …}`.
/// Sending a bare string as `project` is a request the daemon refuses, and sending an empty
/// `project` is a target with no scope — that is, starting the whole machine, exactly what T125
/// removed.
fn service_start_project_params(project: &str) -> Value {
    json!({ "project": { "name": project }, "wait": true })
}

/// Opens the event stream. Opening again closes the one already open.
///
/// Only the calling window's: each webview has its own stream since T168a.
#[tauri::command]
pub async fn mixengine_watch(
    webview: tauri::WebviewWindow,
    on_event: Channel<String>,
    state: State<'_, MixEngineState>,
) -> Result<(), AppError> {
    events::stream_events(on_event, webview.label(), &state).await
}

/// Closes the stream. Calling it with nothing open is harmless — the cleanup of an effect that runs
/// twice under StrictMode.
#[tauri::command]
pub fn mixengine_unwatch(webview: tauri::WebviewWindow, state: State<'_, MixEngineState>) {
    state.stop(webview.label());
}

/// Every operation waiting for administrator rights, with the description the daemon wrote for
/// each.
///
/// `daemon.status` carries only a number (`ElevationSummary.pending`); the actual list is here. A
/// tab opened while operations are already waiting receives no `elevation_required` — that event
/// only fires when the queue changes — so this is the only way to see them.
#[tauri::command]
pub async fn mixengine_elevation_status() -> Result<Value, AppError> {
    rpc::call("elevation.status", json!({})).await
}

/// Grants the whole batch of waiting operations. Raises exactly one OS prompt.
#[tauri::command]
pub async fn mixengine_elevation_grant() -> Result<Value, AppError> {
    rpc::call("elevation.grant", json!({})).await
}

/// Drops the whole batch. Declining is an outcome the API models, not an error.
#[tauri::command]
pub async fn mixengine_elevation_drop() -> Result<Value, AppError> {
    rpc::call("elevation.drop", json!({})).await
}

/// `project` filters by name; leave it empty to see every site in the home.
#[tauri::command]
pub async fn mixengine_sites(project: Option<String>) -> Result<Value, AppError> {
    let params = match project {
        Some(name) => json!({ "project": { "name": name } }),
        None => json!({}),
    };
    rpc::call("site.list", params).await
}

/// Always looks up by domain — MixLab does not use `SiteRef::Path`; only a CLI standing in a
/// directory needs it.
#[tauri::command]
pub async fn mixengine_site(domain: String) -> Result<Value, AppError> {
    rpc::call("site.show", json!({ "site": { "domain": domain } })).await
}

/// `params` already has the shape of `SiteCreate` from the frontend — not decoded into a Rust
/// struct of its own; that would be a second hand-written copy of a contract already typed
/// correctly in TypeScript (Phase 2 spec, section 5).
#[tauri::command]
pub async fn mixengine_site_create(params: Value) -> Result<Value, AppError> {
    rpc::call("site.create", params).await
}

/// `params` has the shape of `SiteUpdate`.
#[tauri::command]
pub async fn mixengine_site_update(params: Value) -> Result<Value, AppError> {
    rpc::call("site.update", params).await
}

/// `params` has the shape of `SiteShare`.
#[tauri::command]
pub async fn mixengine_site_share(params: Value) -> Result<Value, AppError> {
    rpc::call("site.share", params).await
}

#[tauri::command]
pub async fn mixengine_site_unshare(domain: String) -> Result<Value, AppError> {
    rpc::call("site.unshare", json!({ "site": { "domain": domain } })).await
}

/// Only to build the project dropdown in the create-site dialog — not a Projects screen.
#[tauri::command]
pub async fn mixengine_projects() -> Result<Value, AppError> {
    rpc::call("project.list", json!({})).await
}

/// Looks up a project by name, with its **effective** pins (the file wins over the row).
#[tauri::command]
pub async fn mixengine_project_show(name: String) -> Result<Value, AppError> {
    rpc::call("project.show", json!({ "project": { "name": name } })).await
}

/// `params` has the shape of `ProjectCreate` from the frontend — not decoded into a Rust struct of
/// its own, for the same reason `mixengine_site_create` follows (Phase 2 spec, section 5).
#[tauri::command]
pub async fn mixengine_project_create(params: Value) -> Result<Value, AppError> {
    rpc::call("project.create", params).await
}

/// `params` has the shape of `ProjectUpdate`. `pins` replaces everything — the frontend sends back
/// every existing pin plus the change, not just the new pin.
#[tauri::command]
pub async fn mixengine_project_update(params: Value) -> Result<Value, AppError> {
    rpc::call("project.update", params).await
}

/// Removes the registration — the directory and `mixengine.toml` are kept
/// (`ProjectRemoval.root_kept`/`manifest_kept`), and the UI has to say so in the confirmation
/// dialog.
#[tauri::command]
pub async fn mixengine_project_delete(name: String) -> Result<Value, AppError> {
    rpc::call("project.delete", json!({ "project": { "name": name } })).await
}

/// `domain.dns_status` is both `domain.list` and the diagnosis of one name — leave `domain` empty
/// to see every name.
#[tauri::command]
pub async fn mixengine_domains(domain: Option<String>) -> Result<Value, AppError> {
    rpc::call("domain.dns_status", json!({ "domain": domain })).await
}

#[tauri::command]
pub async fn mixengine_domain_add(params: Value) -> Result<Value, AppError> {
    rpc::call("domain.add", params).await
}

#[tauri::command]
pub async fn mixengine_domain_remove(domain: String) -> Result<Value, AppError> {
    rpc::call("domain.remove", json!({ "domain": domain })).await
}

#[tauri::command]
pub async fn mixengine_ca_status() -> Result<Value, AppError> {
    rpc::call("cert.ca_status", json!({})).await
}

/// Repairs the "browser" half of the CA. `params` has the shape of `DoctorRepair { grant }` — the
/// original assumption that repairing the NSS database needs no administrator rights was wrong on a
/// real machine, so this goes through the same two-step T64 flow as `mixengine_doctor_repair` /
/// `elevation.*` on the Dashboard, instead of hard-coding `grant: true`.
#[tauri::command]
pub async fn mixengine_ca_repair(params: Value) -> Result<Value, AppError> {
    rpc::call("daemon.doctor_repair", params).await
}

/// Leave `domain` empty to issue for every site that declares HTTPS — one call both draws the table
/// and reissues.
#[tauri::command]
pub async fn mixengine_certs(domain: Option<String>) -> Result<Value, AppError> {
    let site = domain.map(|d| json!({ "domain": d }));
    rpc::call("cert.issue", json!({ "site": site })).await
}

/// `filter` has the shape of `RuntimeFilter` — empty (`{}`) shows all four kinds.
#[tauri::command]
pub async fn mixengine_runtime_list_installed(filter: Value) -> Result<Value, AppError> {
    rpc::call("runtime.list_installed", filter).await
}

/// `filter` has the shape of `RuntimeFilter`. `RuntimeCatalogue.stale` must be drawn by the
/// frontend, not ignored — see D3.
#[tauri::command]
pub async fn mixengine_runtime_list_available(filter: Value) -> Result<Value, AppError> {
    rpc::call("runtime.list_available", filter).await
}

/// `target` is a `RuntimeInstall` — a `RuntimeTarget` plus `install_prerequisites` /
/// `ignore_requirements`. Answers `JobSummary`, whose id the frontend follows over the `/events`
/// stream it already has open.
#[tauri::command]
pub async fn mixengine_runtime_install(target: Value) -> Result<Value, AppError> {
    rpc::call("runtime.install", target).await
}

/// `target` is a `RuntimeTarget`. Answers `Requirements`: what that version lacks on this machine,
/// asked before `runtime.install` so the window can ask the person once — T151.
#[tauri::command]
pub async fn mixengine_runtime_requirements(target: Value) -> Result<Value, AppError> {
    rpc::call("runtime.requirements", target).await
}

/// `params` has the shape of `RuntimeUninstall { kind, version, force? }`.
#[tauri::command]
pub async fn mixengine_runtime_uninstall(params: Value) -> Result<Value, AppError> {
    rpc::call("runtime.uninstall", params).await
}

/// `target` has the shape of `RuntimeTarget`.
#[tauri::command]
pub async fn mixengine_runtime_set_default(target: Value) -> Result<Value, AppError> {
    rpc::call("runtime.set_default", target).await
}

/// `target` has the shape of `RuntimeTarget` — one PHP version; returns `RuntimeExtension[]`.
#[tauri::command]
pub async fn mixengine_runtime_list_extensions(target: Value) -> Result<Value, AppError> {
    rpc::call("runtime.list_extensions", target).await
}

/// `choice` has the shape of `ExtensionChoice { kind, version, name, enabled }`. Returns
/// `ExtensionChange { extension, pool }` — `pool` is what the frontend reads to decide which banner
/// to show.
#[tauri::command]
pub async fn mixengine_runtime_set_extension(choice: Value) -> Result<Value, AppError> {
    rpc::call("runtime.set_extension", choice).await
}

/// `filter` has the shape of `PackageFilter { package? }`.
#[tauri::command]
pub async fn mixengine_package_list(filter: Value) -> Result<Value, AppError> {
    rpc::call("package.list", filter).await
}

/// `filter` has the shape of `PackageFilter`. `PackageCatalogue.stale` must be drawn, by the same
/// component as `RuntimeCatalogue.stale` — D3.
#[tauri::command]
pub async fn mixengine_package_list_available(filter: Value) -> Result<Value, AppError> {
    rpc::call("package.list_available", filter).await
}

/// `target` is a `PackageInstall` — a `PackageTarget` plus `install_prerequisites` /
/// `ignore_requirements`. Answers `JobSummary`, followed over the stream as `runtime.install` is.
#[tauri::command]
pub async fn mixengine_package_install(target: Value) -> Result<Value, AppError> {
    rpc::call("package.install", target).await
}

/// `target` is a `PackageTarget`. `mixengine_runtime_requirements` for a service package — T151.
#[tauri::command]
pub async fn mixengine_package_requirements(target: Value) -> Result<Value, AppError> {
    rpc::call("package.requirements", target).await
}

/// `target` has the shape of `PackageTarget`. **There is no `force`** — refusing because `services`
/// is not empty is final; no parameter overrides it (D6).
#[tauri::command]
pub async fn mixengine_package_uninstall(target: Value) -> Result<Value, AppError> {
    rpc::call("package.uninstall", target).await
}

/// `query` is a `RuntimeUpgradeQuery { kind, from, to? }`. Answers `UpgradePlan` and changes
/// nothing — T193b.
#[tauri::command]
pub async fn mixengine_runtime_upgrade_plan(query: Value) -> Result<Value, AppError> {
    rpc::call("runtime.upgrade_plan", query).await
}

/// `asked` is a `RuntimeUpgrade`. Answers `JobSummary`, followed over the stream as
/// `runtime.install` is; the finished job's result is the plan, marked — T193b.
#[tauri::command]
pub async fn mixengine_runtime_upgrade(asked: Value) -> Result<Value, AppError> {
    rpc::call("runtime.upgrade", asked).await
}

/// `query` is a `PackageUpgradeQuery { package, from, to? }`. Answers `UpgradePlan` — T193c.
#[tauri::command]
pub async fn mixengine_package_upgrade_plan(query: Value) -> Result<Value, AppError> {
    rpc::call("package.upgrade_plan", query).await
}

/// `asked` is a `PackageUpgrade`; the window sends `grant: true`, because the person has just
/// agreed to a plan that says the machine may ask — T193c.
#[tauri::command]
pub async fn mixengine_package_upgrade(asked: Value) -> Result<Value, AppError> {
    rpc::call("package.upgrade", asked).await
}

/// `service` is a bare `ServiceId` (a string).
#[tauri::command]
pub async fn mixengine_service_limits(service: String) -> Result<Value, AppError> {
    rpc::call("service.limits", json!({ "service": service })).await
}

/// `params` has the shape of `ServiceLimitsSet { service, limits }` — `limits` must be the whole of
/// `ResourceLimits`, not part of it: any field left out is cleared, exactly as `ServiceLimitsSet`'s
/// doc says. The frontend is responsible for sending all of it.
#[tauri::command]
pub async fn mixengine_service_set_limits(params: Value) -> Result<Value, AppError> {
    rpc::call("service.set_limits", params).await
}

/// Reads the current idle policy. **There is no vendored TypeScript type for this answer yet** —
/// confirm the real shape when running against a real daemon (Task 8).
#[tauri::command]
pub async fn mixengine_service_idle(service: String) -> Result<Value, AppError> {
    rpc::call("service.idle", json!({ "service": service })).await
}

/// `params` has the shape of `ServiceIdleSet { service, minutes? }` — three states: absent (follow
/// the recipe), `0` (off entirely), `n` (n minutes).
#[tauri::command]
pub async fn mixengine_service_set_idle(params: Value) -> Result<Value, AppError> {
    rpc::call("service.set_idle", params).await
}

/// `params` has the shape of `ServiceAutostartSet { service, autostart }` — T112. Two states, not
/// three: `service.set_idle` has three because absence means "follow the recipe", whereas no recipe
/// declares autostart. Returns `ServiceSummary` — that very service, as it has just become.
///
/// **Starts nothing and stops nothing.** What it changes is the walk at the *next* daemon start.
#[tauri::command]
pub async fn mixengine_service_set_autostart(params: Value) -> Result<Value, AppError> {
    rpc::call("service.set_autostart", params).await
}

/// `service.save_resources` — T167b / ADR 0041. Returns `SaveResources { on }`: whether this home
/// stops services nobody is using. Off unless the user has turned it on.
#[tauri::command]
pub async fn mixengine_service_save_resources() -> Result<Value, AppError> {
    rpc::call("service.save_resources", json!({})).await
}

/// `params` has the shape of `SaveResourcesSet { on }` — turns "Save battery" on or off. Stops
/// nothing and starts nothing: the next idle sweep is what reads the switch.
#[tauri::command]
pub async fn mixengine_service_set_save_resources(params: Value) -> Result<Value, AppError> {
    rpc::call("service.set_save_resources", params).await
}

/// `params` has the shape of `CredentialStoreSet { store }` — `daemon.set_credential_store`, T194.
/// Records the store the next start uses; the running daemon keeps its own.
#[tauri::command]
pub async fn mixengine_daemon_set_credential_store(params: Value) -> Result<Value, AppError> {
    rpc::call("daemon.set_credential_store", params).await
}

/// `service.set_front_end` — T97 / ADR 0026. `params` has the shape of `FrontEndSwitch { server,
/// version?, grant }`; returns a `JobSummary` (stopping the old server and bringing up the new one
/// is one job), whose result is `FrontEndReport`. There is no separate read method: the active
/// server is read from `ServiceSummary.role`.
#[tauri::command]
pub async fn mixengine_service_set_front_end(params: Value) -> Result<Value, AppError> {
    rpc::call("service.set_front_end", params).await
}

/// `params` has the shape of `ServiceCreate { id, version, port?, bind_addr?, data_dir?,
/// autostart?, overrides? }` — not decoded into a Rust struct of its own, for the same reason
/// `mixengine_site_create` follows. Returns `ServiceCreation { service, moved_from? }`:
/// `moved_from` is only true at this moment, so it lives here and not in `service.list`.
#[tauri::command]
pub async fn mixengine_service_create(params: Value) -> Result<Value, AppError> {
    rpc::call("service.create", params).await
}

/// `params` has the shape of `ServiceDelete { service, force? }`. Returns `ServiceRemoval {
/// removed, data_kept? }` — `force` only overrides a site that declares this service, never a
/// running process; the data directory is never deleted, only named if there is one.
#[tauri::command]
pub async fn mixengine_service_delete(params: Value) -> Result<Value, AppError> {
    rpc::call("service.delete", params).await
}

/// `params` has the shape of `DatabaseCreate { service, database, user? }`.
#[tauri::command]
pub async fn mixengine_database_create(params: Value) -> Result<Value, AppError> {
    rpc::call("database.create", params).await
}

/// `service` is a bare `ServiceId`. Read-only, starts nothing — used to draw the "Open" affordance
/// before knowing whether it can be clicked.
#[tauri::command]
pub async fn mixengine_database_client(service: String) -> Result<Value, AppError> {
    rpc::call("database.client", json!({ "service": service })).await
}

/// `database.credentials` — the password MixEngine holds for one account.
///
/// **The only method in the whole API that returns the password itself**, and it exists precisely
/// to do that: [ADR 0025] says a credential is answered only by a method that exists to answer it.
/// Every other `database.*` returns the credential's *address* in the credential store, never its
/// value.
///
/// The value goes straight out to the frontend so the user can paste it into their `.env`. Do not
/// log it here: a `tracing` line carrying a password is a password on disk in a place nobody meant
/// it to be.
///
/// An absent `user` means the server's administrator — the default of `database.open` itself,
/// because the two commands are one question, asked by a process and by a person.
///
/// [ADR 0025]: https://github.com/mixnz/mixlab/blob/master/docs/decisions/0025-a-credential-is-answered-only-by-a-method-that-exists-to-answer-it.md
#[tauri::command]
pub async fn mixengine_database_credentials(
    service: String,
    user: Option<String>,
) -> Result<Value, AppError> {
    let mut params = json!({ "service": service });
    if let Some(user) = user {
        params["user"] = json!(user);
    }
    rpc::call("database.credentials", params).await
}

/// `service.reset_credential` — writes the administrator credential back into that database's own
/// data directory, when this home can no longer produce the old password (T127).
///
/// **Not a small operation, and the client must say so before asking**: it stops this service and
/// everything that depends on it, runs the recipe's offline set-password step, then starts them
/// again. What decides this for a person is what it does **not** do — every database in that
/// directory is kept intact.
///
/// `wait: true` like the three start/stop/restart commands: the call returns once the work is done,
/// so the screen reads the truth instead of a state in transition.
#[tauri::command]
pub async fn mixengine_service_reset_credential(service: String) -> Result<Value, AppError> {
    rpc::call(
        "service.reset_credential",
        json!({ "service": service, "wait": true }),
    )
    .await
}

/// `home.previous` — what a copy of an earlier install's state in the kept folders holds (T182h).
#[tauri::command]
pub async fn mixengine_home_previous() -> Result<Value, AppError> {
    rpc::call("home.previous", json!({})).await
}

/// `home.restore` — bring that copy back into this home (T182h). Databases get a new admin
/// password and stay stopped.
#[tauri::command]
pub async fn mixengine_home_restore() -> Result<Value, AppError> {
    rpc::call("home.restore", json!({})).await
}

/// `runtime.found` — runtime directories on disk with no row, and why (T182i). A read.
#[tauri::command]
pub async fn mixengine_runtime_found() -> Result<Value, AppError> {
    rpc::call("runtime.found", json!({})).await
}

/// `runtime.adopt` — record one runtime version that is on disk without a row, checked against the
/// package index (T182f). Nothing is downloaded or removed.
#[tauri::command]
pub async fn mixengine_runtime_adopt(kind: String, version: String) -> Result<Value, AppError> {
    rpc::call("runtime.adopt", json!({ "kind": kind, "version": version })).await
}

/// `package.found` — package directories on disk with no row, and why (T182i). A read.
#[tauri::command]
pub async fn mixengine_package_found() -> Result<Value, AppError> {
    rpc::call("package.found", json!({})).await
}

/// `package.adopt` — record one package version that is on disk without a row, checked against the
/// package index (T182f). Nothing is downloaded or removed.
#[tauri::command]
pub async fn mixengine_package_adopt(package: String, version: String) -> Result<Value, AppError> {
    rpc::call(
        "package.adopt",
        json!({ "package": package, "version": version }),
    )
    .await
}

/// `service.found` — service data an earlier install left under `data/`, and whether each can be
/// adopted (T182g). A read: nothing is written.
#[tauri::command]
pub async fn mixengine_service_found() -> Result<Value, AppError> {
    rpc::call("service.found", json!({})).await
}

/// `service.adopt` — turn one found data directory back into a stopped service with a new admin
/// password (T182g). The databases and accounts in it are kept.
#[tauri::command]
pub async fn mixengine_service_adopt(service: String) -> Result<Value, AppError> {
    rpc::call("service.adopt", json!({ "service": service })).await
}

/// Opens a service's log stream. Opening again (a different service, or the same service with a
/// different `tail`) closes the one already open — the same rule `LogsState::keep` follows for
/// `MixEngineState`.
#[tauri::command]
pub async fn mixengine_logs_watch(
    service: String,
    tail: u32,
    follow: bool,
    on_line: Channel<String>,
    state: State<'_, super::state::LogsState>,
) -> Result<(), AppError> {
    super::logs::stream_logs("service", service, tail, follow, on_line, &state).await
}

/// A job's output (`GET /logs/job/{id}`) — the same `LogsState`, the same "opening again closes the
/// open one" rule the service log follows. Used for the `run_scaffold` step of `blueprint.apply`:
/// this is the only place the scaffold command's real output shows up; `BlueprintApplied` (the job
/// result) does not carry it.
#[tauri::command]
pub async fn mixengine_job_logs_watch(
    job: i64,
    tail: u32,
    follow: bool,
    on_line: Channel<String>,
    state: State<'_, super::state::LogsState>,
) -> Result<(), AppError> {
    super::logs::stream_logs("job", job.to_string(), tail, follow, on_line, &state).await
}

/// Closes the open log stream. Calling it with nothing open is harmless.
#[tauri::command]
pub fn mixengine_logs_unwatch(state: State<'_, super::state::LogsState>) {
    state.stop();
}

/// Opens `GET /metrics`. **Opening this connection is the subscription** — the daemon samples at
/// 1 Hz while it is open, and once a minute when nobody holds it. Called from the Dashboard exactly
/// when `active` turns `true`, and closed exactly when it turns `false` — not held open for the
/// app's lifetime like `mixengine_watch`/`/events`; see `MetricsState`.
#[tauri::command]
pub async fn mixengine_metrics_watch(
    webview: tauri::WebviewWindow,
    on_frame: Channel<String>,
    state: State<'_, super::state::MetricsState>,
) -> Result<(), AppError> {
    super::metrics::stream_metrics(on_frame, webview.label(), &state).await
}

/// Closes the open `/metrics` stream. Calling it with nothing open is harmless.
#[tauri::command]
pub fn mixengine_metrics_unwatch(
    webview: tauri::WebviewWindow,
    state: State<'_, super::state::MetricsState>,
) {
    state.stop(webview.label());
}

/// `daemon.disk_usage` — `refresh: false` reads the copy the daemon holds (up to a minute old),
/// `true` walks the disk again.
#[tauri::command]
pub async fn mixengine_disk_usage(refresh: bool) -> Result<Value, AppError> {
    rpc::call("daemon.disk_usage", json!({ "refresh": refresh })).await
}

/// `daemon.cleanup` — returns a `JobSummary`; progress is followed over the shared `/events` like
/// every other job, with no infrastructure of its own.
#[tauri::command]
pub async fn mixengine_cleanup(params: Value) -> Result<Value, AppError> {
    rpc::call("daemon.cleanup", params).await
}

/// `metrics.history` — a plain read, no stream needed. The Metrics screen is built on exactly this
/// call.
#[tauri::command]
pub async fn mixengine_metrics_history(params: Value) -> Result<Value, AppError> {
    rpc::call("metrics.history", params).await
}

/// `blueprint.list` — every blueprint this home holds, in slug order.
#[tauri::command]
pub async fn mixengine_blueprints() -> Result<Value, AppError> {
    rpc::call("blueprint.list", json!({})).await
}

/// `params` has the shape of `BlueprintCapture { project: ProjectRef, name, description?,
/// overwrite }`.
#[tauri::command]
pub async fn mixengine_blueprint_capture(params: Value) -> Result<Value, AppError> {
    rpc::call("blueprint.capture", params).await
}

/// `params` has the shape of `BlueprintImport { path, signature?, name?, overwrite }`. Never
/// returns an error for a bad signature — a missing or wrong signature only changes the result's
/// `BlueprintSummary.trusted`/`signature`; it does not block the import.
#[tauri::command]
pub async fn mixengine_blueprint_import(params: Value) -> Result<Value, AppError> {
    rpc::call("blueprint.import", params).await
}

/// `params` has the shape of `BlueprintApply { blueprint, project, root, dry_run, answers?,
/// scaffold? }` — one method, called twice: `dry_run: true` returns `{ outcome: "planned", plan }`,
/// `dry_run: false` returns `{ outcome: "started", job }`.
#[tauri::command]
pub async fn mixengine_blueprint_apply(params: Value) -> Result<Value, AppError> {
    rpc::call("blueprint.apply", params).await
}

/// `job.status` — no command in this file called into the `job.*` namespace before. Needed exactly
/// once: reading `BlueprintApplied` after the job has left the list of running jobs on the stream
/// (`job_finished` removes the row and keeps no payload — see `daemonState.applyJob`).
#[tauri::command]
pub async fn mixengine_job_status(job: i64) -> Result<Value, AppError> {
    rpc::call("job.status", json!({ "job": job })).await
}

/// `extension.list` — every extension this home has installed. The Tauri command name follows the
/// `mixengine_runtime_list_installed`/`mixengine_package_list` pattern already used for the
/// installed/available pair.
#[tauri::command]
pub async fn mixengine_extension_list_installed() -> Result<Value, AppError> {
    rpc::call("extension.list", json!({})).await
}

/// `extension.available` — what the registry publishes, with `unreadable`/`stale`. **Not
/// `extension.registry_list`** — that name does not exist, even though roadmap T4.4 says so.
#[tauri::command]
pub async fn mixengine_extension_list_available() -> Result<Value, AppError> {
    rpc::call("extension.available", json!({})).await
}

/// `params` has the shape of `ExtensionPlanRequest { source: ExtensionOrigin }`. This is the only
/// step before installing — `extension.inspect` is not called (Decision D2, spec).
#[tauri::command]
pub async fn mixengine_extension_plan(params: Value) -> Result<Value, AppError> {
    rpc::call("extension.plan", params).await
}

/// `params` has the shape of `ExtensionInstall { source, consent }` — `consent` must be taken
/// verbatim from the `ExtensionPlan` just received (Decision D3, spec), not rebuilt from user
/// input.
#[tauri::command]
pub async fn mixengine_extension_install(params: Value) -> Result<Value, AppError> {
    rpc::call("extension.install", params).await
}

/// `params` has the shape of `ExtensionUninstall { id, delete_data }`.
#[tauri::command]
pub async fn mixengine_extension_uninstall(params: Value) -> Result<Value, AppError> {
    rpc::call("extension.uninstall", params).await
}

/// `id` is a bare `ExtensionId`. Called through `extension.*`, not `service.*` — two different
/// namespaces even though the id values coincide for a `service`-kind extension (spec, section
/// Extensions/Remove, Start, Stop).
#[tauri::command]
pub async fn mixengine_extension_start(id: String) -> Result<Value, AppError> {
    rpc::call("extension.start", json!({ "id": id })).await
}

#[tauri::command]
pub async fn mixengine_extension_stop(id: String) -> Result<Value, AppError> {
    rpc::call("extension.stop", json!({ "id": id })).await
}

/// `autostart.status` — reads the current mechanism/location/enabled/for_this_home; no parameters.
#[tauri::command]
pub async fn mixengine_autostart_status() -> Result<Value, AppError> {
    rpc::call("autostart.status", json!({})).await
}

#[tauri::command]
pub async fn mixengine_autostart_enable() -> Result<Value, AppError> {
    rpc::call("autostart.enable", json!({})).await
}

#[tauri::command]
pub async fn mixengine_autostart_disable() -> Result<Value, AppError> {
    rpc::call("autostart.disable", json!({})).await
}

/// `path.status` — whether `<root>/bin` is on this user's PATH yet, read from the saved PATH
/// (registry, shell profile) rather than the daemon's environment. No parameters; returns
/// `PathReport`.
#[tauri::command]
pub async fn mixengine_path_status() -> Result<Value, AppError> {
    rpc::call("path.status", json!({})).await
}

/// `path.install` — fills `<root>/bin` and puts it on this user's PATH. Needs no administrator
/// rights.
#[tauri::command]
pub async fn mixengine_path_install() -> Result<Value, AppError> {
    rpc::call("path.install", json!({})).await
}

/// `path.uninstall` — removes `<root>/bin` from PATH, leaving the files in it alone.
#[tauri::command]
pub async fn mixengine_path_uninstall() -> Result<Value, AppError> {
    rpc::call("path.uninstall", json!({})).await
}

/// `service.stop` with no `service` — every declared service, stopped by the daemon in reverse
/// dependency order (T168, the tray's *Stop all*).
///
/// A command of its own rather than `mixengine_service_action` with an empty id, for the reason
/// `mixengine_service_start_project` gives: "one service" and "all of them" are different
/// questions, and an empty id is where a typo becomes a stop-everything.
#[tauri::command]
pub async fn mixengine_service_stop_all() -> Result<Value, AppError> {
    rpc::call("service.stop", json!({ "wait": true })).await
}

/// `daemon.shutdown` — stops every service in reverse dependency order, answers with what it
/// stopped, then exits. The connection closing after the answer is the shutdown happening, not a
/// failure; the answer is read in full before that close.
#[tauri::command]
pub async fn mixengine_shutdown() -> Result<Value, AppError> {
    rpc::call("daemon.shutdown", json!({})).await
}

/// `daemon.doctor` — a pure read, no parameters, cannot raise elevation by itself.
#[tauri::command]
pub async fn mixengine_doctor() -> Result<Value, AppError> {
    rpc::call("daemon.doctor", json!({})).await
}

/// `params` has the shape of `DoctorRepair { grant }` — separate from the existing
/// `mixengine_ca_repair`: that one hard-codes `grant: true` just for the CA repair flow (Phase 2),
/// this one takes `grant` from Settings, following the two-pass T64 flow (enqueue first,
/// `elevation.grant` after the queue has been shown).
#[tauri::command]
pub async fn mixengine_doctor_repair(params: Value) -> Result<Value, AppError> {
    rpc::call("daemon.doctor_repair", params).await
}

/// `daemon.bundle` — no real parameters (`DiagnosticsBundle` is empty); gathers an archive and
/// returns its path.
#[tauri::command]
pub async fn mixengine_bundle() -> Result<Value, AppError> {
    rpc::call("daemon.bundle", json!({})).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The field name is `service`, not `id`.** `ServiceTarget` — the params shared by
    /// `service.start/stop/restart` — says plainly that absence of that field means *every declared
    /// service*. So a mistyped key does not produce an error: it produces a press-everything
    /// command, silently, on every row.
    #[test]
    fn an_action_names_the_service_it_is_about() {
        let (method, params) =
            service_action_call("mariadb@main", "start").expect("a known action");
        assert_eq!(method, "service.start");
        assert_eq!(params["service"], "mariadb@main");
        assert!(
            params.get("id").is_none(),
            "`id` is not a field of ServiceTarget; sending it leaves `service` absent"
        );
    }

    /// `wait: true`: the Dashboard's `act()` calls `reload()` as soon as the call returns, so a
    /// call that returns before the plan finishes would read `service.list` midway and overwrite
    /// the state the stream just applied with older figures. See `service_action_call`'s doc for
    /// when to switch back to `false`.
    #[test]
    fn the_call_waits_because_the_screen_reloads_after_it() {
        let (_, params) = service_action_call("caddy", "stop").expect("a known action");
        assert_eq!(params["wait"], true);
    }

    /// **The scope travels in `project`, and the name sits in `{"name": …}`** — T125. A
    /// `ProjectRef` sent in the wrong shape is a refused request; an absent `project` is far worse,
    /// because it *succeeds*: an empty target is every service this home declares, exactly the
    /// behaviour this task removes.
    #[test]
    fn a_project_start_names_the_project_it_is_about() {
        let params = service_start_project_params("shop");
        assert_eq!(params["project"]["name"], "shop");
        assert_eq!(params["wait"], true);
        assert!(
            params.get("service").is_none(),
            "a project scope and a service are two subjects; the daemon refuses both at once"
        );
    }

    #[test]
    fn each_action_maps_to_its_own_method() {
        for (action, method) in [
            ("start", "service.start"),
            ("stop", "service.stop"),
            ("restart", "service.restart"),
        ] {
            let (mapped, _) = service_action_call("caddy", action).expect("a known action");
            assert_eq!(mapped, method);
        }
    }

    /// The frontend is the only caller of this command, so an unknown `action` is a programming
    /// error — and it must come out as an error, not fall into a method guessed at random.
    #[test]
    fn an_unknown_action_is_refused_rather_than_guessed() {
        assert!(service_action_call("caddy", "pause").is_err());
    }
}
