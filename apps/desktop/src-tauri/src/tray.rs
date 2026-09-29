//! MixEngine in the tray or the menu bar — T168.
//!
//! The icon and a second webview window, label [`PANEL`], that draws the same React code as the
//! Dashboard (`src/tray.tsx`). The design, including why Linux gets a menu in front of the panel
//! rather than a popover, is `docs/specs/2026-09-19-t168-mixengine-in-the-tray-design.md`.
//!
//! **The icon is MixLab's, not a module's** — ADR 0058, `docs/specs/2026-09-29-t192-the-tray-is-mixlabs-design.md`.
//! `tray_configure` puts it up wherever the session can show one, and says whether a visible module
//! lends the panel a section. While there is an icon, closing the main window hides it instead of
//! quitting; a primary click opens the panel when there is one and the main window when there is not.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Deserialize;
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Builder, Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder, Window,
    WindowEvent,
};

use crate::error::AppError;

/// The label of the panel's window, and of its capability (`capabilities/tray.json`).
pub const PANEL: &str = "tray";

/// The label of the main window, as `tauri.conf.json` declares it.
const MAIN: &str = "main";

/// The tray icon's id — there is only ever one.
const ICON: &str = "mixengine";

/// The panel's size in logical pixels. On Linux it is a normal window and this is its minimum.
const PANEL_WIDTH: f64 = 540.0;
const PANEL_HEIGHT: f64 = 600.0;

/// The gap between the window and the edges of the usable area, in logical pixels. None: the page
/// keeps its card 10px inside the window, which is both the gap the eye sees and the room the
/// card's shadow is drawn in.
#[cfg_attr(target_os = "linux", allow(dead_code))]
const PANEL_MARGIN: f64 = 0.0;

/// The event that asks the panel's page to put itself away: slide the card out, then call
/// `tray_hide_panel`. The page hides the window, not Rust, so that the card is already back at
/// the edge when the window goes — a window hidden mid-show keeps its last frame, and the next show
/// would flash the card in place before sliding it in.
const DISMISS_EVENT: &str = "tray://dismiss";

/// How long Rust waits for the page to hide the window itself before doing it: longer than the
/// slide out, for a page that is broken or not loaded yet.
const DISMISS_FALLBACK: Duration = Duration::from_millis(1200);

/// The three words the icon's menu needs, sent by the frontend so that Rust holds no dictionary.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels {
    pub open_panel: String,
    pub open_main: String,
    pub quit: String,
}

#[derive(Default)]
pub struct TrayState {
    /// Whether the icon is up, and with it whether closing the main window hides it.
    icon: AtomicBool,
    /// Whether a visible module lends the panel a section: what a click opens, and whether the
    /// Linux menu offers the panel.
    panel: AtomicBool,
    /// Counts the panel's shows, so that a fallback hide from before a show never hides the
    /// panel it shows — see [`request_dismiss`].
    shows: AtomicU64,
    labels: Mutex<Labels>,
    /// Whether this session can show a tray icon at all, asked once — see [`has_host`].
    host: OnceLock<bool>,
    /// Started by the login entry, with the main window kept hidden until the tray is up. Cleared
    /// the moment either the icon appears or it turns out there will be none — see [`hidden_start`].
    hidden_start: AtomicBool,
}

impl TrayState {
    fn icon(&self) -> bool {
        self.icon.load(Ordering::SeqCst)
    }

    fn panel(&self) -> bool {
        self.panel.load(Ordering::SeqCst)
    }

    fn host(&self) -> bool {
        *self.host.get_or_init(has_host)
    }
}

/// Puts the tray's state in and routes window events through [`on_window_event`].
pub fn register<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder
        .manage(TrayState::default())
        .on_window_event(on_window_event)
}

/// How long a login start waits for the main window to turn the icon on before showing the window
/// instead. The frontend calls `tray_configure` within a second of loading; this is for the start
/// where it never does — a first-run screen, a broken page.
const HIDDEN_START_GRACE: Duration = Duration::from_secs(8);

/// Whether this session can show a tray icon — for Settings' note under the login switch.
pub fn has_tray_host<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.state::<TrayState>().host()
}

