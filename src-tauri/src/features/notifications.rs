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
//! `click` on the original `Notification` object, so Google's own handler runs
//! and opens the conversation -- more than the electron original did, which only
//! raised the window. macOS and Windows have no equivalent hook here.

use tauri::{AppHandle, Emitter};

/// Payload is the notification id assigned by `chat.js`.
pub const ACTIVATED_EVENT: &str = "notification-activated";

/// The last id the page asked us to show, so `--test-activation` can pretend
/// that one was clicked. Debug builds only -- nothing in a release build may be
/// able to fake an activation.
#[cfg(debug_assertions)]
static LAST_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn show(app: &AppHandle, id: u32, title: &str, body: Option<&str>) {
    #[cfg(debug_assertions)]
    LAST_ID.store(id, std::sync::atomic::Ordering::Relaxed);

    #[cfg(target_os = "linux")]
    show_linux(app, id, title, body);

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    show_via_plugin(app, title, body);

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = (app, id, title, body);
    }
}

/// Raise the window, then tell the page a notification was clicked.
///
/// That order matters, and the pause with it. Chat's own click handler asks its
/// router to open the conversation, and the router does nothing while the page
/// is hidden -- the same thing that makes the unread count read zero from a
/// hidden window. Showing first, and letting the page paint before the click
/// arrives, is what gets the conversation opened rather than just the window.
///
/// This already runs on the notification's own thread, so sleeping here holds
/// nothing else up.
fn activated(app: &AppHandle, id: u32) {
    log::debug!("notification activated: id={id}");

    crate::features::window::show_and_focus(app);
    std::thread::sleep(std::time::Duration::from_millis(300));

    if let Err(e) = app.emit(ACTIVATED_EVENT, id) {
        log::error!("notification: failed to emit activation: {e}");
    }
}

/// Debug only: fire a notification the way Chat does, from inside the page.
///
/// Going through `window.Notification` puts a real object in the shim's map, so
/// clicking the popup exercises the whole path -- activation event, dispatch
/// back onto that object, and the link fallback when nothing handles the click
/// -- rather than only proving that a popup appears. The `data` here is shaped
/// like the payload a click is expected to navigate from. The tray's Test
/// Notification item still goes straight to the daemon, which is the other half
/// worth being able to test on its own.
#[cfg(debug_assertions)]
pub fn show_test_from_page(app: &AppHandle) {
    use tauri::Manager;

    let Some(window) = app.get_webview_window(crate::features::window::MAIN) else {
        log::warn!("notification: no main window to fire a test notification from");
        return;
    };

    let script = r#"
        new Notification('Test Notification', {
          body: 'Click me: the window should come back and the page should navigate.',
          data: { url: 'https://chat.google.com/u/0/chat/home' }
        });
    "#;

    if let Err(e) = window.eval(script) {
        log::error!("notification: failed to fire a test notification: {e}");
    }
}

/// Debug only: act as if the last notification had been clicked.
///
/// Clicking a real popup cannot be automated -- Cinnamon draws notifications
/// inside the compositor, so there is no window to target -- which left the
/// page half of the click path (dispatching onto Chat's own object, and the
/// link fallback when nothing handles it) unverifiable without a human and a
/// real incoming message. This is that click, minus the mouse.
#[cfg(debug_assertions)]
pub fn activate_last(app: &AppHandle) {
    let id = LAST_ID.load(std::sync::atomic::Ordering::Relaxed);
    if id == 0 {
        log::warn!("notification: nothing has been shown yet to activate");
        return;
    }
    activated(app, id);
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
