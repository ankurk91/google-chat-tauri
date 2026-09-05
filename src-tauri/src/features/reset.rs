//! Reset app data: sign out, forget every preference, start over.
//!
//! Split in two halves, because a running process cannot delete the storage it
//! is holding open. `request` asks, drops a sentinel file and restarts;
//! `take_pending` runs at the very top of the next launch -- before any plugin
//! or the webview has opened a single one of those files -- and does the
//! deleting.
//!
//! The first attempt at this called `WebviewWindow::clear_all_browsing_data`
//! and restarted immediately after. It looked like it worked and did not: the
//! WebKit call is asynchronous, and WebKit's network process rewrites the
//! cookie jar as it shuts down, so the session survived the restart.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tauri::{AppHandle, Manager};

/// Written into the app config directory, read once at the next startup. Holds
/// the process id that asked, which the next launch uses to tell a restart from
/// someone starting the app again by hand.
const SENTINEL: &str = ".reset-pending";

/// Kept when emptying a directory: on Linux and Windows the log directory sits
/// inside the app's local data directory, this process already has the current
/// file open, and a reset is exactly when the log is worth reading.
const KEEP: &[&str] = &["logs"];

/// Ask, then restart into a clean profile.
///
/// The last-resort fix for a wedged session, so it asks first -- it cannot be
/// undone, and the user has to sign in again afterwards.
pub fn request(app: &AppHandle) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

    let app = app.clone();
    app.clone()
        .dialog()
        .message(
            "You will be signed out and all settings will return to their \
             defaults.\n\nThe app will restart.",
        )
        .title("Reset app data?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Reset".into(),
            "Cancel".into(),
        ))
        .show(move |confirmed| {
            if confirmed {
                perform(&app);
            }
        });
}

/// Mark the reset and restart into it. Answering the dialog is the only way in
/// -- except in a debug build, where `--test-reset` calls this directly so the
/// whole path can be scripted (see `lib.rs`).
pub fn perform(app: &AppHandle) {
    // Launch at login lives in the desktop environment, not in any file we are
    // about to delete, so it has to be turned off from here.
    crate::features::autostart::set(app, false);

    let Ok(dir) = app.path().app_config_dir() else {
        log::error!("reset: no config directory; nothing done");
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    if let Err(e) = std::fs::write(dir.join(SENTINEL), std::process::id().to_string()) {
        log::error!("reset: failed to mark the reset as pending: {e}");
        return;
    }

    log::info!("reset: restarting to wipe app data");

    // The same flag the Quit menu item sets. Without it the close-to-tray
    // handler vetoes the window close the exit has to go through and the
    // process hangs instead of coming back.
    app.state::<crate::state::AppState>().set_quitting();

    // `request_restart`, not `restart`: it asks the event loop to exit and
    // returns, so the replacement is spawned from `RunEvent::Exit`, after the
    // plugins have shut down. `restart` skips that -- it spawns the new process
    // while this one still holds the single-instance name, so the new one hands
    // its argv to the process on its way out and exits, leaving nothing
    // running. It also never returns, which deadlocks the callers that reach
    // here from a plugin thread.
    app.request_restart();
}

/// If a reset was requested before the last exit, carry it out.
///
/// Must be called before the Tauri builder runs: the window state plugin reads
/// its file during setup and the webview opens the cookie jar as soon as it is
/// created, and neither would notice the files disappearing underneath it.
/// That is also why the identifier is passed in rather than resolved from an
/// `AppHandle` -- there is no app yet. These paths mirror `PathResolver`.
pub fn take_pending(identifier: &str) {
    let Some(sentinel) = dirs::config_dir().map(|d| d.join(identifier).join(SENTINEL)) else {
        return;
    };
    if !sentinel.exists() {
        return;
    }

    let requester = std::fs::read_to_string(&sentinel)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());

    // Clear the mark first. A wipe that fails half way is a bad reset; a wipe
    // that runs again at every launch is a broken app.
    let _ = std::fs::remove_file(&sentinel);

    wait_for_previous_process(requester);

    for dir in data_dirs(identifier) {
        empty_dir(&dir, KEEP);
    }

    // Logging is not up yet -- the log plugin is installed by the builder that
    // has not run -- so this and the failures above go to stderr. The log file
    // was just emptied of everything but itself anyway.
    eprintln!("reset: app data wiped");
}