/// A start from the login entry: keep the main window hidden, out of the Dock on macOS, until the
/// tray is up — and show it after all if no tray comes. Nobody is ever left with a running MixLab
/// and nothing on screen.
pub fn hidden_start<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<TrayState>();
    state.hidden_start.store(true, Ordering::SeqCst);
    // Declared hidden, and on Windows nothing shows it before `launch::bring_to_front` any more.
    // Said again anyway, for the systems where the window-state plugin still maximizes it on
    // creation.
    if let Some(main) = app.get_webview_window(MAIN) {
        let _ = main.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(HIDDEN_START_GRACE);
        if app
            .state::<TrayState>()
            .hidden_start
            .swap(false, Ordering::SeqCst)
        {
            log::info!("tray: no icon came up after a login start; showing the window");
            crate::launch::bring_to_front(&app);
        }
    });
}

/// Creates the panel's window, hidden. Called from `setup`, once.
///
/// Created up front rather than on the first click: a webview takes a visible half-second to come
/// up, and a panel is something a person expects the instant they click. It is never destroyed.
pub fn create_panel<R: Runtime>(app: &AppHandle<R>) {
    let builder = WebviewWindowBuilder::new(app, PANEL, WebviewUrl::App("tray.html".into()))
        .title("MixLab")
        .visible(false)
        .inner_size(PANEL_WIDTH, PANEL_HEIGHT);

    // Transparent and without the system's shadow: the page draws a rounded card with its own
    // shadow inside a margin of this window, and slides it in from the right (`TrayPanel.module.css`).
    #[cfg(not(target_os = "linux"))]
    let builder = builder
        .transparent(true)
        .shadow(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible_on_all_workspaces(true);

    // On Linux the panel is an ordinary window the window manager places — the spec's D1.
    #[cfg(target_os = "linux")]
    let builder = builder.min_inner_size(PANEL_WIDTH, PANEL_HEIGHT);

    if let Err(e) = builder.build() {
        // No panel means no tray, never no MixLab.
        log::error!("tray: the panel window could not be created: {e}");
    }
}

/// Puts the icon up, and says whether a visible module lends the panel a section.
///
/// Called by the main window on start and whenever the modules it draws or its language change.
/// The icon no longer depends on any module (ADR 0058): whether this session can show one at all is
/// decided here, by `has_host`.
#[tauri::command]
pub fn tray_configure(
    app: AppHandle,
    state: tauri::State<'_, TrayState>,
    panel: bool,
    labels: Labels,
) -> Result<(), AppError> {
    *state.labels.lock().unwrap_or_else(|e| e.into_inner()) = labels;
    state.panel.store(panel, Ordering::SeqCst);
    if !panel {
        hide_panel(&app);
    }

    let icon = state.host();
    if icon {
        if app.tray_by_id(ICON).is_none() {
            create_icon(&app)?;
        } else {
            refresh_menu(&app, &state);
        }
    }

    state.icon.store(icon, Ordering::SeqCst);
    if state.hidden_start.swap(false, Ordering::SeqCst) && !icon {
        // Nobody may be left with a running app and no way to see it.
        crate::launch::bring_to_front(&app);
    }
    Ok(())
}

/// Shows, unminimises and focuses the main window, and puts the panel away.
#[tauri::command]
pub fn tray_open_main(app: AppHandle) {
    crate::launch::bring_to_front(&app);
    request_dismiss(&app);
}

/// Hides the panel's window, now. The page calls it once its card has slid out; nothing else
/// should, because a window hidden before that shows a stale frame on its next show.
#[tauri::command]
pub fn tray_hide_panel(app: AppHandle) {
    hide_panel(&app);
}

/// Quits MixLab. The daemon is left running: stopping it is `mixengine_shutdown`, a different
/// button. `RunEvent::Exit` in `lib.rs` still runs, so the single-instance endpoint is cleaned up.
#[tauri::command]
pub fn app_quit(app: AppHandle) {
    app.exit(0);
}

/// Puts the app back in the Dock before a window of it is shown. The other half is in
/// [`on_window_event`], where hiding the main window takes it out.
pub(crate) fn show_in_dock<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
        #[cfg(debug_assertions)]
        restore_dev_dock_icon(app);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// A development build is a bare executable, not an `.app`: its Dock icon is one Tauri sets by hand
/// when the app starts, and macOS drops it for the generic "exec" one when the app comes back into
/// the Dock. A release reads its icon from the bundle's `icon.icns` and needs none of this.
#[cfg(all(target_os = "macos", debug_assertions))]
fn restore_dev_dock_icon<R: Runtime>(app: &AppHandle<R>) {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    const ICON: &[u8] = include_bytes!("../icons/128x128@2x.png");
    let _ = app.run_on_main_thread(|| {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let data = NSData::with_bytes(ICON);
        if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
            // SAFETY: on the main thread, with an image just made from a PNG this crate carries.
            unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
        }
    });
}

