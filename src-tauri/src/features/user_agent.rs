//! Ported from electron `src/main/features/userAgent.ts`.
//!
//! Google serves a degraded experience (and `accounts.google.com` gets actively
//! hostile) to browsers it does not recognise. WebKitGTK identifies as Safari on
//! Linux, which trips this. The electron app shipped a Firefox user-agent for
//! years and it reliably passes both sign-in and the full Chat UI, so we keep it.
//!
//! Difference from electron: it rewrote the `User-Agent` *request header* via
//! `onBeforeSendHeaders`, which left `navigator.userAgent` untouched. Tauri sets
//! the webview-level UA, so the header and the JS-visible value agree.

/// Bump periodically. Single source of truth.
const FIREFOX_MAJOR: &str = "134";

pub fn spoofed() -> String {
    // Escape hatch for debugging Google sign-in problems without a rebuild.
    if let Ok(custom) = std::env::var("GOOGLE_CHAT_UA") {
        if !custom.trim().is_empty() {
            return custom;
        }
    }

    let platform = if cfg!(target_os = "windows") {
        "Windows NT 10.0; Win64; x64"
    } else if cfg!(target_os = "macos") {
        "Macintosh; Intel Mac OS X 10.15"
    } else {
        "X11; Ubuntu; Linux x86_64"
    };

    format!(
        "Mozilla/5.0 ({platform}; rv:{FIREFOX_MAJOR}.0) Gecko/20100101 Firefox/{FIREFOX_MAJOR}.0"
    )
}
