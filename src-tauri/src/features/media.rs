//! Linux only: the camera and mic.
//!
//! WebKitGTK asks the host for every camera or mic request through
//! `WebKitWebView::permission-request`, and a request nobody answers is
//! refused. Neither Tauri 2.11 nor wry 0.55 connects that signal, so every
//! `getUserMedia` failed. Measured on Mint 22.3 / WebKitGTK 2.52.6 in the
//! signed-in app: `enumerateDevices` listed `audioinput,videoinput`, and
//! `getUserMedia({audio, video})` came back `NotAllowedError` at once, with no
//! prompt. With the request allowed, both tracks open and every device comes
//! with its name. The `enable-media-stream` setting played no part: it is on
//! by default in 2.52.
//!
//! A call needs WebRTC as well, and that is out of reach: `RTCPeerConnection`
//! is undefined in the app, and stays undefined with WebKit's `enable-webrtc`
//! setting switched on -- measured in the app and in a bare WebKitGTK view.
//! Ubuntu's 2.52.6 build (Mint uses it too) carries no GStreamer WebRTC at
//! all, so there is nothing for the setting to enable, and it is not set here.
//!
//! Granted without asking, because that is what the electron app did --
//! Chromium grants every permission unless the host says otherwise.
//!
//! Only on Chat itself. WebKitGTK does not say which frame is asking, so the
//! check is on the page the window is showing, which is what the user sees.

use tauri::WebviewWindow;
use webkit2gtk::glib::prelude::*;
use webkit2gtk::{
    DeviceInfoPermissionRequest, PermissionRequest, PermissionRequestExt,
    UserMediaPermissionRequest, UserMediaPermissionRequestExt, WebViewExt,
};

/// What a permission request is for, as far as this module cares.
#[derive(Debug, PartialEq)]
enum Request {
    /// `getUserMedia`. Screen capture is the same type, but WebKit reports it
    /// on a third, display, flag rather than on these two -- so it should
    /// arrive with neither set and be refused here. Not yet measured.
    Media { audio: bool, video: bool },
    /// `enumerateDevices` naming the devices -- WebKit asks before it hands a
    /// page device labels, and a device picker has nothing to show without
    /// them.
    DeviceInfo,
    /// Notifications, geolocation, pointer lock and the rest: left to WebKit,
    /// which refuses them. `chat.js` stands in for notifications.
    Other,
}

fn classify(request: &PermissionRequest) -> Request {
    if let Some(media) = request.dynamic_cast_ref::<UserMediaPermissionRequest>() {
        return Request::Media {
            audio: media.is_for_audio_device(),
            video: media.is_for_video_device(),
        };
    }
    if request.is::<DeviceInfoPermissionRequest>() {
        return Request::DeviceInfo;
    }
    Request::Other
}

fn should_allow(request: &Request, page: Option<&url::Url>) -> bool {
    let on_chat = page.is_some_and(crate::urls::is_chat_page);
    match request {
        Request::Media { audio, video } => on_chat && (*audio || *video),
        Request::DeviceInfo => on_chat,
        Request::Other => false,
    }
}

pub fn install(window: &WebviewWindow) {
    let asked = window.with_webview(|webview| {
        webview.inner().connect_permission_request(|view, request| {
            let kind = classify(request);
            let page = view.uri().and_then(|u| url::Url::parse(&u).ok());
            let allow = should_allow(&kind, page.as_ref());

            let on = page
                .as_ref()
                .map(crate::redact::foreign_url)
                .unwrap_or_else(|| "<no url>".into());
            match (&kind, allow) {
                (Request::Other, _) => {
                    log::debug!("media: left to WebKit: {} on {on}", request.type_().name())
                }
                (_, true) => log::info!("media: allowed {kind:?} on {on}"),
                (_, false) => log::info!("media: refused {kind:?} on {on}"),
            }

            // Returning false hands the request back to WebKit, which refuses
            // it -- the same answer as before this handler existed.
            if allow {
                request.allow();
            }
            allow
        });
    });

    if let Err(e) = asked {
        log::warn!("media: no webview to answer camera and mic requests: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(u: &str) -> Option<url::Url> {
        Some(url::Url::parse(u).unwrap())
    }

    const CHAT: &str = "https://mail.google.com/chat/u/0/#chat/home";

    #[test]
    fn chat_gets_the_camera_and_mic() {
        for (audio, video) in [(true, false), (false, true), (true, true)] {
            let request = Request::Media { audio, video };
            assert!(should_allow(&request, page(CHAT).as_ref()), "{request:?}");
        }
        assert!(should_allow(&Request::DeviceInfo, page(CHAT).as_ref()));
    }

    #[test]
    fn nothing_else_does() {
        let camera = Request::Media {
            audio: true,
            video: true,
        };
        for u in [
            "https://accounts.google.com/ServiceLogin",
            "https://mail.google.com/mail/u/0/",
        ] {
            assert!(!should_allow(&camera, page(u).as_ref()), "{u}");
            assert!(!should_allow(&Request::DeviceInfo, page(u).as_ref()), "{u}");
        }
        assert!(!should_allow(&camera, None));
    }

    #[test]
    fn screen_capture_and_the_rest_stay_refused() {
        let screen = Request::Media {
            audio: false,
            video: false,
        };
        assert!(!should_allow(&screen, page(CHAT).as_ref()));
        assert!(!should_allow(&Request::Other, page(CHAT).as_ref()));
    }
}
