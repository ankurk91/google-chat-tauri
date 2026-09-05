//! The window menu bar. Ported from electron `src/main/features/appMenu.ts`.
//!
//! On Linux and Windows this renders as a menu bar under the title bar; on
//! macOS it becomes the application menu. `AppHandle::set_menu` handles that
//! difference for us, so there is one definition rather than three.
//!
//! Accelerators declared here are dispatched by the OS menu, which on GTK is not
//! guaranteed to receive keys while focus is inside the webview. Anything that
//! must work reliably from inside the page -- Ctrl+F search being the one that
//! matters -- is handled in `chat.js` instead.

use tauri::menu::{AboutMetadata, Menu, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Manager, Runtime};

use crate::config::{self, Config, ZOOM_MAX, ZOOM_MIN, ZOOM_STEP};
use crate::features::window::MAIN;
use crate::state::AppState;

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let file = SubmenuBuilder::new(app, "File")
        .item(
            &MenuItemBuilder::with_id("close-to-tray", "Close to Tray")
                .accelerator("CmdOrCtrl+W")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("reload", "Reload")
                .accelerator("CmdOrCtrl+R")
                .build(app)?,
        )
        .separator()
        .item(&MenuItemBuilder::with_id("sign-out", "Sign Out").build(app)?)
        .separator()
        .item(
            &MenuItemBuilder::with_id("quit", "Quit")
                .accelerator("CmdOrCtrl+Q")
                .build(app)?,
        )
        .build()?;

    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let mut view = SubmenuBuilder::new(app, "View")
        .item(
            &MenuItemBuilder::with_id("zoom-in", "Zoom In")
                .accelerator("CmdOrCtrl+Plus")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("zoom-out", "Zoom Out")
                .accelerator("CmdOrCtrl+-")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("zoom-reset", "Actual Size")
                .accelerator("CmdOrCtrl+0")
                .build(app)?,
        )
        .separator()
        .fullscreen();

    // Only useful in a dev build; shipping it invites confusion.
    #[cfg(debug_assertions)]
    {
        view = view.separator().item(
            &MenuItemBuilder::with_id("devtools", "Developer Tools")
                .accelerator("CmdOrCtrl+Shift+I")
                .build(app)?,
        );
    }
    let view = view.build()?;

    let history = SubmenuBuilder::new(app, "History")
        .item(
            &MenuItemBuilder::with_id("back", "Back")
                .accelerator("Alt+Left")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("forward", "Forward")
                .accelerator("Alt+Right")
                .build(app)?,
        )
        .separator()
        .item(
            &MenuItemBuilder::with_id("home", "Go to Chat")
                .accelerator("Alt+Home")
                .build(app)?,
        )
        .build()?;

    let help = SubmenuBuilder::new(app, "Help")
        .item(&MenuItemBuilder::with_id("report-issue", "Report an Issue").build(app)?)
        .separator()
        .about(Some(AboutMetadata {
            name: Some("Google Chat".into()),
            icon: crate::icons::decode(crate::icons::APP).ok(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
            authors: Some(vec!["ankurk91".into()]),
            comments: Some("Unofficial desktop app for Google Chat.".into()),
            license: Some("GPL-3.0-only".into()),
            website: Some(env!("CARGO_PKG_REPOSITORY").into()),
            ..Default::default()
        }))
        .build()?;

    Menu::with_items(app, &[&file, &edit, &view, &history, &help])
}

pub fn handle(app: &AppHandle, id: &str) {
    eprintln!("[menu] {id}");
    let Some(window) = app.get_webview_window(MAIN) else {
        return;
    };

    match id {
        "close-to-tray" => {
            #[cfg(target_os = "macos")]
            let _ = app.hide();
            #[cfg(not(target_os = "macos"))]
            let _ = window.hide();
        }
        "reload" => {
            if let Ok(url) = window.url() {
                let _ = window.navigate(url);
            }
        }
        "sign-out" => {
            if let Ok(url) = crate::urls::logout_url().parse() {
                let _ = window.navigate(url);
            }
        }
        "quit" => {
            app.state::<AppState>().set_quitting();
            app.exit(0);
        }

        "zoom-in" => set_zoom(app, |z| z + ZOOM_STEP),
        "zoom-out" => set_zoom(app, |z| z - ZOOM_STEP),
        "zoom-reset" => set_zoom(app, |_| 1.0),

        "back" => {
            // No history API on Webview; the page's own history works fine.
            let _ = window.eval("history.back()");
        }
        "forward" => {
            let _ = window.eval("history.forward()");
        }
        "home" => {
            if let Ok(url) = crate::urls::APP_URL.parse() {
                let _ = window.navigate(url);
            }
        }

        "report-issue" => {
            crate::features::external_links::open_in_browser(app, &crate::urls::issue_url());
        }

        #[cfg(debug_assertions)]
        "devtools" => window.open_devtools(),

        _ => {}
    }
}

/// Apply a zoom change, clamp it, and remember it.
pub fn set_zoom(app: &AppHandle, f: impl FnOnce(f64) -> f64) {
    let prefs = app.state::<Config>().update(|p| {
        // Round to avoid float drift accumulating across many steps.
        p.zoom = (f(p.zoom).clamp(ZOOM_MIN, ZOOM_MAX) * 100.0).round() / 100.0;
    });

    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.set_zoom(prefs.zoom);
    }
    config::save(app, &prefs);
}