/// Asks the page to slide the card out and hide the window — and hides it anyway if the page has
/// not after [`DISMISS_FALLBACK`], unless the panel was shown again in between.
fn request_dismiss<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.emit_to(PANEL, DISMISS_EVENT, ());
    let shows = app.state::<TrayState>().shows.load(Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(DISMISS_FALLBACK);
        if app.state::<TrayState>().shows.load(Ordering::SeqCst) == shows {
            hide_panel(&app);
        }
    });
}

/// Shows the panel's window and focuses it; the page slides the card in when it gets the focus.
fn show_panel<R: Runtime>(app: &AppHandle<R>, panel: &tauri::WebviewWindow<R>) {
    app.state::<TrayState>()
        .shows
        .fetch_add(1, Ordering::SeqCst);
    let _ = panel.show();
    let _ = panel.set_focus();
}

fn hide_panel<R: Runtime>(app: &AppHandle<R>) {
    if let Some(panel) = app.get_webview_window(PANEL) {
        let _ = panel.hide();
    }
}

/// Which button a click on the icon was made with, as far as routing it goes.
#[cfg_attr(target_os = "linux", allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Primary,
    Secondary,
    Other,
}

/// What a click on the icon does — the spec's D2.
#[cfg_attr(target_os = "linux", allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClickAction {
    TogglePanel,
    OpenMain,
    /// The native menu answers the secondary button on its own; nothing else may.
    Ignore,
}

/// Where a click goes. There is a panel only while a visible module lends it a section; without
/// one, the icon is the way back to the main window.
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn click_action(panel: bool, button: Button) -> ClickAction {
    match button {
        Button::Primary if panel => ClickAction::TogglePanel,
        Button::Primary => ClickAction::OpenMain,
        Button::Secondary | Button::Other => ClickAction::Ignore,
    }
}

fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    let app = window.app_handle();
    let state = app.state::<TrayState>();
    match (window.label(), event) {
        // A popover goes away when you click anywhere else. Not on Linux, where it is a window.
        #[cfg(not(target_os = "linux"))]
        (PANEL, WindowEvent::Focused(false)) => {
            if window.is_visible().unwrap_or(false) {
                request_dismiss(app);
            }
        }
        // The panel is never destroyed: Alt+F4 or its close button only puts it away.
        (PANEL, WindowEvent::CloseRequested { api, .. }) => {
            api.prevent_close();
            request_dismiss(app);
        }
        (MAIN, WindowEvent::CloseRequested { api, .. }) if state.icon() => {
            api.prevent_close();
            let _ = window.hide();
            #[cfg(target_os = "macos")]
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
        }
        // Without an icon (a session with no tray host), closing the main window is quitting —
        // and the hidden panel would
        // otherwise keep the process alive with nothing on screen.
        (MAIN, WindowEvent::Destroyed) => app.exit(0),
        _ => {}
    }
}

fn create_icon(app: &AppHandle) -> Result<(), AppError> {
    let builder = TrayIconBuilder::with_id(ICON).tooltip("MixLab");

    #[cfg(target_os = "macos")]
    let builder = builder
        .icon(tauri::include_image!("icons/tray/44x44.png"))
        .icon_as_template(true);
    #[cfg(not(target_os = "macos"))]
    let builder = match app.default_window_icon() {
        Some(icon) => builder.icon(icon.clone()),
        None => builder,
    };

    let builder = builder
        .menu(&native_menu(app, &app.state::<TrayState>())?)
        .on_menu_event(|app, event| on_menu_event(app, event.id().as_ref()));

    // macOS and Windows: the primary click is ours (`on_icon_event`), the secondary opens the menu.
    #[cfg(not(target_os = "linux"))]
    let builder = builder
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| on_icon_event(tray.app_handle(), event));

    // Linux tells an application nothing about clicks: every click opens the menu (T168 D1).
    #[cfg(target_os = "linux")]
    let builder = builder.show_menu_on_left_click(true);

    builder
        .build(app)
        .map(|_| ())
        .map_err(|e| err!("error.trayUnavailable", message = e))
}

