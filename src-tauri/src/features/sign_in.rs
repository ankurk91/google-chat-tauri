//! Getting back to the sign-in form when Google parks the window elsewhere.
//!
//! After **File > Sign Out** the browser follows
//! `accounts/Logout?continue=<APP_URL>`, and a session-less visit to Chat is
//! answered either with the sign-in form or with an advertisement for Workspace
//! -- Google's choice, and not a consistent one. The advertisement strands the
//! app: it is not Chat, **History > Go to Chat** only bounces off the same
//! redirect, and it is not one of the origins the IPC capability covers, so
//! `chat.js` cannot hand its links to Rust either. The only way back used to be
//! **Help > Reset App Data**, which throws the whole profile away.
//!
//! So watch what commits and send the window at the sign-in form directly. The
//! rule lives in `urls::is_signed_out_landing`; the loop guard is in `AppState`.

use tauri::{Manager, WebviewWindow};

use crate::state::AppState;

/// Called for every page that starts loading in the main window.
pub fn check(window: &WebviewWindow, url: &url::Url) {
    let app = window.app_handle().clone();
    let state = app.state::<AppState>();

    if !crate::urls::is_signed_out_landing(url) {
        // Anything else -- Chat, the sign-in form, an identity provider -- ends
        // the run. Whatever strands the user next is a fresh case.
        state.clear_rescues();
        return;
    }

    if !state.claim_rescue() {
        // The sign-in form is bouncing straight back here. Another hop would
        // only loop, and a loop is worse than a dead end the user can click
        // their own way out of -- which they now can: off the Chat origins
        // `chat.js` leaves an ordinary link to the page that owns it, and for
        // the ones it does take it falls back to navigating this window.
        log::warn!(
            "sign-in: back at {} after redirecting twice; leaving it alone",
            crate::redact::foreign_url(url)
        );
        return;
    }

    let Ok(target) = crate::urls::sign_in_url().parse::<url::Url>() else {
        return;
    };
    log::info!(
        "sign-in: no session at {}; redirecting to {}",
        crate::redact::foreign_url(url),
        crate::redact::url(&target)
    );

    // From another thread on purpose. `navigate` runs inline when it is already
    // on the main thread, and this is called from inside the webview's own
    // load-changed handler -- so it would ask WebKit to start a second load
    // from within the first one's callback. Off-thread it goes through the
    // event loop instead and runs once this load has settled.
    let window = window.clone();
    std::thread::spawn(move || {
        let _ = window.navigate(target);
    });
}
