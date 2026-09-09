//! Tray / badge artwork, compiled into the binary.
//!
//! Regenerate with `python3 scripts/gen-icons.py` (the PNGs are committed).

use tauri::image::Image;

macro_rules! tray_png {
    ($name:literal) => {
        include_bytes!(concat!("../icons/tray/", $name, ".png"))
    };
}

pub const NORMAL_16: &[u8] = tray_png!("normal-16");
pub const NORMAL_32: &[u8] = tray_png!("normal-32");
// Google's own "unread" variant of the Chat mark: the logo with a red dot.
pub const BADGE_16: &[u8] = tray_png!("badge-16");
pub const BADGE_32: &[u8] = tray_png!("badge-32");
pub const OFFLINE_16: &[u8] = tray_png!("offline-16");
pub const OFFLINE_32: &[u8] = tray_png!("offline-32");

/// Windows taskbar overlay icons.
#[cfg(target_os = "windows")]
const COUNT_16: [&[u8]; 10] = [
    tray_png!("count-16/1"),
    tray_png!("count-16/2"),
    tray_png!("count-16/3"),
    tray_png!("count-16/4"),
    tray_png!("count-16/5"),
    tray_png!("count-16/6"),
    tray_png!("count-16/7"),
    tray_png!("count-16/8"),
    tray_png!("count-16/9"),
    tray_png!("count-16/9plus"),
];

/// Index into the `count-16` table for `count >= 1`. Anything above 9 shows "9+".
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn count_index(count: i64) -> usize {
    (count.clamp(1, 10) as usize) - 1
}

#[cfg(target_os = "windows")]
pub fn count_16(count: i64) -> &'static [u8] {
    COUNT_16[count_index(count)]
}

/// The app icon, for the About dialog. Not from `tray/` -- that artwork is
/// sized for a 16-32px tray slot.
pub const APP: &[u8] = include_bytes!("../icons/128x128.png");

pub fn decode(bytes: &'static [u8]) -> tauri::Result<Image<'static>> {
    Image::from_bytes(bytes)
}

/// macOS menu-bar icons are 16px; every other tray wants 32px. Same split the
/// electron app used.
const SMALL: bool = cfg!(target_os = "macos");

/// The icon shown before `chat.js` has reported in.
pub fn initial() -> &'static [u8] {
    if SMALL {
        OFFLINE_16
    } else {
        OFFLINE_32
    }
}

/// Tray artwork for the current state.
///
/// The tray signals *whether* there is anything unread, not how many: the count
/// itself lives in the window title, and in the dock badge (macOS) or taskbar
/// overlay (Windows). A digit rendered into a 16-32px tray icon is hard to read
/// and duplicates what the title already says.
pub fn tray(connected: bool, has_unread: bool) -> &'static [u8] {
    if !connected {
        return initial();
    }

    match (has_unread, SMALL) {
        (true, true) => BADGE_16,
        (true, false) => BADGE_32,
        (false, true) => NORMAL_16,
        (false, false) => NORMAL_32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_icon_decodes() {
        for bytes in [
            NORMAL_16, NORMAL_32, BADGE_16, BADGE_32, OFFLINE_16, OFFLINE_32,
        ] {
            assert!(decode(bytes).is_ok());
        }
    }

    #[test]
    fn counts_above_nine_collapse_to_the_9plus_icon() {
        assert_eq!(count_index(9), 8);
        assert_eq!(count_index(10), 9);
        assert_eq!(count_index(999), 9);
        // Clamped, not panicking, if the page ever reports something silly.
        assert_eq!(count_index(0), 0);
    }

    #[test]
    fn tray_shows_muted_icon_until_the_page_reports_in() {
        assert_eq!(tray(false, false), initial());
        assert_eq!(tray(false, true), initial());
        assert_ne!(tray(true, false), initial());
    }

    #[test]
    fn tray_switches_to_the_dot_variant_when_unread() {
        assert_eq!(tray(true, false), if SMALL { NORMAL_16 } else { NORMAL_32 });
        assert_eq!(tray(true, true), if SMALL { BADGE_16 } else { BADGE_32 });
    }
}
