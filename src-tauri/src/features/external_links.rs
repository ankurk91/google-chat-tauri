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

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

pub fn open_in_browser(app: &AppHandle, url: &str) {
    log::info!("opening externally: {url}");
    if let Err(e) = app.opener().open_url(url, None::<&str>) {
        log::error!("failed to open {url} externally: {e}");
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
        log::warn!("blocked navigation to scheme {}: {url}", url.scheme());
    }
    false
}
