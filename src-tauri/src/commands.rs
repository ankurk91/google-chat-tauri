//! Every command reachable from the embedded Google Chat page.
//!
//! This is attack surface: the page is remote and we do not control it. Keep
//! the list small, validate inputs here, and never expose anything that
//! navigates, quits, reads files or touches preferences. Menu-driven actions
//! stay entirely in Rust.
//!
//! Each command must also be listed in `permissions/chat-ipc.toml`, or the ACL
//! rejects it when it arrives from a remote origin.

use tauri::{AppHandle, Manager};

use crate::features::window::MAIN;
use crate::state::AppState;

#[tauri::command]
pub fn page_log(level: String, message: String) {
    // Truncate: this is remote-controlled text.
    let msg: String = message.chars().take(500).collect();
    // The page is remote; log at its requested level but never above info.
    // `debug` is honoured too, so the page can leave diagnostics that a release
    // build -- which logs at info -- drops on the floor.
    match level.as_str() {
        "error" => log::error!("page: {msg}"),
        "warn" => log::warn!("page: {msg}"),
        "debug" => log::debug!("page: {msg}"),
        _ => log::info!("page: {msg}"),
    }
}

#[tauri::command]
pub fn set_unread_count(app: AppHandle, count: i64, has_unread: bool) {
    let count = count.clamp(0, 9999);
    log::debug!("unread: count={count} has_unread={has_unread}");
    if app.state::<AppState>().set_unread(count, has_unread) {
        crate::features::badge::apply(&app);
    }
}

#[tauri::command]
pub fn open_external_url(app: AppHandle, url: String) -> Result<(), String> {
    let parsed = url::Url::parse(&url).map_err(|e| e.to_string())?;

    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("refusing non-http(s) scheme: {}", parsed.scheme()));
    }

    let Some(window) = app.get_webview_window(MAIN) else {
        return Err("main window is gone".into());
    };

    log::debug!("link request: {parsed}");

    // Preferences > Open Every Link in This Window suspends the allow-list for
    // five minutes, so an external identity provider can finish a sign-in here
    // rather than in the system browser.
    let in_window = app.state::<AppState>().links_open_in_app();

    if crate::urls::should_open_externally(&parsed) && !in_window {
        crate::features::external_links::open_in_browser(&app, parsed.as_str());
    } else {
        // Electron's `action: 'allow'` spawned a popup window. A second window
        // is not useful for Chat, so navigate the main webview instead.
        log::debug!(
            "navigating main window{}",
            if in_window { " (grant in force)" } else { "" }
        );
        let _ = window.navigate(parsed);
    }

    Ok(())
}

/// Backs the `window.Notification` shim in `chat.js`; see `features::notifications`.
#[tauri::command]
pub fn show_notification(app: AppHandle, id: u32, title: String, body: Option<String>) {
    // Remote-controlled text: clamp it before handing it to the OS.
    let title: String = title.chars().take(200).collect();
    let body = body.map(|b| b.chars().take(500).collect::<String>());

    crate::features::notifications::show(&app, id, &title, body.as_deref());
}

/// Keyboard shortcuts, forwarded from `chat.js`.
///
/// GTK menu accelerators do not reach the app while focus is inside the
/// WebKitGTK webview -- measured: zero menu events for Ctrl+Plus and friends --
/// so the page has to forward them. Menu *clicks* still work normally.
///
/// The allow-list matters: this command is callable by a page we do not
/// control, so it deliberately excludes anything destructive. "quit" and
/// "sign-out" stay menu-click-only; everything here is something the page could
/// already do to itself.
#[tauri::command]
pub fn menu_action(app: AppHandle, action: String) -> Result<(), String> {
    const ALLOWED: [&str; 7] = [
        "zoom-in",
        "zoom-out",
        "zoom-reset",
        "back",
        "forward",
        // Navigating to the app root is something the page could already do to
        // itself, which is the line this list draws.
        "home",
        "close-to-tray",
    ];

    if !ALLOWED.contains(&action.as_str()) {
        return Err(format!("action not allowed from the page: {action}"));
    }

    crate::features::app_menu::handle(&app, &action);
    Ok(())
}
