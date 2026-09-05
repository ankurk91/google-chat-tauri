//! The unread message counter -- the app's headline feature.
//!
//! Derived from electron `src/main/features/badgeIcon.ts`, with two changes.
//!
//! **Source of truth.** Electron drove the tray icon from Google's favicon URL,
//! matching on names like `favicon_chat_new_notif_r2`. That mechanism is dead:
//! today's `chat.google.com` ships no `<link rel="icon">` at all, so the lookup
//! returns an empty string forever. The unread count we already scrape is a
//! more direct signal for the same question, so the tray follows it instead.
//!
//! **Platform spread.** No single API covers all three desktops:
//!
//! * **macOS** -- `Window::set_badge_count` is a real dock badge. Works.
//! * **Windows** -- `set_badge_count` is unsupported; the equivalent is a
//!   taskbar *overlay icon*, so we swap in a pre-rendered numbered disc.
//! * **Linux** -- `set_badge_count` routes through tao, which `dlopen`s
//!   `libunity.so`. That is absent on most modern desktops (Cinnamon, XFCE,
//!   MATE, Ubuntu 24.04 generally), where it silently does nothing. The real
//!   indicators on Linux are the **numbered tray icon** and the **window title
//!   suffix**; `set_badge_count` is still called best-effort for the KDE /
//!   Unity / GNOME+dash-to-dock users where it does work.

use tauri::{AppHandle, Manager};

use crate::features::window::MAIN;
use crate::icons;
use crate::state::AppState;

/// Push the current unread count out to every indicator.
pub fn apply(app: &AppHandle) {
    let state = app.state::<AppState>();
    let count = state.unread();
    let connected = state.is_connected();

    // The tray follows the favicon, which keeps working while the window is
    // hidden; the count comes from the DOM, which does not.
    update_tray(app, connected, state.has_unread());

    let Some(window) = app.get_webview_window(MAIN) else {
        return;
    };

    // Shows in Alt-Tab and every desktop's window list. On Linux this is the
    // indicator most users actually notice.
    let title = if count > 0 {
        format!("Google Chat ({count})")
    } else {
        "Google Chat".to_string()
    };
    let _ = window.set_title(&title);

    // macOS dock badge; best-effort elsewhere (see the module note).
    let _ = window.set_badge_count(if count > 0 { Some(count) } else { None });

    #[cfg(target_os = "windows")]
    {
        let overlay = if count > 0 {
            icons::decode(icons::count_16(count)).ok()
        } else {
            None
        };
        let _ = window.set_overlay_icon(overlay);
    }
}

fn update_tray(app: &AppHandle, connected: bool, has_unread: bool) {
    let Some(tray) = app.tray_by_id(crate::features::tray::ID) else {
        return;
    };

    if let Ok(image) = icons::decode(icons::tray(connected, has_unread)) {
        let _ = tray.set_icon(Some(image));
    }
}
