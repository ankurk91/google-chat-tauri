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

/// What `deliver` needs, owned, so it can cross a thread boundary.
#[cfg(target_os = "linux")]
struct Job {
    app: AppHandle,
    id: u32,
    title: String,
    body: Option<String>,
}

/// Hand the notification to a worker thread instead of showing it here.
///
/// `Notification::show()` is a blocking D-Bus round-trip to the daemon, and the
/// callers that matter are all on the GTK main thread: `show_notification` is a
/// synchronous Tauri command, and Tauri runs those on the main thread, while the
/// tray's Test Notification is a menu handler. That is also the thread that
/// draws and services the window's own close, minimise and maximise buttons --
/// mutter gives Wayland clients no server-side titlebar, so GTK draws them in
/// this process -- so showing a notification there stalls them. Measured against
/// gnome-shell 50.1 over 25 calls: median 48 ms, max 520 ms, and a burst of
/// messages compounds it. See the titlebar entry in `docs/Notes.md`.
///
/// One worker, not a thread per notification: a burst then neither spawns
/// threads unboundedly nor lets the popups reach the daemon out of order, which
/// serialising on the main thread used to give for free.
#[cfg(target_os = "linux")]
fn show_linux(app: &AppHandle, id: u32, title: &str, body: Option<&str>) {
    use std::sync::{Mutex, OnceLock, mpsc};

    static QUEUE: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();

    let queue = QUEUE.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        // Detached deliberately: it lives as long as the process, and there is
        // nothing to join or report back.
        let spawned = std::thread::Builder::new()
            .name("notifications".into())
            .spawn(move || {
                for job in rx {
                    deliver(&job.app, job.id, &job.title, job.body.as_deref());
                }
            });

        if let Err(e) = spawned {
            log::error!("notification: could not start the worker thread: {e}");
        }

        Mutex::new(tx)
    });

    let job = Job {
        app: app.clone(),
        id,
        title: title.to_owned(),
        body: body.map(str::to_owned),
    };

    // A send only fails once the worker is gone, which it never is while the
    // process runs -- log rather than unwrap, because dropping a notification is
    // not worth taking the app down for.
    match queue.lock() {
        Ok(tx) => {
            if let Err(e) = tx.send(job) {
                log::error!("notification: worker gone, dropped a notification: {e}");
            }
        }
        Err(e) => log::error!("notification: queue lock poisoned: {e}"),
    }
}

/// Actually talk to the daemon. Runs on the worker thread, never the caller's.
#[cfg(target_os = "linux")]
fn deliver(app: &AppHandle, id: u32, title: &str, body: Option<&str>) {
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

    // Claimed before the action is registered, and released when the waiter
    // below finishes. Past the cap the notification still appears, it just is
    // not clickable -- which costs little, because a click here only raises the
    // window (Chat gives us nothing to navigate to; see `docs/Notes.md`).
    let waiting = actions_enabled() && claim_waiter();

    if !waiting {
        match builder.show() {
            // Logged on success as well as failure: the absence of an error is
            // also what a notification nobody ever attempted looks like, which
            // let `scripts/notification-test.py` pass vacuously while the page
            // was on an origin the ACL turns down. Positive evidence or none.
            Ok(_) => log::debug!("notification: shown id={id} actions=no"),
            Err(e) => log::error!("notification: failed to show notification: {e}"),
        }
        return;
    }

    builder.action("default", "Open");

    let handle = match builder.show() {
        Ok(h) => h,
        Err(e) => {
            release_waiter();
            log::error!("notification: failed to show notification: {e}");
            return;
        }
    };

    log::debug!("notification: shown id={id} actions=yes");

    // wait_for_action blocks until the notification is acted on, so it needs a
    // thread of its own even here: on the worker it would hold up every later
    // notification until this one was clicked or dismissed.
    //
    // Named, because Linux gives a new thread the *creating* thread's name and
    // this is created from the worker -- so without it, `ps` and gdb show two
    // threads called "notifications" and only one of them is the worker.
    let app = app.clone();
    let waiter = std::thread::Builder::new()
        .name(format!("notif-wait-{id}"))
        .spawn(move || {
            handle.wait_for_action(|action| {
                // "__closed" means dismissed, which we ignore.
                if action == "default" {
                    activated(&app, id);
                }
            });
            // Returns once the notification is clicked, dismissed or expired.
            release_waiter();
        });

    if let Err(e) = waiter {
        release_waiter();
        log::error!("notification: no thread to wait for a click on id={id}: {e}");
    }
}

/// How many notifications may be waiting on a click at once.
///
/// Each one costs a blocked thread *and* a D-Bus connection of its own, and
/// neither is released until the notification is clicked, dismissed or expired
/// -- so the cost tracks what is sitting unread on the desktop, not what has
/// been delivered. Measured with a burst of 60 while none were dismissed:
/// threads went 44 -> 85, settling at 81 with 18 popups still on screen, made
/// up of 18 `notif-wait` and 19 `zbus::Connection` threads. Everything drained
/// the moment the tray was cleared, so this is not a leak; it is an unbounded
/// cost for being away from the desk while a channel is busy.
///
/// Sixteen is chosen for the shape of the loss rather than the number: past it
/// a notification is still shown, and all that goes is click-to-raise on the
/// oldest ones -- which on Linux only raises the window anyway.
#[cfg(target_os = "linux")]
const MAX_WAITERS: usize = 16;

#[cfg(target_os = "linux")]
static WAITERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Take a waiter slot, or report that they are all taken.
#[cfg(target_os = "linux")]
fn claim_waiter() -> bool {
    use std::sync::atomic::Ordering;

    let claimed = WAITERS
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
            (n < MAX_WAITERS).then_some(n + 1)
        })
        .is_ok();

    if !claimed {
        log::debug!("notification: {MAX_WAITERS} clicks already pending; showing without one");
    }
    claimed
}

#[cfg(target_os = "linux")]
fn release_waiter() {
    use std::sync::atomic::Ordering;

    WAITERS.fetch_sub(1, Ordering::SeqCst);
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn the_waiter_cap_holds_and_releases() {
        // The only test touching WAITERS, and it puts the counter back.
        for slot in 0..MAX_WAITERS {
            assert!(claim_waiter(), "slot {slot} should still be free");
        }
        assert!(!claim_waiter(), "the cap must hold at {MAX_WAITERS}");

        // A notification being dismissed frees exactly one slot, so a busy
        // desktop recovers click-through as the user works through the tray.
        release_waiter();
        assert!(claim_waiter(), "a dismissal frees a slot");
        assert!(!claim_waiter(), "and only one");

        for _ in 0..MAX_WAITERS {
            release_waiter();
        }
        assert_eq!(
            WAITERS.load(Ordering::SeqCst),
            0,
            "slots must all come back"
        );
    }
}
