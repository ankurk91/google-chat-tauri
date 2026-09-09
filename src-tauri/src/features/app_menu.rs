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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::menu::{
    AboutMetadata, CheckMenuItem, CheckMenuItemBuilder, Menu, MenuItemBuilder, SubmenuBuilder,
};
use tauri::{AppHandle, Manager, Runtime};

use crate::config::{self, Config, ZOOM_MAX, ZOOM_MIN, ZOOM_STEP};
use crate::features::window::MAIN;
use crate::state::AppState;

/// What the About dialog shows. Shared with the tray menu, which offers the
/// same item -- on macOS and Windows the window menu bar is not always in front
/// of the user, and on Linux there are setups where the tray is all there is.
pub fn about_metadata() -> AboutMetadata<'static> {
    AboutMetadata {
        name: Some("Google Chat".into()),
        icon: crate::icons::decode(crate::icons::APP).ok(),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        authors: Some(vec!["ankurk91".into()]),
        comments: Some("Unofficial desktop app for Google Chat.".into()),
        license: Some("GPL-3.0-only".into()),
        website: Some(env!("CARGO_PKG_REPOSITORY").into()),
        ..Default::default()
    }
}

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

    // muda's predefined Undo/Redo are macOS and Windows only -- on Linux they
    // are documented Unsupported and simply do not appear, which is why the
    // Edit menu there was missing its first two entries. Custom items with the
    // same labels close the gap, driven through the page's own edit stack.
    //
    // They deliberately declare no accelerator: a menu accelerator is consumed
    // by GTK before the webview sees the key (measured -- see the note in
    // chat.js), so claiming Ctrl+Z here would take the working native undo away
    // from every text field and hand it to execCommand, which cannot reach an
    // editable inside a cross-origin frame. Clicking the item is additive; the
    // keystroke is left alone.
    let mut edit = SubmenuBuilder::new(app, "Edit");
    #[cfg(not(target_os = "linux"))]
    {
        edit = edit.undo().redo();
    }
    #[cfg(target_os = "linux")]
    {
        edit = edit
            .item(&MenuItemBuilder::with_id("undo", "Undo").build(app)?)
            .item(&MenuItemBuilder::with_id("redo", "Redo").build(app)?);
    }
    let edit = edit.separator().cut().copy().paste().select_all().build()?;

    // `mut` is only needed in debug builds, where the devtools item below
    // reassigns this.
    #[cfg_attr(not(debug_assertions), allow(unused_mut))]
    let mut view = SubmenuBuilder::new(app, "View")
        // "Plus" is not a key name muda knows -- it parses only NumpadPlus --
        // and tauri drops an accelerator it cannot parse instead of failing, so
        // this item silently had none and showed no shortcut at all next to a
        // Zoom Out that did. "Equal" is the physical key, and matches what
        // chat.js accepts (= as well as +).
        .item(
            &MenuItemBuilder::with_id("zoom-in", "Zoom In")
                .accelerator("CmdOrCtrl+Equal")
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
        .item(&MenuItemBuilder::with_id("copy-url", "Copy Current URL").build(app)?);

    // muda's Fullscreen is macOS-only. Linux never rendered it, but Windows
    // draws the item and then does nothing when it is clicked -- an entry that
    // exists only to disappoint. Ask for it where it works.
    #[cfg(target_os = "macos")]
    {
        view = view.separator().fullscreen();
    }

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

    // Checkbox state is read from the OS and the config file, not assumed:
    // the user may have removed the autostart entry through their desktop's own
    // startup-applications tool.
    let prefs = app.state::<Config>().get();
    let preferences = SubmenuBuilder::new(app, "Preferences")
        .item(
            &CheckMenuItemBuilder::with_id("pref-autostart", "Launch at Login")
                .checked(crate::features::autostart::is_enabled(app))
                .enabled(!cfg!(debug_assertions))
                .build(app)?,
        )
        .item(
            &CheckMenuItemBuilder::with_id("pref-start-hidden", "Start Hidden in Tray")
                .checked(prefs.start_hidden)
                .build(app)?,
        )
        .item(
            &CheckMenuItemBuilder::with_id("pref-check-updates", "Check for Updates Automatically")
                .checked(prefs.check_updates)
                .build(app)?,
        )
        .separator()
        // Not a stored preference, which is why it sits below the separator:
        // it lapses on its own and never survives a restart. See
        // `features::external_links::toggle_in_app`.
        .item(
            &CheckMenuItemBuilder::with_id(
                crate::features::external_links::MENU_ID,
                crate::features::external_links::MENU_LABEL,
            )
            .checked(app.state::<AppState>().links_open_in_app())
            .build(app)?,
        )
        .build()?;

    let help = SubmenuBuilder::new(app, "Help")
        .item(&MenuItemBuilder::with_id("check-updates", "Check for Updates").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id("report-issue", "Report an Issue").build(app)?)
        .item(&MenuItemBuilder::with_id("show-logs", "Show Logs").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id("reset-app", "Reset App Data...").build(app)?)
        .separator()
        .about(Some(about_metadata()))
        .build()?;

    Menu::with_items(app, &[&file, &edit, &view, &history, &preferences, &help])
}

