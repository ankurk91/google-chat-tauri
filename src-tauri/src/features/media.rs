//! The camera and mic.
//!
//! Each webview asks the host before a page gets a camera or mic, and each
//! falls back to something different when the host has no answer: WebKitGTK
//! refuses, WKWebView grants every page, WebView2 shows a prompt of its own.
//! Measured on Mint 22.3 / WebKitGTK 2.52.6 in the signed-in app, before any
//! handler existed: `enumerateDevices` listed `audioinput,videoinput`, and
//! `getUserMedia({audio, video})` came back `NotAllowedError` at once, with no
//! prompt. With the request allowed, both tracks open.
//!
//! So `answer` is given to Tauri's permission handler, which reaches all three
//! webviews, and gives the same answer on each: granted without asking,
//! because that is what the electron app did -- Chromium grants every
//! permission unless the host says otherwise -- and only on Chat itself. The
//! webviews do not all say which frame is asking, so the check is on the page
//! the window is showing, which is what the user sees.
//!
//! macOS asks once more on its own, the first time: the system prompt, which
//! needs the usage strings in `Info.plist`. WebKit refuses a capture request
//! outright from an app that has none.
//!
//! A call needs WebRTC as well, and on Linux that is out of reach:
//! `RTCPeerConnection` is undefined in the app, and stays undefined with
//! WebKit's `enable-webrtc` setting switched on -- measured in the app and in a
//! bare WebKitGTK view. Ubuntu's 2.52.6 build (Mint uses it too) carries no
//! GStreamer WebRTC at all, so there is nothing for the setting to enable.

use tauri::webview::{PermissionKind, PermissionResponse};
use tauri::{Runtime, Webview};

/// Tauri's permission handler: camera and mic for Chat, refused anywhere
/// else; everything else left to the webview, as before this handler existed.
///
/// Screen capture comes as its own kind and is left alone. Tauri on Linux
/// sends a combined camera-and-mic request here twice, once for each.
pub fn answer<R: Runtime>(webview: Webview<R>, kind: PermissionKind) -> PermissionResponse {
    if !matches!(kind, PermissionKind::Camera | PermissionKind::Microphone) {
        return PermissionResponse::Default;
    }

    let page = webview.url().ok();
    let allow = should_allow(page.as_ref());
    let on = page
        .as_ref()
        .map(crate::redact::foreign_url)
        .unwrap_or_else(|| "<no url>".into());
    if allow {
        log::info!("media: allowed {kind:?} on {on}");
        PermissionResponse::Allow
    } else {
        log::info!("media: refused {kind:?} on {on}");
        PermissionResponse::Deny
    }
}

fn should_allow(page: Option<&url::Url>) -> bool {
    page.is_some_and(crate::urls::is_chat_page)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(u: &str) -> Option<url::Url> {
        Some(url::Url::parse(u).unwrap())
    }

    #[test]
    fn chat_gets_the_camera_and_mic() {
        assert!(should_allow(
            page("https://mail.google.com/chat/u/0/#chat/home").as_ref()
        ));
        assert!(should_allow(page("https://chat.google.com/").as_ref()));
    }

    #[test]
    fn nothing_else_does() {
        for u in [
            "https://accounts.google.com/ServiceLogin",
            "https://mail.google.com/mail/u/0/",
        ] {
            assert!(!should_allow(page(u).as_ref()), "{u}");
        }
        assert!(!should_allow(None));
    }
}
