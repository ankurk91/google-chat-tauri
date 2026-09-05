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
    eprintln!("[chat.js/{level}] {msg}");
}

#[tauri::command]
pub fn set_unread_count(app: AppHandle, count: i64) {
    let count = count.clamp(0, 9999);
    if app.state::<AppState>().set_unread(count) {
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

    let current_host = window
        .url()
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default();

    eprintln!("[popup] request: {parsed} (current host: {current_host})");

    if crate::urls::should_open_externally(&parsed, &current_host) {
        crate::features::external_links::open_in_browser(&app, parsed.as_str());
    } else {
        // Electron's `action: 'allow'` spawned a popup window. A second window
        // is not useful for Chat, so navigate the main webview instead.
        eprintln!("[popup] -> navigating main window");
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

#[tauri::command]
pub fn focus_main_window(app: AppHandle) {
    crate::features::window::show_and_focus(&app);
}