#[cfg(not(target_os = "linux"))]
fn on_icon_event(app: &AppHandle, event: tauri::tray::TrayIconEvent) {
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
    use tauri::PhysicalPosition;

    let TrayIconEvent::Click {
        rect,
        position,
        button,
        button_state: MouseButtonState::Up,
        ..
    } = event
    else {
        return;
    };
    let button = match button {
        MouseButton::Left => Button::Primary,
        MouseButton::Right => Button::Secondary,
        MouseButton::Middle => Button::Other,
    };
    match click_action(app.state::<TrayState>().panel(), button) {
        ClickAction::Ignore => return,
        ClickAction::OpenMain => {
            crate::launch::bring_to_front(app);
            return;
        }
        ClickAction::TogglePanel => {}
    }
    let Some(panel) = app.get_webview_window(PANEL) else {
        return;
    };

    // Open, or sliding out after the click took its focus: either way the click means "away".
    if panel.is_visible().unwrap_or(false) {
        request_dismiss(app);
        return;
    }

    match monitor_under(app, position.x, position.y) {
        Some(monitor) => {
            let scale = monitor.scale_factor();
            let icon = rect.position.to_physical::<f64>(scale);
            let icon_size = rect.size.to_physical::<f64>(scale);
            let screen = Bounds {
                x: f64::from(monitor.position().x),
                y: f64::from(monitor.position().y),
                width: f64::from(monitor.size().width),
                height: f64::from(monitor.size().height),
            };
            let work = monitor.work_area();
            let work = Bounds {
                x: f64::from(work.position.x),
                y: f64::from(work.position.y),
                width: f64::from(work.size.width),
                height: f64::from(work.size.height),
            };
            let at_top = bar_at_top(icon.y + icon_size.height / 2.0, screen, work);
            let (x, y) = panel_position(
                at_top,
                (PANEL_WIDTH * scale, PANEL_HEIGHT * scale),
                work,
                PANEL_MARGIN * scale,
            );
            let _ = panel.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
        }
        None => log::warn!("tray: no monitor found for a click at {position:?}"),
    }
    show_panel(app, &panel);
}

/// Whether this session can show a tray icon. macOS and Windows always can.
#[cfg(not(target_os = "linux"))]
fn has_host() -> bool {
    true
}

/// Whether this session can show a tray icon, on Linux — the spec's D8. Two questions, both of
/// which have to be yes:
///
/// 1. **Is AppIndicator there to load?** `libappindicator-sys` loads it on first use and *panics*
///    when it cannot — on the main thread, inside `TrayIconBuilder::build` — so the same names it
///    tries are tried here first, where a no is only a no.
/// 2. **Is anybody drawing StatusNotifierItems?** GNOME without the AppIndicator extension is not,
///    and there the library falls back to a GtkStatusIcon the shell never draws: `build` succeeds
///    and the icon is invisible. Hiding the main window on close would then hide it for good.
#[cfg(target_os = "linux")]
fn has_host() -> bool {
    let library = appindicator_loads();
    let watcher = status_notifier_watcher();
    if !(library && watcher) {
        log::info!("tray: no tray on this session (AppIndicator {library}, a StatusNotifierItem host {watcher})");
    }
    library && watcher
}

/// The names `libappindicator-sys` 0.9 tries, in its order.
#[cfg(target_os = "linux")]
fn appindicator_loads() -> bool {
    const NAMES: [&str; 4] = [
        "libayatana-appindicator3.so.1",
        "libappindicator3.so.1",
        "libayatana-appindicator3.so",
        "libappindicator3.so",
    ];
    NAMES.iter().any(|name| {
        let Ok(name) = std::ffi::CString::new(*name) else {
            return false;
        };
        // SAFETY: a NUL-terminated name and flags libc defines. The handle is left open on
        // purpose: the tray is about to load the same library and would only reopen it.
        let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
        !handle.is_null()
    })
}

/// Whether `org.kde.StatusNotifierWatcher` has an owner on the session bus — the name every
/// StatusNotifierItem host (KDE, the GNOME extension, waybar, …) takes. Any failure is a no.
#[cfg(target_os = "linux")]
fn status_notifier_watcher() -> bool {
    use zbus::blocking::{connection, fdo::DBusProxy};
    use zbus::names::BusName;

    let ask = || -> zbus::Result<bool> {
        let connection = connection::Builder::session()?
            .method_timeout(Duration::from_millis(500))
            .build()?;
        let name = BusName::try_from("org.kde.StatusNotifierWatcher")
            .map_err(|e| zbus::Error::Failure(e.to_string()))?;
        Ok(DBusProxy::new(&connection)?.name_has_owner(name)?)
    };
    ask().unwrap_or(false)
}

