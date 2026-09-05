mod commands;
mod features;
mod icons;
mod inject;
mod state;
mod urls;

use state::AppState;

pub fn run() {
    let mut builder = tauri::Builder::default();

    // Must be registered first so a second launch is short-circuited before it
    // does any work. Ported from electron src/main/features/singleInstance.ts.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            features::window::show_and_focus(app);
        }));
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
        .invoke_handler(tauri::generate_handler![
            commands::page_log,
            commands::set_unread_count,
            commands::open_external_url,
            commands::focus_main_window,
        ])
        .setup(|app| {
            let handle = app.handle();
            let window = features::window::create(handle)?;

            features::tray::create(handle)?;
            features::close_to_tray::attach(&window);

            window.show()?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
