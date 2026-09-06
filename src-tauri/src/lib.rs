mod commands;
mod config;
mod features;
mod icons;
mod inject;
mod state;
mod urls;

use tauri::Manager;

use config::Config;
use state::AppState;

pub fn run() {
    let context = tauri::generate_context!();

    // Before the builder: a pending reset deletes files that the log and window
    // state plugins open during their setup. See features::reset.
    features::reset::take_pending(&context.config().identifier);

    let mut builder = tauri::Builder::default();

    // Must be registered first so a second launch is short-circuited before it
    // does any work. Ported from electron src/main/features/singleInstance.ts.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            // Start into the tray, not into a window thrown at the user
            // mid-login. `--hidden` is honoured in setup below.
            Some(vec!["--hidden"]),
        ));

        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // A second launch normally just means "show me the window".
            //
            // In a debug build it doubles as a remote control, because the
            // plugin hands us the new process's argv: `google-chat-tauri
            // --test-notification` fires one without needing the tray menu,
            // which makes the notification path scriptable.
            // Same trick for the reset path, which otherwise needs a human to
            // answer a modal: `google-chat-tauri --test-reset`.
            #[cfg(debug_assertions)]
            if argv.iter().any(|a| a == "--test-reset") {
                features::reset::perform(app);
                return;
            }

            #[cfg(debug_assertions)]
            if argv.iter().any(|a| a == "--test-notification") {
                features::notifications::show_test_from_page(app);
                return;
            }

            // The update check normally waits half a minute and then twelve
            // hours; this runs one immediately, dialog and all.
            #[cfg(debug_assertions)]
            if argv.iter().any(|a| a == "--test-update-check") {
                features::updates::check_now(app);
                return;
            }

            // Clicking the popup itself cannot be scripted, so this stands in
            // for it: `google-chat-tauri --test-activation`.
            #[cfg(debug_assertions)]
            if argv.iter().any(|a| a == "--test-activation") {
                features::notifications::activate_last(app);
                return;
            }
            let _ = &argv;

            features::window::show_and_focus(app);
        }));
    }

    // Only macOS/Windows use the plugin; Linux talks to notify-rust directly so
    // it can observe clicks. See features::notifications.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        builder = builder.plugin(tauri_plugin_notification::init());
    }

    builder
        .plugin(
            // Without this, every diagnostic in the app goes to stderr, which
            // is nowhere at all once the app is launched from a desktop menu.
            tauri_plugin_log::Builder::new()
                // `targets`, not two `target` calls: the builder starts with
                // these same two, and adding them again writes every line to
                // each of them twice.
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: None,
                    }),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                ])
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Debug
                } else {
                    log::LevelFilter::Info
                })
                // These are chatty and say nothing we need.
                .level_for("tao", log::LevelFilter::Warn)
                .level_for("wry", log::LevelFilter::Warn)
                // Local time, not UTC: the first thing anyone does with a log
                // is line it up against when they saw the problem.
                .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                .max_file_size(2 * 1024 * 1024)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                // VISIBLE is deliberately excluded: the window starts hidden by
                // design and closes to tray, so persisting visibility would make
                // it start hidden forever.
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED
                        | tauri_plugin_window_state::StateFlags::FULLSCREEN,
                )
                .build(),
        )
        .manage(AppState::default())
        .manage(Config::default())
        .invoke_handler(tauri::generate_handler![
            commands::page_log,
            commands::set_unread_count,
            commands::open_external_url,
            commands::show_notification,
            commands::menu_action,
        ])
        .on_menu_event(|app, event| features::app_menu::handle(app, event.id.as_ref()))
        .setup(|app| {
            let handle = app.handle();

            let prefs = config::load(handle);
            app.state::<Config>().update(|p| *p = prefs.clone());

            // `--hidden` is what the autostart entry passes; honour the
            // preference too, so the app can start straight to the tray.
            let hidden = prefs.start_hidden || std::env::args().any(|a| a == "--hidden");

            // Before anything that can fail, so a log from a launch that died
            // still says which machine and build it died on.
            features::diagnostics::log_startup(handle, &prefs, hidden);

            let window = features::window::create(handle)?;

            app.set_menu(features::app_menu::build(handle)?)?;
            features::tray::create(handle)?;
            features::close_to_tray::attach(&window);

            if prefs.zoom != 1.0 {
                let _ = window.set_zoom(prefs.zoom);
            }

            if !hidden {
                window.show()?;
            }

            // Both run on their own threads and return immediately: a probe
            // that waits out a slow wifi association, and a check that sleeps
            // between the twice-daily ones.
            features::connectivity::check_at_startup(handle);
            features::updates::start(handle);

            Ok(())
        })
        .run(context)
        .expect("error while running tauri application");
}