/// The icon's menu: on macOS and Windows the secondary click's, on Linux every click's. **Open
/// control panel** is Linux's alone — elsewhere the primary click opens the panel — and only while
/// there is one.
fn native_menu(
    app: &AppHandle,
    state: &TrayState,
) -> Result<tauri::menu::Menu<tauri::Wry>, AppError> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};

    let labels = state
        .labels
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let fail = |e: tauri::Error| err!("error.trayUnavailable", message = e);
    let open_main =
        MenuItem::with_id(app, "open_main", &labels.open_main, true, None::<&str>).map_err(fail)?;
    let separator = PredefinedMenuItem::separator(app).map_err(fail)?;
    let quit = MenuItem::with_id(app, "quit", &labels.quit, true, None::<&str>).map_err(fail)?;
    if cfg!(target_os = "linux") && state.panel() {
        let open_panel =
            MenuItem::with_id(app, "open_panel", &labels.open_panel, true, None::<&str>)
                .map_err(fail)?;
        return Menu::with_items(app, &[&open_panel, &open_main, &separator, &quit]).map_err(fail);
    }
    Menu::with_items(app, &[&open_main, &separator, &quit]).map_err(fail)
}

/// The same menu, after the language or the panel changed.
fn refresh_menu(app: &AppHandle, state: &TrayState) {
    let Some(tray) = app.tray_by_id(ICON) else {
        return;
    };
    match native_menu(app, state) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(e) => log::error!("tray: the menu could not be rebuilt: {}", e.code),
    }
}

