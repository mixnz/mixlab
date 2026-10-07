//! **MixLab ▸ Remove MixLab from this Mac…** — roadmap task **T182a**, spec D1.
//!
//! Tauri gives macOS a default application menu and MixLab has never replaced it. That menu is
//! taken as it is, which keeps the Edit submenu that makes copy and paste work in a webview, and
//! one item goes into the application submenu before *Quit*.

use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Emitter, Manager, Runtime, Wry};

/// The item's id, and the event the shell listens for when it is clicked.
pub const ITEM: &str = "uninstall";
pub const REQUEST_EVENT: &str = "uninstall://request";

/// The item, once built, so the frontend can give it its translated text.
#[derive(Default)]
pub struct UninstallMenu(Mutex<Option<MenuItem<Wry>>>);

impl UninstallMenu {
    pub fn set_text(&self, label: &str) -> tauri::Result<()> {
        match self.0.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            Some(item) => item.set_text(label),
            None => Ok(()),
        }
    }
}

/// Puts the item in the application menu, where the removal is offered (`commands::available`).
pub fn install(app: &AppHandle<Wry>) -> tauri::Result<()> {
    if !super::commands::available() {
        return Ok(());
    }

    let menu = Menu::default(app)?;
    let Some(application) = menu
        .items()?
        .into_iter()
        .next()
        .and_then(|first| first.as_submenu().cloned())
    else {
        return Ok(());
    };

    // About, Services, Hide… then a separator and Quit: the item goes before the last separator's
    // Quit, with a separator of its own, so it is not one slip away from Quit.
    let item = MenuItem::with_id(
        app,
        ITEM,
        "Remove MixLab from this Mac…",
        true,
        None::<&str>,
    )?;
    let quit = application.items()?.len().saturating_sub(1);
    application.insert(&item, quit)?;
    application.insert(&PredefinedMenuItem::separator(app)?, quit + 1)?;

    app.set_menu(menu)?;
    *app.state::<UninstallMenu>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(item);
    app.on_menu_event(|app, event| on_event(app, event.id().as_ref()));

    Ok(())
}

fn on_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    if id == ITEM {
        crate::launch::bring_to_front(app);
        let _ = app.emit(REQUEST_EVENT, ());
    }
}
