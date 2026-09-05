//! Desktop notifications.
//!
//! Electron got these for free: Chromium implements the Web Notification API,
//! so `src/preload/overrideNotifications.ts` only had to wrap `window.Notification`
//! to hook the click. None of the three system webviews can do that:
//!
//! * **Linux (WebKitGTK)** -- the API exists, but permission is denied unless the
//!   embedder handles `WebKitWebView::permission-request`, and Tauri 2.11 exposes
//!   no way to do so (`WebviewBuilder::on_permission_request` is on `dev` only).
//!   Measured on this machine: `Notification.requestPermission()` -> `"denied"`.
//! * **macOS (WKWebView)** -- `window.Notification` does not exist at all.
//! * **Windows (WebView2)** -- the host must handle `NotificationReceived`, and
//!   wry does not, so notifications are silently dropped.
//!
//! So `chat.js` replaces `window.Notification` wholesale with a shim that reports
//! `"granted"` and forwards to this module. Chat only ever asks the shim, which
//! sidesteps the permission problem entirely.
//!
//! Clicking is where the platforms diverge. On Linux we get a real activation
//! callback and emit [`ACTIVATED_EVENT`]; `chat.js` then dispatches a synthetic
//! `click` on the original `Notification` object, so **Google's own handler runs
//! and opens the right conversation** -- better than the electron original, which
//! only raised the window. macOS and Windows have no equivalent hook here.

use tauri::{AppHandle, Emitter};

/// Payload is the notification id assigned by `chat.js`.
pub const ACTIVATED_EVENT: &str = "notification-activated";

pub fn show(app: &AppHandle, id: u32, title: &str, body: Option<&str>) {
    #[cfg(target_os = "linux")]
    show_linux(app, id, title, body);

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    show_via_plugin(app, title, body);

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = (app, id, title, body);
    }
}

/// Tell the page a notification was clicked, and raise the window.
fn activated(app: &AppHandle, id: u32) {
    log::debug!("notification activated: id={id}");
    if let Err(e) = app.emit(ACTIVATED_EVENT, id) {
        log::error!("notification: failed to emit activation: {e}");
    }
    crate::features::window::show_and_focus(app);
}

#[cfg(target_os = "linux")]
fn show_linux(app: &AppHandle, id: u32, title: &str, body: Option<&str>) {
    let mut builder = notify_rust::Notification::new();
    builder
        .summary(title)
        .appname("Google Chat")
        // Matches the `Icon=` key in the installed .desktop entry. Falls back to
        // the daemon's default when running unpackaged.
        .icon("google-chat-tauri")
        // Chat notifications are transient; let the daemon time them out.
        .hint(notify_rust::Hint::Category("im.received".into()));

    if let Some(body) = body {
        builder.body(body);
    }

    if !actions_enabled() {
        if let Err(e) = builder.show() {
            log::error!("notification: failed to show notification: {e}");
        }
        return;
    }

    builder.action("default", "Open");

    let handle = match builder.show() {
        Ok(h) => h,
        Err(e) => {
            log::error!("notification: failed to show notification: {e}");
            return;
        }
    };

    // wait_for_action blocks until the notification is acted on, so it cannot
    // run on the main thread.
    let app = app.clone();
    std::thread::spawn(move || {
        handle.wait_for_action(|action| {
            // "__closed" means dismissed, which we ignore.
            if action == "default" {
                activated(&app, id);
            }
        });
    });
}

/// Whether to register a clickable "default" action on Linux notifications.
///
/// On by default. The escape hatch exists because a notification service that
/// reports "activated" when a notification merely expires would raise the
/// window a few seconds after every message -- far more irritating than not
/// having click-through at all. Cinnamon, GNOME and KDE all behave correctly;
/// if some desktop does not, set:
///
/// ```text
/// GOOGLE_CHAT_NOTIFICATION_ACTIONS=0
/// ```
///
/// `scripts/notification-test.py` checks this by watching the pointer while it
/// waits, so it can tell a real click from a self-activation without relying on
/// anyone sitting still.
#[cfg(target_os = "linux")]
fn actions_enabled() -> bool {
    !matches!(
        std::env::var("GOOGLE_CHAT_NOTIFICATION_ACTIONS").as_deref(),
        Ok("0") | Ok("false")
    )
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn show_via_plugin(app: &AppHandle, title: &str, body: Option<&str>) {
    use tauri_plugin_notification::NotificationExt;

    let mut builder = app.notification().builder().title(title);
    if let Some(body) = body {
        builder = builder.body(body);
    }

    if let Err(e) = builder.show() {
        log::error!("notification: failed to show notification: {e}");
    }
}
