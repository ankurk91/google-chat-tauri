//! Ported from electron `src/main/features/closeToTray.ts`.
//!
//! Closing the window hides it instead of quitting; the app only really exits
//! via the tray's Quit item, which sets the `quitting` flag first.

use tauri::{Manager, WebviewWindow, WindowEvent};

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

        #[cfg(target_os = "macos")]
        let _ = app.hide();
        #[cfg(not(target_os = "macos"))]
        let _ = window.hide();
    });
}
