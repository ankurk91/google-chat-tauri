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

/// How much of a page-supplied message reaches the log.
const MAX_PAGE_LOG: usize = 500;

/// A page-supplied message, reduced to something that can only occupy one line.
///
/// Truncated because it is remote-controlled text, and stripped of anything a
/// reader could break a line on because a newline here is a *forged log entry*.
/// These files are written to be attached to a public issue, so a page that can
/// send `"x\n2026-09-09 12:00:00 [INFO] reset: app data wiped"` can put a line
/// into a bug report that this app never wrote, and nothing downstream would
/// show it apart from the ones that are real.
///
/// Replaced with a space rather than dropped: the words stay apart, so a
/// forgery attempt is still legible as the one mangled line it now is.
fn one_line(message: &str) -> String {
    message
        .chars()
        .take(MAX_PAGE_LOG)
        .map(|c| {
            // C0 and C1 controls, plus the two Unicode separators that are not
            // controls but that some viewers still break a line on.
            if c.is_control() || c == '\u{2028}' || c == '\u{2029}' {
                ' '
            } else {
                c
            }
        })
        .collect()
}

#[tauri::command]
pub fn page_log(level: String, message: String) {
    let msg = one_line(&message);
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

    log::debug!("link request: {}", crate::redact::foreign_url(&parsed));

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
/// Not needed on Linux, where GTK delivers the menu accelerator itself and
/// consumes the key before the page sees it -- the earlier claim to the
/// contrary came from measuring a shortcut that was never registered, see the
/// note in `chat.js`. It is what makes the shortcuts work on Windows, where
/// WebView2 holds the key and the menu's accelerator never fires.
///
/// The allow-list matters: this command is callable by a page we do not
/// control. The line it draws is that nothing here is destructive and nothing
/// here takes more than one click to undo -- *not* that the page could do it
/// anyway, which is true of only part of the list. `back`, `forward` and `home`
/// the page can already do to itself. `zoom-*` persists through `config.json`
/// and `close-to-tray` hides the native window; a page can reach neither on its
/// own, and they are here because they are shortcuts the menu fails to deliver
/// -- zoom on Windows, Ctrl+W as cover on macOS. Both are plain to see when
/// they happen and undone from the View menu or the tray.
///
/// `quit`, `sign-out` and `reset-app` stay menu-click-only.
#[tauri::command]
pub fn menu_action(app: AppHandle, action: String) -> Result<(), String> {
    const ALLOWED: [&str; 7] = [
        "zoom-in",
        "zoom-out",
        "zoom-reset",
        "back",
        "forward",
        "home",
        "close-to-tray",
    ];

    if !ALLOWED.contains(&action.as_str()) {
        return Err(format!("action not allowed from the page: {action}"));
    }

    crate::features::app_menu::handle(&app, &action);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_cannot_forge_a_second_log_line() {
        // The reason this function exists. These files are attached to public
        // issues, and a line the app never wrote must not be able to reach one.
        let forged = one_line("harmless\n2026-09-09 12:00:00 [INFO] reset: app data wiped");

        assert!(!forged.contains('\n'));
        assert_eq!(
            forged,
            "harmless 2026-09-09 12:00:00 [INFO] reset: app data wiped"
        );
    }

    #[test]
    fn every_way_of_starting_a_line_is_closed() {
        // The last two are not control characters -- `is_control` is false for
        // both -- and are handled by name for exactly that reason.
        for c in [
            '\n', '\r', '\u{0b}', '\u{0c}', '\u{85}', '\u{2028}', '\u{2029}',
        ] {
            assert_eq!(one_line(&format!("a{c}b")), "a b", "{c:?} survived");
        }
    }

    #[test]
    fn ordinary_text_is_left_alone() {
        let line = "link intercepted: https://example.com/<path>?<2 params>";
        assert_eq!(one_line(line), line);
        // Non-ASCII is not a control character.
        assert_eq!(one_line("zoom → 1.3"), "zoom → 1.3");
    }

    #[test]
    fn the_cap_counts_characters_not_bytes() {
        // Cutting by bytes would panic on a multibyte boundary.
        let long = "→".repeat(MAX_PAGE_LOG * 2);
        assert_eq!(one_line(&long).chars().count(), MAX_PAGE_LOG);
    }
}
