//! User preferences, at `app_config_dir()/config.json`.
//!
//! Replaces electron's `electron-store` (`src/main/config.ts`). Deliberately a
//! plain serde struct rather than tauri-plugin-store: nothing in the webview
//! needs to read or write these, so a plugin and its ACL surface would buy
//! nothing.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

pub const ZOOM_MIN: f64 = 0.5;
pub const ZOOM_MAX: f64 = 3.0;
pub const ZOOM_STEP: f64 = 0.1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub zoom: f64,
    pub start_hidden: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            start_hidden: false,
        }
    }
}

/// Loaded once at startup and kept in Tauri's state.
#[derive(Default)]
pub struct Config(Mutex<Prefs>);

impl Config {
    pub fn update(&self, f: impl FnOnce(&mut Prefs)) -> Prefs {
        let mut g = self.0.lock().unwrap();
        f(&mut g);
        g.clone()
    }
}

fn path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("config.json"))
}

pub fn load(app: &AppHandle) -> Prefs {
    let Some(p) = path(app) else {
        return Prefs::default();
    };

    match std::fs::read_to_string(&p) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
            // A corrupt file should not stop the app from starting; electron's
            // store did the same via `clearInvalidConfig`.
            eprintln!("[config] ignoring unreadable {}: {e}", p.display());
            Prefs::default()
        }),
        Err(_) => Prefs::default(),
    }
}

pub fn save(app: &AppHandle, prefs: &Prefs) {
    let Some(p) = path(app) else { return };

    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }

    match serde_json::to_string_pretty(prefs) {
        Ok(s) => {
            if let Err(e) = std::fs::write(&p, s) {
                eprintln!("[config] failed to write {}: {e}", p.display());
            }
        }
        Err(e) => eprintln!("[config] failed to serialise: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let p: Prefs = serde_json::from_str("{}").unwrap();
        assert_eq!(p.zoom, 1.0);
        assert!(!p.start_hidden);
    }

    #[test]
    fn unknown_fields_do_not_break_loading() {
        // Forward compatibility: an older build must tolerate a newer config.
        let p: Prefs = serde_json::from_str(r#"{"zoom":1.5,"future_setting":true}"#).unwrap();
        assert_eq!(p.zoom, 1.5);
    }
}
