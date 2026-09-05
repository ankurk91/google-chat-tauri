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
    items.push(&quit);

    let menu = Menu::with_items(app, &items)?;

    TrayIconBuilder::with_id(ID)
        .icon(icons::decode(icons::initial())?)
        .tooltip("Google Chat")
        .menu(&menu)
        // Windows gets a real click event and toggles directly. Everywhere else
        // left-click opens the menu, whose first item is Toggle -- Linux tray
        // backends deliver no click events at all, so a menu is the only option.
        .show_menu_on_left_click(!cfg!(target_os = "windows"))
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => toggle_window(app),
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
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Left-click-to-toggle is Windows-only: Linux tray backends
            // (AppIndicator) do not deliver click events at all, and on macOS a
            // left click should open the menu.
            if !cfg!(target_os = "windows") {
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
