//! Ported from electron `src/main/features/externalLinks.ts`.
//!
//! Where the policy lives, and why:
//!
//! `on_navigation` looks like the natural place for a host allow-list, but wry's
//! WebKitGTK backend hooks `PolicyDecisionType::NavigationAction`, which fires
//! for **every frame**, not just the top one. An allow-list there cancels
//! legitimate third-party iframes on Google's sign-in page and shoves them at
//! the system browser. So it only enforces the one frame-agnostic rule: no
//! non-http(s) schemes.
//!
//! The real guard is in `chat.js`, which -- because `initialization_script`
//! injects into the main frame only -- is inherently top-level. It hands
//! `target=_blank` and cross-origin link clicks to `commands::open_external_url`,
//! which applies the strict allow-list ported from the electron app.
//!
//! This also matches what electron actually did: it had no `will-navigate`
//! handler at all, only `setWindowOpenHandler`.
//!
//! The one way out of that policy is `toggle_in_app` below, which suspends it
//! for five minutes so an external identity provider can finish a sign-in in
//! this window rather than in the browser.

use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::state::AppState;

/// How long "open every link in this window" lasts before it turns itself off.
///
/// Five minutes, matching the electron app. Long enough to walk through an
/// identity provider -- Okta, an SSO portal, a hardware key prompt -- and short
/// enough that nobody leaves the window pointed at the whole web by accident.
/// The grant is deliberately *not* written to the config file: it should never
/// survive a restart.
const GRANT: Duration = Duration::from_secs(5 * 60);

/// How long the grant lasts, shortened for testing.
///
/// Debug builds only, like `connectivity::probe_host`: waiting out five real
/// minutes is the only way to watch the grant lapse and the menu untick itself,
/// and an environment variable that can quietly widen the link policy has no
/// business in a release.
///
/// ```text
/// GOOGLE_CHAT_LINK_GRANT_SECS=10 ./google-chat-tauri
/// ```
#[cfg(debug_assertions)]
pub fn grant() -> Duration {
    match std::env::var("GOOGLE_CHAT_LINK_GRANT_SECS") {
        Ok(secs) => match secs.parse() {
            Ok(secs) => Duration::from_secs(secs),
            Err(e) => {
                log::warn!("external links: ignoring GOOGLE_CHAT_LINK_GRANT_SECS={secs}: {e}");
                GRANT
            }
        },
        Err(_) => GRANT,
    }
}

#[cfg(not(debug_assertions))]
pub fn grant() -> Duration {
    GRANT
}

/// The menu item's id, shared with `app_menu` so the two cannot drift apart.
pub const MENU_ID: &str = "pref-links-in-app";

/// What the Preferences menu calls it.
///
/// "Temporarily", not a number of minutes: the menu is where the setting is
/// switched, and the dialog that comes next is where it is explained. Putting
/// the duration in both means keeping them in step for no gain.
pub const MENU_LABEL: &str = "Temporarily Open Every Link in This Window";

/// The grant's length in words, for that dialog. Seconds are only reachable in
/// a debug build with the override set.
fn grant_in_words() -> String {
    let secs = grant().as_secs();
    match (secs / 60, secs % 60) {
        (0, s) => format!("{s} seconds"),
        (1, 0) => "1 minute".to_string(),
        (m, 0) => format!("{m} minutes"),
        (m, s) => format!("{m} minutes {s} seconds"),
    }
}

/// Preferences > Temporarily Open Every Link in This Window.
///
/// The point is single sign-on that leaves Google: a Workspace account whose
/// identity provider is someone else's sends the user off to a host the
/// allow-list has never heard of, and handing that to the system browser
/// finishes the sign-in there instead of here.
///
/// Turning it *on* asks first, the same split as `reset::request`. It widens
/// what this window will load from a three-host allow-list to the whole web,
/// which deserves a sentence before it happens rather than a surprise
/// afterwards -- and that sentence is also where the duration is explained, so
/// the menu does not have to carry a number. Turning it off is the safe
/// direction and happens on the spot.
pub fn toggle_in_app(app: &AppHandle) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

    if app.state::<AppState>().links_open_in_app() {
        set_grant(app, false);
        return;
    }

    let app = app.clone();
    app.clone()
        .dialog()
        .message(format!(
            "Every link will open in this window instead of your browser, \
             including links to sites that have nothing to do with Google \
             Chat.\n\nThis is meant for signing in through an identity \
             provider that is not Google, such as Okta or Entra ID. Turn it \
             off again as soon as you are signed in.\n\nIt switches itself \
             off after {}.",
            grant_in_words()
        ))
        .title("Open every link in this window?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Turn On".into(),
            "Cancel".into(),
        ))
        .show(move |confirmed| {
            if confirmed {
                set_grant(&app, true);
            } else {
                // The menu ticked itself the moment it was clicked, before this
                // dialog existed to be answered. Put it back.
                log::info!("external links: declined; staying with the system browser");
                set_tick(&app, false);
            }
        });
}