fn on_menu_event(app: &AppHandle, id: &str) {
    match id {
        "open_panel" => {
            if let Some(panel) = app.get_webview_window(PANEL) {
                let _ = panel.center();
                show_panel(app, &panel);
            }
        }
        "open_main" => {
            crate::launch::bring_to_front(app);
            request_dismiss(app);
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

/// The monitor a click on the icon happened on.
///
/// Not `monitor_from_point` alone: the point a tray event carries is physical on Windows and
/// logical on macOS, and asking with the wrong one finds nothing — which is how the panel once
/// opened wherever the window manager put it instead of beside the menu bar. So every monitor is
/// asked both ways, and the primary one answers when none contains the point.
#[cfg(not(target_os = "linux"))]
fn monitor_under(app: &AppHandle, x: f64, y: f64) -> Option<tauri::Monitor> {
    let monitors = app.available_monitors().unwrap_or_default();
    let contains = |monitor: &tauri::Monitor, x: f64, y: f64| {
        let origin = monitor.position();
        let size = monitor.size();
        x >= f64::from(origin.x)
            && x < f64::from(origin.x) + f64::from(size.width)
            && y >= f64::from(origin.y)
            && y < f64::from(origin.y) + f64::from(size.height)
    };
    monitors
        .iter()
        .find(|m| contains(m, x, y))
        .or_else(|| {
            monitors.iter().find(|m| {
                let scale = m.scale_factor();
                contains(m, x * scale, y * scale)
            })
        })
        .cloned()
        .or_else(|| app.primary_monitor().ok().flatten())
}

/// A rectangle in physical pixels. Linux is never told where its icon is, so it has no use for one.
#[cfg_attr(target_os = "linux", allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq)]
struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// Whether the bar the icon sits in runs along the top of the screen — the macOS menu bar, a
/// Windows taskbar moved up — rather than the bottom.
///
/// The icon says it best: it is in the bar. When its position makes no sense (outside the
/// screen), the usable area does: a bar at the top pushes it down from the screen's edge.
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn bar_at_top(icon_centre_y: f64, screen: Bounds, work: Bounds) -> bool {
    if icon_centre_y >= screen.y && icon_centre_y < screen.y + screen.height {
        icon_centre_y < screen.y + screen.height / 2.0
    } else {
        work.y > screen.y
    }
}

/// Where the panel's top-left corner goes: in the right-hand corner of the usable area on the
/// bar's side, `margin` in from both edges — tucked against the taskbar or under the menu bar,
/// the way the system's own tray panels sit, rather than hanging off the icon.
///
/// A taskbar on the right is outside the usable area already, so "the right-hand corner" is the
/// one beside it. One on the left leaves the panel in the corner opposite, which is still the
/// corner every other tray panel on that screen uses.
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn panel_position(at_top: bool, panel: (f64, f64), work: Bounds, margin: f64) -> (f64, f64) {
    let (width, height) = panel;
    let x = (work.x + work.width - width - margin).max(work.x);
    let y = if at_top {
        work.y + margin
    } else {
        (work.y + work.height - height - margin).max(work.y)
    };
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANEL_SIZE: (f64, f64) = (440.0, 600.0);

    fn bounds(x: f64, y: f64, width: f64, height: f64) -> Bounds {
        Bounds {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn a_menu_bar_puts_the_panel_in_the_top_right_corner() {
        // macOS: a 1440×900 screen whose work area starts under a 25px menu bar.
        let screen = bounds(0.0, 0.0, 1440.0, 900.0);
        let work = bounds(0.0, 25.0, 1440.0, 875.0);
        assert!(bar_at_top(12.0, screen, work));
        assert_eq!(
            panel_position(true, PANEL_SIZE, work, 8.0),
            (1440.0 - 440.0 - 8.0, 33.0)
        );
    }

    #[test]
    fn a_bottom_taskbar_puts_the_panel_in_the_bottom_right_corner() {
        let screen = bounds(0.0, 0.0, 1920.0, 1080.0);
        let work = bounds(0.0, 0.0, 1920.0, 1040.0);
        assert!(!bar_at_top(1060.0, screen, work));
        assert_eq!(
            panel_position(false, PANEL_SIZE, work, 8.0),
            (1920.0 - 448.0, 1040.0 - 608.0)
        );
    }

    #[test]
    fn a_top_taskbar_puts_the_panel_under_it() {
        let screen = bounds(0.0, 0.0, 1920.0, 1080.0);
        let work = bounds(0.0, 40.0, 1920.0, 1040.0);
        assert!(bar_at_top(20.0, screen, work));
        assert_eq!(panel_position(true, PANEL_SIZE, work, 8.0).1, 48.0);
    }

    #[test]
    fn a_right_taskbar_puts_the_panel_beside_it() {
        let work = bounds(0.0, 0.0, 1860.0, 1080.0);
        assert_eq!(
            panel_position(false, PANEL_SIZE, work, 8.0).0,
            1860.0 - 448.0
        );
    }

    #[test]
    fn an_icon_off_the_screen_falls_back_to_where_the_work_area_starts() {
        let screen = bounds(0.0, 0.0, 1440.0, 900.0);
        assert!(bar_at_top(-5.0, screen, bounds(0.0, 25.0, 1440.0, 875.0)));
        assert!(!bar_at_top(-5.0, screen, bounds(0.0, 0.0, 1440.0, 860.0)));
    }

    #[test]
    fn a_monitor_left_of_the_primary_one_has_negative_coordinates() {
        let work = bounds(-1920.0, 0.0, 1920.0, 1040.0);
        assert_eq!(
            panel_position(false, PANEL_SIZE, work, 8.0),
            (-448.0, 1040.0 - 608.0)
        );
    }

    #[test]
    fn a_scaled_monitor_is_measured_in_physical_pixels() {
        // 200%: the caller hands in the panel, the margin and the area in physical pixels.
        let work = bounds(0.0, 50.0, 2880.0, 1750.0);
        assert_eq!(
            panel_position(true, (880.0, 1200.0), work, 16.0),
            (2880.0 - 896.0, 66.0)
        );
    }

    #[test]
    fn a_panel_larger_than_the_work_area_starts_at_its_corner() {
        let work = bounds(0.0, 0.0, 300.0, 400.0);
        assert_eq!(panel_position(false, PANEL_SIZE, work, 8.0), (0.0, 0.0));
    }

    #[test]
    fn a_primary_click_toggles_the_panel_when_there_is_one() {
        assert_eq!(
            click_action(true, Button::Primary),
            ClickAction::TogglePanel
        );
    }

    #[test]
    fn a_primary_click_opens_the_main_window_when_no_module_lends_a_section() {
        assert_eq!(click_action(false, Button::Primary), ClickAction::OpenMain);
    }

    #[test]
    fn a_secondary_click_is_left_to_the_native_menu() {
        // The menu opens on its own (`show_menu_on_left_click(false)`); doing anything here as well
        // would open the panel behind it.
        for panel in [true, false] {
            assert_eq!(click_action(panel, Button::Secondary), ClickAction::Ignore);
            assert_eq!(click_action(panel, Button::Other), ClickAction::Ignore);
        }
    }
}
