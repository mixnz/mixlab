// First, and with `macro_use`: the `err!` macro it defines is used by every module below it.
#[macro_use]
mod error;
mod downloads;

mod import;
mod instance;
mod launch;
mod login_item;
mod modules;
mod platform;
mod relaunch;
mod secrets;
mod ssh;
/// **Public because `tests/sync_live.rs` drives it** against a real server — the one place the
/// client meets a server it did not write. Everything else here is private because `lib.rs` wires
/// it; this is wired too (`sync::commands`), and public on top of that.
pub mod sync;
mod tray;
mod uninstall;
mod updater;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Before anything else, while this is still one thread with no children: the URL this was
    // started with, and the credential for it out of the environment. Everything the builder
    // starts — threads, webview helpers, later a shell in a terminal tab — inherits what is left.
    let opening = launch::Opening::from_process();
    // And, in the same breath and for the same reason, whether this copy was started by the copy it
    // is replacing — T106. Read and removed here: left in the environment it would be inherited by
    // every child this process ever starts, the next relaunch's included. `remember` goes with it,
    // because after an update `/proc/self/exe` names the file that was renamed out of the way rather
    // than the one that replaced it — so the only safe moment to ask is before an update can have
    // happened.
    let taking_over = relaunch::taking_over();
    relaunch::remember();
    let context = tauri::generate_context!();

    if taking_over {
        // The predecessor is still winding down, and handing it this start would be handing a start
        // to a window that is closing. Wait for its endpoint instead, then carry on as the only copy
        // — `relaunch::wait_for_predecessor` says what happens when it does not let go.
        relaunch::wait_for_predecessor(&context.config().identifier);
    } else if launch::forward(&context.config().identifier, &opening) {
        // A copy already running takes it and opens the tab. This process is then done, and exiting
        // 0 is what tells the program that started it that the connection was handed on.
        return;
    }

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            // The default targets are stdout and the log directory already. Adding `LogDir` again
            // gave the file two writers, and every line landed in it twice.
            tauri_plugin_log::Builder::new()
                // The plugin's default is 40_000 bytes — too small to hold a session with a real
                // bug in it. 5MB holds a lot of lines before it ever needs to rotate.
                .max_file_size(5_000_000)
                // `info` and above, for every crate. The plugin's default passes TRACE and DEBUG
                // too, and dependencies log every call there: `sqlx` each statement, `keyring` each
                // credential, `h2`, `hyper_util`, `reqwest` and `tracing` each frame of a request.
                // Measured, they were thousands of lines a minute, and rotated a real error out of
                // the file within minutes. MixLab's own lines are `info`, `warn` and `error`.
                .level(tauri_plugin_log::log::LevelFilter::Info)
                .build(),
        );

    // **No updater plugin and no process plugin** — T106. MixEngine's updater is the only one:
    // `update.status | check | decide | apply` on the daemon, one signed feed, one key, one payload.
    // What the process plugin was here for — restarting this window — is `crate::relaunch`, which has
    // to know rather more than that plugin did about which executable is the right one to start.
    #[cfg(desktop)]
    let builder = builder
        // Only the maximized flag is persisted: leave the window maximized and it comes back
        // maximized, restore it down and the next launch uses the default size from the config.
        .plugin({
            let window_state = tauri_plugin_window_state::Builder::default()
                .with_state_flags(tauri_plugin_window_state::StateFlags::MAXIMIZED)
                // The tray panel is placed beside the icon every time it opens (T168).
                .with_denylist(&[tray::PANEL]);
            // On Windows `main` is maximized as it is first shown instead — see
            // `launch::bring_to_front` for why.
            #[cfg(windows)]
            let window_state = window_state.skip_initial_state("main");
            window_state.build()
        })
        // Registers `mixlab://` with the OS through the installers, and on macOS delivers the URLs
        // the OS opens the app with — see `launch::start` for which systems listen to it.
        .plugin(tauri_plugin_deep_link::init());

    // MixLab at login, starting with `--hidden` — ADR 0042. Not in a development build: the entry
    // is named after the product, so a debug build would overwrite the release's, and it would
    // name an executable under `target/` that the next `cargo clean` removes.
    //
    // `macos_launcher` exists only in the plugin's macOS build, so it is asked for there alone. A
    // debug build never compiles this block, which is how the call without a `cfg` went unnoticed
    // until the release legs of `build` failed on the other four systems.
    #[cfg(all(desktop, not(debug_assertions)))]
    let builder = builder.plugin({
        let autostart = tauri_plugin_autostart::Builder::new();
        #[cfg(target_os = "macos")]
        let autostart =
            autostart.macos_launcher(tauri_plugin_autostart::MacosLauncher::LaunchAgent);
        autostart.arg(launch::HIDDEN).build()
    });

    // Each module puts its own state in; the list of commands they add up to is
    // `modules::handler`.
    let builder = launch::register(builder);
    let builder = modules::db::register(builder);
    let builder = modules::mixengine::register(builder);
    let builder = modules::rest::register(builder);
    let builder = modules::terminal::register(builder);
    let builder = tray::register(builder);

    let hidden = opening.hidden;
    builder
        .setup(move |app| {
            /* Before anything else: a standalone-client user's stores, copied while nothing else can touch
            the directory. `setup` runs inside `build()`, before the event loop that would
            deliver the webview's first `Store.load` — which is the only moment in which that
            copy is race-free. The credentials follow on a thread of their own; the module's
            own documentation is where both halves are argued. */
            import::on_first_launch(app.handle());

            // Sync's state, with where its record store lives. A machine with no data directory
            // still starts: the error waits for the first sign-in, which is where it means something.
            {
                use tauri::Manager as _;
                app.manage(sync::session::SyncState::new(
                    std::sync::Arc::new(sync::saved::CredentialStore),
                    platform::app_data_dir(app.handle()).map(|dir| dir.join("sync.db")),
                ));
            }

            // An update that stopped MixEngine and was interrupted is finished here, and `.old`
            // files an earlier update left are removed (T187, spec D8). Then every download but
            // the release on offer goes (T188, spec D2).
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    updater::install::recover(&handle).await;
                    if let Ok(updates) = updater::install::updates_dir(&handle) {
                        let offered = updater::feed::Cache::new(updates.clone())
                            .load(updater::PUBLIC_KEY)
                            .map(|feed| feed.version);
                        updater::ready::discard_stale(&updates, offered.as_deref());
                    }
                });
            }

            {
                use tauri::Manager as _;
                app.manage(updater::commands::UpdaterState::default());
            }

            // T182a: MixLab ▸ Remove MixLab from this Mac…, on a `.pkg` copy of a release. Its
            // state is managed everywhere, so the label command has something to answer.
            {
                use tauri::Manager as _;
                app.manage(uninstall::menu::UninstallMenu::default());
                if let Err(error) = uninstall::menu::install(app.handle()) {
                    log::warn!("the Remove MixLab menu item could not be added: {error}");
                }
            }

            launch::start(app.handle(), opening);

            // Hidden until its icon is clicked; the icon itself waits for `tray_configure`.
            tray::create_panel(app.handle());

            // `main` is declared hidden so that a login start never flashes it; every other start
            // shows it here.
            if hidden {
                tray::hidden_start(app.handle());
            } else {
                launch::bring_to_front(app.handle());
            }

            /* Housekeeping rather than startup work. A tool download that the app never came back
            from — a crash, a power cut, a force quit — leaves an unpacked server distribution
            in the tools directory, and this is the only thing that ever collects it. On a
            thread of its own and with its answer ignored: the window must not wait behind a
            directory walk and a delete of several hundred megabytes. */
            let handle = app.handle().clone();
            std::thread::spawn(move || modules::db::sweep_downloads(&handle));
            Ok(())
        })
        .invoke_handler(modules::handler())
        .build(context)
        .expect("error while building tauri application")
        .run(|app, event| match event {
            tauri::RunEvent::Exit => launch::stop(app),
            /* macOS never starts a second MixLab: opening it from Finder, Launchpad or Spotlight
            while it runs reopens this process instead, so a start after closing to the tray lands
            here rather than on the single-instance endpoint. `has_visible_windows` is not asked:
            the tray's panel is a window too. */
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => launch::bring_to_front(app),
            _ => {}
        });
}
