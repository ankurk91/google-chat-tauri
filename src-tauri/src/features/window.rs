//! Ported from electron `src/main/windowWrapper.ts`.
//!
//! The window is built here rather than declared in `tauri.conf.json` because
//! `initialization_script`, `on_navigation`, `on_page_load` and a computed
//! `user_agent` have no JSON equivalent. `app.windows` is therefore `[]`.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const MAIN: &str = "main";

pub fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let url: url::Url = crate::urls::APP_URL
        .parse()
        .expect("APP_URL is a compile-time constant and must parse");

    WebviewWindowBuilder::new(app, MAIN, WebviewUrl::External(url))
        .title("Google Chat")
        .inner_size(800.0, 600.0)
        .min_inner_size(480.0, 570.0)
        .center()
        // Shown by the caller once setup is done, mirroring electron's
        // `show: false` + `ready-to-show`.
        .visible(false)
        // Painted before the page renders. Electron used #E8EAED, but Chat
        // follows the system theme and a light flash on a dark desktop is
        // jarring, so use Google's dark surface colour instead.
        .background_color(tauri::window::Color(0x20, 0x21, 0x24, 0xFF))
        // Zoom is handled in chat.js instead, so the level can be persisted;
        // wry's built-in hotkeys would bypass that.
        .zoom_hotkeys_enabled(false)
        .user_agent(&crate::features::user_agent::spoofed())
        .initialization_script(crate::inject::SCRIPT)
        .on_navigation(crate::features::external_links::navigation_guard)
        .on_download(crate::features::downloads::handle)
        .on_page_load(|webview, payload| {
            // Belt and braces. The initialization script is the real mechanism
            // -- it does run at document-start on remote URLs on all three
            // desktop webviews -- but re-evaluating on load costs nothing and
            // chat.js guards against running twice.
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
                let _ = webview.eval(crate::inject::SCRIPT);
            }
        })
        .build()
}

/// Bring the main window back from the tray / another workspace.
/// Ported from electron `src/main/features/singleInstance.ts` + `handleNotification.ts`.
pub fn show_and_focus(app: &AppHandle) {
    let Some(win) = app.get_webview_window(MAIN) else {
        return;
    };

    #[cfg(target_os = "macos")]
    let _ = app.show();

    if win.is_minimized().unwrap_or(false) {
        let _ = win.unminimize();
    }
    let _ = win.show();
    let _ = win.set_focus();
}
