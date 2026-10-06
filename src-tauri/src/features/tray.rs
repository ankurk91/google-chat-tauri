//! Ported from electron `src/main/features/trayIcon.ts`.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::features::window;
use crate::icons;
use crate::state::AppState;

pub const ID: &str = "main-tray";

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "Toggle", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let mut items: Vec<&dyn tauri::menu::IsMenuItem<_>> = vec![&toggle];

    // Exercise the badge and notification paths without waiting for a real
    // message. Mirrors electron's "Demo Badge Count" troubleshooting item.
    #[cfg(debug_assertions)]
    let demo = MenuItem::with_id(app, "demo-badge", "Demo Badge Count", true, None::<&str>)?;
    #[cfg(debug_assertions)]
    let test_notify = MenuItem::with_id(
        app,
        "test-notification",
        "Test Notification",
        true,
        None::<&str>,
    )?;
    #[cfg(debug_assertions)]
    {
        items.push(&demo);
        items.push(&test_notify);
    }

    let separator = PredefinedMenuItem::separator(app)?;
    items.push(&separator);

    // ksni renders predefined items other than separators as disabled blanks,
    // so on Linux About is a regular item -- see `show_about_dialog`.
    #[cfg(target_os = "linux")]
    let about = MenuItem::with_id(app, "about", "About", true, None::<&str>)?;
    #[cfg(not(target_os = "linux"))]
    let about = PredefinedMenuItem::about(
        app,
        Some("About"),
        Some(crate::features::app_menu::about_metadata()),
    )?;
    items.push(&about);
    items.push(&quit);

    let menu = Menu::with_items(app, &items)?;

    TrayIconBuilder::with_id(ID)
        .icon(icons::decode(icons::initial())?)
        .tooltip("Google Chat")
        .menu(&menu)
        // Linux clicks need the ksni backend -- see the Cargo.toml note.
        // macOS keeps the menu-on-left-click convention.
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => toggle_window(app),
            "about" => show_about_dialog(app),
            "demo-badge" => {
                // Cheap pseudo-random: good enough to eyeball the icons.
                let n = (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
                    % 12) as i64;
                app.state::<AppState>().set_unread(n, n > 0);
                crate::features::badge::apply(app);
            }
            "test-notification" => {
                crate::features::notifications::show(
                    app,
                    0,
                    "Test Notification",
                    Some("If you can see this, the notification path works."),
                );
            }
            "quit" => {
                // The page can block a graceful quit via onbeforeunload, so mark
                // our intent first and exit rather than asking the window nicely.
                app.state::<AppState>().set_quitting();
                crate::config::flush(app);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // macOS opens the menu on left click; Windows and Linux toggle.
            if cfg!(target_os = "macos") {
                return;
            }
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

fn toggle_window(app: &AppHandle) {
    let Some(win) = app.get_webview_window(window::MAIN) else {
        return;
    };

    let visible = win.is_visible().unwrap_or(false);
    let focused = win.is_focused().unwrap_or(false);

    // Electron used a different predicate on Windows because a click on the tray
    // steals focus from the window before the handler runs.
    let should_hide = if cfg!(target_os = "windows") {
        visible || focused
    } else {
        visible && focused
    };
    if should_hide {
        #[cfg(target_os = "macos")]
        let _ = app.hide();
        #[cfg(not(target_os = "macos"))]
        let _ = win.hide();
    } else {
        window::show_and_focus(app);
    }
}

/// The tray's About dialog, Linux-only: the ksni menu snapshot renders the
/// predefined About as a disabled blank. The window menu keeps muda's full one.
fn show_about_dialog(app: &AppHandle) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

    let text = format!(
        "Google Chat v{}\n{}\n\n{} · GPL-3.0-only\n{}",
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_DESCRIPTION"),
        env!("CARGO_PKG_AUTHORS"),
        env!("CARGO_PKG_REPOSITORY"),
    );

    app.dialog()
        .message(text)
        .title("About Google Chat")
        .kind(MessageDialogKind::Info)
        .show(|_| {});
}