/// Find a check item one level down, by id.
///
/// `Menu::get` searches the top level only, which is why the check items in
/// Preferences cannot be read back through it -- and why the toggles below
/// track their own state instead of asking the menu. Walking the submenus by
/// hand does reach them, and the link grant needs that: it is the one setting
/// that switches itself off, so something has to clear its tick without the
/// user clicking anything.
pub fn nested_check_item<R: Runtime>(app: &AppHandle<R>, id: &str) -> Option<CheckMenuItem<R>> {
    for top in app.menu()?.items().ok()? {
        let Some(submenu) = top.as_submenu() else {
            continue;
        };
        for item in submenu.items().ok()? {
            if let Some(check) = item.as_check_menuitem() {
                if check.id() == id {
                    return Some(check.clone());
                }
            }
        }
    }
    None
}

pub fn handle(app: &AppHandle, id: &str) {
    log::debug!("menu: {id}");

    // Quit is the user's escape hatch and must work even if the window has
    // gone; everything else needs one, so look it up lazily.
    if id == "quit" {
        app.state::<AppState>().set_quitting();
        // The write throttle may still be holding a change; take it now.
        config::flush(app);
        app.exit(0);
        return;
    }

    let Some(window) = app.get_webview_window(MAIN) else {
        log::warn!("menu action {id} ignored: no main window");
        return;
    };

    match id {
        "close-to-tray" => {
            #[cfg(target_os = "macos")]
            let _ = app.hide();
            #[cfg(not(target_os = "macos"))]
            let _ = window.hide();
        }
        // Linux only -- elsewhere Undo/Redo are predefined items the platform
        // handles itself, and never reach this match.
        "undo" => {
            let _ = window.eval("document.execCommand('undo')");
        }
        "redo" => {
            let _ = window.eval("document.execCommand('redo')");
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

        // A check item flips its own tick before the event arrives, and
        // `Menu::get` cannot read it back -- it only searches top-level items,
        // so nothing nested in a submenu is ever found. (`nested_check_item`
        // above walks the submenus by hand and does reach them, but only the
        // link grant needs that, because it is the one setting that changes
        // without a click.) Toggling the stored value keeps the two in step,
        // because the menu is built from this same state at startup.
        "pref-autostart" => {
            let enabling = !crate::features::autostart::is_enabled(app);
            crate::features::autostart::set(app, enabling);
        }
        "check-updates" => crate::features::updates::check_now(app),

        "pref-check-updates" => {
            let prefs = config::update_and_save(app, |p| p.check_updates = !p.check_updates);
            log::info!(
                "updates: automatic checks {}",
                if prefs.check_updates { "on" } else { "off" }
            );
        }
        id if id == crate::features::external_links::MENU_ID => {
            crate::features::external_links::toggle_in_app(app)
        }

        "pref-start-hidden" => {
            config::update_and_save(app, |p| p.start_hidden = !p.start_hidden);
        }

        "copy-url" => {
            use tauri_plugin_clipboard_manager::ClipboardExt;
            if let Ok(url) = window.url() {
                if let Err(e) = app.clipboard().write_text(url.to_string()) {
                    log::error!("failed to copy url: {e}");
                }
            }
        }
        "show-logs" => match app.path().app_log_dir() {
            Ok(dir) => {
                if let Err(e) = tauri_plugin_opener::reveal_item_in_dir(&dir) {
                    log::error!("failed to reveal {}: {e}", crate::redact::path(&dir));
                }
            }
            Err(e) => log::error!("no log directory: {e}"),
        },

        "reset-app" => crate::features::reset::request(app),

        "report-issue" => {
            crate::features::external_links::open_in_browser(app, &crate::urls::issue_url());
        }

        #[cfg(debug_assertions)]
        "devtools" => window.open_devtools(),

        _ => {}
    }
}

/// Apply a zoom change, clamp it, and remember it.
///
/// Asking for a level the window is already at does nothing at all. At the
/// clamp -- Ctrl+= held down, or a page calling `menu_action` in a loop -- every
/// press used to cost a webview call and a config write for a value that could
/// not move. `config::update_and_save` declines the write on its own; this
/// declines the webview call, which is the more expensive half.
pub fn set_zoom(app: &AppHandle, f: impl FnOnce(f64) -> f64) {
    let mut moved = false;

    // Computed inside the update so the read and the write of `zoom` are the
    // one locked section, rather than a read, a decision, and a later write.
    let prefs = config::update_and_save(app, |p| {
        // Round to avoid float drift accumulating across many steps.
        let after = (f(p.zoom).clamp(ZOOM_MIN, ZOOM_MAX) * 100.0).round() / 100.0;
        moved = after != p.zoom;
        p.zoom = after;
    });

    if !moved {
        return;
    }

    apply_soon(app, prefs.zoom);
}

/// How long the requests must stop before the webview is asked for a level.
///
/// Short enough to read as immediate on a single press, long enough that a held
/// key or a loop collapses into one relayout.
const ZOOM_SETTLE: Duration = Duration::from_millis(120);

/// The level the webview should end up at, and whether anyone is on the way to
/// deliver it.
static PENDING_ZOOM: Mutex<Option<f64>> = Mutex::new(None);
static DELIVERY_SCHEDULED: AtomicBool = AtomicBool::new(false);

/// Hand the level to the webview once the requests stop.
///
/// Storing a zoom level is free; applying it is a full relayout of Chat's page,
/// and that is the expensive half by a wide margin -- measured at 168 ms for a
/// single step, and degrading to 1495 ms each when the level was moved 200
/// times in a row. Applied synchronously, that let a page hold the main thread
/// -- the one that also draws the window's titlebar buttons -- for four minutes
/// with 200 calls through `menu_action`.
///
/// A rate limit would not have helped: those calls were already 1.2 s apart,
/// because each was waiting for the relayout it had just asked for. The fix is
/// not to slow the requests down but to stop doing the work once per request,
/// so a burst of any size costs one relayout with the level it ended on.
///
/// One thread at a time, not one per request. `set_zoom` off the main thread
/// goes through the event loop, the same route `features::connectivity` uses
/// for `navigate`.
fn apply_soon(app: &AppHandle, zoom: f64) {
    *PENDING_ZOOM.lock().unwrap() = Some(zoom);

    // Somebody is already sleeping on this and will pick up the value above.
    if DELIVERY_SCHEDULED.swap(true, Ordering::SeqCst) {
        return;
    }

    let app = app.clone();
    std::thread::spawn(move || {
        // Leading edge first: a press after a quiet spell is applied at once,
        // so a single Ctrl+= is as immediate as it was before any of this.
        // Only a second press arriving inside the window below waits.
        deliver(&app);

        std::thread::sleep(ZOOM_SETTLE);
        DELIVERY_SCHEDULED.store(false, Ordering::SeqCst);

        // Whatever arrived while that was in flight, at the level it ended on.
        deliver(&app);
    });
}

fn deliver(app: &AppHandle) {
    let Some(zoom) = PENDING_ZOOM.lock().unwrap().take() else {
        return;
    };
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.set_zoom(zoom);
    }
}
