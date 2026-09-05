//! Launch at login. Ported from electron `src/main/features/openAtLogin.ts`.
//!
//! The entry is registered with `--hidden`, so starting with the session lands
//! the app in the tray rather than throwing a window at the user mid-login;
//! `lib.rs` honours that flag at startup.
//!
//! Registration is skipped in debug builds. The plugin would otherwise point the
//! autostart entry at `target/debug/`, which follows you around after the
//! checkout moves and launches a development build at every login.

use tauri::{AppHandle, Runtime};
use tauri_plugin_autostart::ManagerExt;

/// Reflects the OS, not our preferences file: the user may have removed the
/// entry through their desktop's startup-applications tool.
pub fn is_enabled<R: Runtime>(app: &AppHandle<R>) -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    app.autolaunch().is_enabled().unwrap_or(false)
}

pub fn set<R: Runtime>(app: &AppHandle<R>, enabled: bool) {
    if cfg!(debug_assertions) {
        log::warn!("autostart: ignoring set({enabled}) in a debug build");
        return;
    }

    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };

    if let Err(e) = result {
        log::warn!("autostart: failed to set to {enabled}: {e}");
    }
}