/// Turn the grant on or off without asking.
///
/// The dialog above is the only way a user reaches this. `--test-links-in-app`
/// calls `toggle_without_asking` instead, so the whole path can be scripted
/// past a modal nothing here can answer -- the same reason `reset` is split
/// into `request` and `perform`.
pub fn set_grant(app: &AppHandle, on: bool) {
    let epoch = app.state::<AppState>().set_links_in_app(on, grant());

    if on {
        log::info!(
            "external links: opening in this window for the next {}s",
            grant().as_secs()
        );
        untick_when_it_lapses(app, epoch);
    } else {
        log::info!("external links: back to the system browser");
    }
}

/// Flip the grant the way clicking the menu item would, minus the dialog.
#[cfg(debug_assertions)]
pub fn toggle_without_asking(app: &AppHandle) {
    let on = !app.state::<AppState>().links_open_in_app();
    set_grant(app, on);
    // Nothing was clicked, so nothing has moved the tick.
    set_tick(app, on);
}

/// Put the menu's tick where the state says it belongs.
///
/// A check item nested in a submenu cannot be found through `Menu::get`, which
/// searches the top level only; `app_menu::nested_check_item` walks the
/// submenus by hand. `set_checked` makes its own way to the main thread, and
/// does not re-emit the menu event -- verified by watching a lapse fail to turn
/// the grant back on.
fn set_tick(app: &AppHandle, checked: bool) {
    match crate::features::app_menu::nested_check_item(app, MENU_ID) {
        Some(item) => {
            if let Err(e) = item.set_checked(checked) {
                log::error!("could not set the {MENU_ID} tick: {e}");
            }
        }
        None => log::warn!("{MENU_ID} is not in the menu; its tick is out of step"),
    }
}

/// Clear the menu's tick once the grant runs out.
///
/// Only the tick. The grant itself expires against the clock in
/// `AppState::links_open_in_app`, so links go back to the system browser on
/// time whether or not this thread ever runs.
fn untick_when_it_lapses(app: &AppHandle, epoch: u64) {
    let app = app.clone();

    std::thread::spawn(move || {
        std::thread::sleep(grant());

        // The grant has been switched since -- turned off by hand, or turned on
        // again, which is a fresh deadline with a thread of its own. Either way
        // the tick is no longer this thread's to clear, and announcing a lapse
        // that did not happen would be worse than saying nothing.
        if !app.state::<AppState>().links_epoch_holds(epoch) {
            return;
        }

        log::info!("external links: grant lapsed, back to the system browser");

        // This one item, not a rebuilt menu. Swapping the whole menu also works
        // and leaves every tick right, but GTK answers it with a burst of "no
        // accelerator installed in accel group" warnings as it tears the old
        // accelerators down -- measured on Cinnamon, one per accelerator, every
        // time a grant lapsed.
        set_tick(&app, false);
    });
}

pub fn open_in_browser(app: &AppHandle, url: &str) {
    log::info!(
        "opening externally: {}",
        crate::redact::foreign_url_str(url)
    );
    if let Err(e) = app.opener().open_url(url, None::<&str>) {
        log::error!(
            "failed to open {} externally: {e}",
            crate::redact::foreign_url_str(url)
        );
    }
}

/// Handler for `WebviewWindowBuilder::on_navigation`; `false` cancels.
///
/// Deliberately permissive: see the module note. Blocking a scheme is safe to do
/// per-frame, because no frame has a legitimate reason to navigate to
/// `file://`, `javascript:` or an external protocol handler.
pub fn navigation_guard(url: &url::Url) -> bool {
    if matches!(url.scheme(), "http" | "https") {
        return true;
    }

    // `about:blank` is routine (OAuth popups, form targets) and not worth
    // logging as a problem; anything else is worth seeing.
    if url.scheme() != "about" {
        log::warn!(
            "blocked navigation to scheme {}: {}",
            url.scheme(),
            crate::redact::foreign_url(url)
        );
    }
    false
}
