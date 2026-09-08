//! Windows only: the two menu accelerators the menu cannot deliver itself.
//!
//! On Windows a menu accelerator is dispatched by `TranslateAcceleratorW`,
//! which Tauri installs as a `msg_hook` on tao's message loop. That loop never
//! sees these keys: WebView2 owns the keyboard while the page has focus, which
//! is always. Measured on Windows 11 (VirtualBox guest, WebView2 152.0.4191.66)
//! with synthetic keystrokes and the window verified foreground -- Ctrl+W left
//! the window visible, Ctrl+Q left the process alive, and neither wrote the
//! `menu: {id}` line that a menu click writes. The accelerator text next to
//! **Quit** and **Close to Tray** was decoration.
//!
//! WebView2 hands every combination to the page instead -- measured against a
//! local probe page: `ctrl+w`, `ctrl+q`, `ctrl+=`, `ctrl+-`, `ctrl+0`, `ctrl+f`,
//! `ctrl+shift+w`, `ctrl+m`, `ctrl+h`, `alt+home`, `f11`, `ctrl+n`, `ctrl+t`
//! all arrived, so it reserves none of them. That is what makes `chat.js`'s
//! forwarding work here, and zoom and history need nothing more.
//!
//! Quit and close-to-tray do. Quit is refused from the page on purpose --
//! `commands::menu_action` excludes it, because that list is callable by a page
//! nobody here controls -- and forwarding of any kind only works on an origin
//! the capability names, so Ctrl+W died on the sign-in page and on the
//! marketing page a sign-out can land on. Both are fixed by taking the keys
//! before the page instead: `AcceleratorKeyPressed` is WebView2's own hook for
//! exactly this, reachable through Tauri's `PlatformWebview::controller`, and
//! it widens the IPC surface by nothing.

use tauri::{Manager, WebviewWindow};
use webview2_com::AcceleratorKeyPressedEventHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
};

// One call into user32, declared rather than depended on.
//
// The event args carry the virtual key and whether Alt was down, but not Ctrl,
// so the modifier has to be read from the keyboard state. `windows` is already
// in the tree via Tauri and would do this too, but naming it here would pin a
// second crate to whatever version Tauri happens to resolve. `GetKeyState` is
// as stable as Win32 gets, and user32 is already linked by tao.
#[link(name = "user32")]
extern "system" {
    fn GetKeyState(vk: i32) -> i16;
}

const VK_SHIFT: i32 = 0x10;
const VK_CONTROL: i32 = 0x11;
const VK_MENU: i32 = 0x12;
const VK_Q: u32 = 0x51;
const VK_W: u32 = 0x57;

/// The high bit of `GetKeyState` is "held down right now".
fn is_down(vk: i32) -> bool {
    (unsafe { GetKeyState(vk) } as u16 & 0x8000) != 0
}

/// Ask WebView2 for the keys the menu advertises and cannot deliver.
///
/// Failing to install is not fatal: the menu items themselves still work, and
/// so does the tray's Quit. It is logged rather than returned for that reason.
pub fn install(window: &WebviewWindow) {
    let app = window.app_handle().clone();

    let asked = window.with_webview(move |webview| {
        let controller = webview.controller();

        let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else {
                return Ok(());
            };

            // Key-down only. The event fires again on release, and Alt combos
            // arrive as SYSTEM_KEY_DOWN, which is not ours either.
            let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
            unsafe { args.KeyEventKind(&mut kind)? };
            if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN {
                return Ok(());
            }

            // Exactly Ctrl. Ctrl+Shift+Q and Ctrl+Alt+W belong to the page.
            if !is_down(VK_CONTROL) || is_down(VK_SHIFT) || is_down(VK_MENU) {
                return Ok(());
            }

            let mut vk = 0u32;
            unsafe { args.VirtualKey(&mut vk)? };
            let id = match vk {
                VK_Q => "quit",
                VK_W => "close-to-tray",
                _ => return Ok(()),
            };

            // Take the key off the page, so `chat.js` cannot act on it a second
            // time -- the same thing GTK does with a menu accelerator on Linux.
            unsafe { args.SetHandled(true)? };

            // Off-thread on purpose. This runs inside WebView2's own dispatch,
            // and `quit` ends in `app.exit`, which tears the runtime down --
            // from here that would unwind the COM call that is still on the
            // stack. The same reasoning as the redirect in `features::sign_in`.
            let app = app.clone();
            std::thread::spawn(move || crate::features::app_menu::handle(&app, id));

            Ok(())
        }));

        let mut token = 0i64;
        if let Err(e) = unsafe { controller.add_AcceleratorKeyPressed(&handler, &mut token) } {
            log::warn!("accelerators: WebView2 refused the key hook: {e}");
        } else {
            log::debug!("accelerators: Ctrl+Q and Ctrl+W taken from WebView2");
        }
    });

    if let Err(e) = asked {
        log::warn!("accelerators: no webview to hook: {e}");
    }
}
