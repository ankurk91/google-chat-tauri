//! Ported from electron `src/main/features/closeToTray.ts`.
//!
//! Closing the window hides it instead of quitting; the app only really exits
//! via the tray's Quit item, which sets the `quitting` flag first.

use tauri::{Manager, WebviewWindow, WindowEvent};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

use crate::state::AppState;

pub fn attach(window: &WebviewWindow) {
    let window = window.clone();

    window.clone().on_window_event(move |event| {
        let WindowEvent::CloseRequested { api, .. } = event else {
            return;
        };

        let app = window.app_handle();
        if app.state::<AppState>().is_quitting() {
            return;
        }

        api.prevent_close();

        // Persist geometry now rather than relying on the plugin's save-on-exit.
        // Hiding to the tray is where most sessions effectively end -- the
        // process can then live for days and be killed by a reboot or a logout,
        // which never reaches `RunEvent::Exit`, silently losing the window size
        // and position the user chose.
        if let Err(e) = app.save_window_state(StateFlags::all()) {
            eprintln!("[window-state] failed to save on hide: {e}");
        }

        #[cfg(target_os = "macos")]
        let _ = app.hide();
        #[cfg(not(target_os = "macos"))]
        let _ = window.hide();
    });
}