/// Every directory this app keeps state in, deduplicated.
///
/// The bases overlap by platform: on Linux config and data are separate trees,
/// on macOS both are `~/Library/Application Support`, and on Windows the
/// roaming and local trees differ. Listing all of them and deduplicating is
/// simpler than three sets of rules, and covers the webview's own storage --
/// WebKitGTK's cookie jar, WebView2's `EBWebView`, WKWebView's `~/Library/
/// WebKit` -- which is the part that actually keeps the user signed in.
fn data_dirs(identifier: &str) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [
        dirs::config_dir(),
        dirs::data_dir(),
        dirs::data_local_dir(),
        dirs::cache_dir(),
    ]
    .into_iter()
    .flatten()
    .map(|base| base.join(identifier))
    .collect();

    #[cfg(target_os = "macos")]
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("Library/WebKit").join(identifier));
    }

    dirs.sort();
    dirs.dedup();
    dirs
}

/// Delete everything in `dir` except the named top-level entries.
///
/// The directory itself stays: it is about to be used again, and on macOS one
/// of these is the parent of the app's own bundle data.
fn empty_dir(dir: &Path, keep: &[&str]) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        if keep.iter().any(|k| name == *k) {
            continue;
        }

        let path = entry.path();
        let result = if entry.file_type().is_ok_and(|t| t.is_dir()) {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };

        if let Err(e) = result {
            eprintln!("reset: failed to remove {}: {e}", path.display());
        }
    }
}

/// Wait for the process that asked for the reset to be gone.
///
/// A restart spawns us and only then exits, so it may still be alive right now
/// with WebKit holding the cookie jar -- and WebKit writes that jar out as it
/// shuts down, which would restore the session we are here to destroy.
///
/// It spawned us directly, so if it is still our parent it has not exited yet.
/// If it is not, the reset is being picked up by a launch that has nothing to
/// do with it -- the user quit instead of letting it restart, say -- and there
/// is nothing to wait for.
#[cfg(unix)]
fn wait_for_previous_process(requester: Option<u32>) {
    use std::time::Instant;

    if requester == Some(std::os::unix::process::parent_id()) {
        let parent = std::os::unix::process::parent_id();
        let deadline = Instant::now() + Duration::from_secs(5);

        while std::os::unix::process::parent_id() == parent && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    // The webview keeps its storage in helper processes that outlive the parent
    // by a moment. Nothing observable to wait on, so give them one.
    std::thread::sleep(Duration::from_millis(250));
}

/// No cheap parent-id check here, so wait a fixed moment instead. It costs a
/// second on the one launch that follows a reset, and nothing on any other.
#[cfg(not(unix))]
fn wait_for_previous_process(_requester: Option<u32>) {
    std::thread::sleep(Duration::from_millis(1500));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dirs_are_unique_and_named_after_the_app() {
        let dirs = data_dirs("com.example.test");
        assert!(!dirs.is_empty());

        let mut seen = dirs.clone();
        seen.dedup();
        assert_eq!(seen.len(), dirs.len(), "duplicate directories: {dirs:?}");

        for dir in &dirs {
            assert!(dir.ends_with("com.example.test"), "stray path: {dir:?}");
        }
    }

    #[test]
    fn empty_dir_keeps_only_the_named_entries() {
        let root = std::env::temp_dir().join(format!("reset-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("logs")).unwrap();
        std::fs::create_dir_all(root.join("localstorage")).unwrap();
        std::fs::write(root.join("logs/app.log"), "kept").unwrap();
        std::fs::write(root.join("cookies"), "gone").unwrap();

        empty_dir(&root, KEEP);

        assert!(root.join("logs/app.log").exists());
        assert!(!root.join("cookies").exists());
        assert!(!root.join("localstorage").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn emptying_a_missing_directory_is_not_an_error() {
        empty_dir(Path::new("/nonexistent/reset-test"), KEEP);
    }
}
