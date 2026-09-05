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
            #[cfg(debug_assertions)]
            if argv.iter().any(|a| a == "--test-notification") {
                features::notifications::show(
                    app,
                    0,
                    "Test Notification",
                    Some("Click me to check the window comes back."),
                );
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
            commands::focus_main_window,
        ])
        .on_menu_event(|app, event| features::app_menu::handle(app, event.id.as_ref()))
        .setup(|app| {
            let handle = app.handle();

            let prefs = config::load(handle);
            app.state::<Config>().update(|p| *p = prefs.clone());

            let window = features::window::create(handle)?;

            app.set_menu(features::app_menu::build(handle)?)?;
            features::tray::create(handle)?;
            features::close_to_tray::attach(&window);

            if prefs.zoom != 1.0 {
                let _ = window.set_zoom(prefs.zoom);
            }

            // `--hidden` is what the autostart entry passes; honour the
            // preference too, so the app can start straight to the tray.
            let hidden = prefs.start_hidden
                || std::env::args().any(|a| a == "--hidden");
            if !hidden {
                window.show()?;
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
