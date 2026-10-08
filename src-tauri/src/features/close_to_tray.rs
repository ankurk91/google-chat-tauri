//! Ported from electron `src/main/features/closeToTray.ts`.
//!
//! Closing the window hides it instead of quitting; the app only really exits
//! via the tray's Quit item, which sets the `quitting` flag first.

use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

use crate::features::tray;
use crate::state::AppState;

/// Hide the window to the tray -- or minimise it when there is no tray to
/// bring it back from, which on Linux means no StatusNotifierWatcher on the
/// bus (see Workarounds.md). A hidden window with no tray is lost until the
/// app is launched again.
pub fn hide(app: &AppHandle, window: &WebviewWindow) {
    if app.tray_by_id(tray::ID).is_none() {
        let _ = window.minimize();
        return;
    }

    #[cfg(target_os = "macos")]
    let _ = app.hide();
    #[cfg(not(target_os = "macos"))]
    let _ = window.hide();
}

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
            log::warn!("failed to save window state on hide: {e}");
        }

        hide(app, &window);
    });
}
