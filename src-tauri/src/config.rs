//! User preferences, at `app_config_dir()/config.json`.
//!
//! Replaces electron's `electron-store` (`src/main/config.ts`). Deliberately a
//! plain serde struct rather than tauri-plugin-store: nothing in the webview
//! needs to read or write these, so a plugin and its ACL surface would buy
//! nothing.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

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
    /// Ask GitHub twice a day whether there is a newer release.
    pub check_updates: bool,
    /// The last version the user was told about, so an automatic check does not
    /// raise the same dialog every twelve hours until they get round to it. The
    /// manual check ignores this.
    pub offered_version: String,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            start_hidden: false,
            check_updates: true,
            offered_version: String::new(),
        }
    }
}

/// Loaded once at startup and kept in Tauri's state.
#[derive(Default)]
pub struct Config(Mutex<Inner>);

/// The most often the file is allowed to be written, however fast the
/// preferences change. A burst is held in memory and lands as one write.
///
/// The cost is a window in which a change is only in memory: kill the process
/// inside it and that change is gone. Accepted deliberately -- the settings
/// this covers are cheap to redo, and the quit paths call [`flush`] anyway, so
/// the window is really only open for a crash or a `kill -9`.
const WRITE_EVERY: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Inner {
    prefs: Prefs,
    /// The JSON last written to the file, so an update that leaves the
    /// serialised form untouched does not write it again. `None` until this
    /// process has written once -- `load` does not fill it in, so the first
    /// save of a session always goes through.
    written: Option<String>,
    /// When that write happened, for the throttle above.
    last_write: Option<Instant>,
    /// Whether a thread is already waiting to write what is in memory, so a
    /// burst schedules one writer rather than one per change.
    flush_scheduled: bool,
}

impl Config {
    pub fn get(&self) -> Prefs {
        self.0.lock().unwrap().prefs.clone()
    }

    /// Change the preferences in memory and nowhere else.
    ///
    /// Startup uses this to install what `load` read. Everything after that
    /// wants `update_and_save`, which cannot lose the change it just made.
    pub fn update(&self, f: impl FnOnce(&mut Prefs)) -> Prefs {
        let mut g = self.0.lock().unwrap();
        f(&mut g.prefs);
        g.prefs.clone()
    }
}

/// Change the preferences and write them out, both under the one lock.
///
/// Update-then-save as two steps had two ways to go wrong, and both need only
/// the two threads this app already has: the menu changes zoom and the toggles,
/// while `features::updates` writes `offered_version` from its own. Interleave
/// the updates and the second save writes a snapshot taken before the first
/// one's change, silently dropping it. Interleave the writes and a truncating
/// `fs::write` is partway through one file with another already going in.
///
/// Holding the lock across the write closes both: what reaches the file is what
/// was just set, and only one write is ever in flight.
///
/// A change that serialises to what is already on disk writes nothing.
/// Measured before that was true: 200 page-driven zoom-ins against a zoom
/// already at the clamp wrote this file 200 times, byte for byte identical, at
/// around 180 writes a second -- on the thread that also draws the window's own
/// titlebar buttons. The page can reach `set_zoom` through `menu_action`, so
/// there was no ceiling on it.
///
/// On top of that the preferences are kept in memory and reach the file at most
/// once every [`WRITE_EVERY`]; a change inside that window schedules one writer
/// to carry whatever the value has settled on by then.
pub fn update_and_save(app: &AppHandle, f: impl FnOnce(&mut Prefs)) -> Prefs {
    let config = app.state::<Config>();
    let mut guard = config.0.lock().unwrap();

    f(&mut guard.prefs);
    let prefs = guard.prefs.clone();

    let Some(since) = guard.last_write.map(|at| at.elapsed()) else {
        // Nothing written yet this session, so there is nothing to throttle.
        flush_locked(app, &mut guard);
        return prefs;
    };

    if since >= WRITE_EVERY {
        flush_locked(app, &mut guard);
    } else if !guard.flush_scheduled {
        guard.flush_scheduled = true;

        let wait = WRITE_EVERY - since;
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(wait);

            let config = app.state::<Config>();
            let mut guard = config.0.lock().unwrap();
            guard.flush_scheduled = false;
            flush_locked(&app, &mut guard);
        });
    }

    prefs
}

/// Write what is in memory, if it is not already on disk. Caller holds the lock.
fn flush_locked(app: &AppHandle, inner: &mut Inner) {
    let json = match serde_json::to_string_pretty(&inner.prefs) {
        Ok(json) => json,
        Err(e) => {
            log::error!("config: failed to serialise: {e}");
            return;
        }
    };

    if inner.written.as_deref() == Some(json.as_str()) {
        return;
    }

    // Only remembered once it is really on disk, so a failed write is attempted
    // again rather than assumed.
    if write(app, &json) {
        inner.written = Some(json);
        inner.last_write = Some(Instant::now());
    }
}

/// Write anything the throttle is still holding.
///
/// Called on the way out, so quitting never costs the setting the user changed
/// a moment before.
pub fn flush(app: &AppHandle) {
    let config = app.state::<Config>();
    let mut guard = config.0.lock().unwrap();
    flush_locked(app, &mut guard);
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("config.json"))
}

pub fn load(app: &AppHandle) -> Prefs {
    let Some(p) = path(app) else {
        return Prefs::default();
    };

    match std::fs::read_to_string(&p) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
            // A corrupt file should not stop the app from starting; electron's
            // store did the same via `clearInvalidConfig`.
            log::error!(
                "config: ignoring unreadable {}: {e}",
                crate::redact::path(&p)
            );
            Prefs::default()
        }),
        Err(_) => Prefs::default(),
    }
}

/// Only ever called from `update_and_save`, which holds the lock.
///
/// Returns whether the file now holds `json`.
fn write(app: &AppHandle, json: &str) -> bool {
    let Some(p) = path(app) else { return false };

    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }

    match write_to(&p, json) {
        Ok(()) => true,
        Err(e) => {
            log::error!("config: failed to write {}: {e}", crate::redact::path(&p));
            false
        }
    }
}

/// Write beside the real file, then rename over it.
///
/// `fs::write` truncates before it writes, so a crash, a full disk or a killed
/// process between the two leaves an empty config where the settings were. That
/// is survivable -- `load` falls back to defaults -- but the user's preferences
/// are gone, which is not the sort of thing that should follow from bad timing.
/// A rename is atomic on all three platforms (Windows included: `fs::rename`
/// asks for `MOVEFILE_REPLACE_EXISTING`), so the file is either what it was or
/// what it is becoming, never half of either.
fn write_to(path: &Path, json: &str) -> std::io::Result<()> {
    let scratch = path.with_extension("json.tmp");

    let written = File::create(&scratch).and_then(|mut file| {
        file.write_all(json.as_bytes())?;
        // The rename orders the directory entry, not the bytes behind it.
        // Without this, a power loss can leave the new name pointing at a file
        // whose contents never arrived -- the empty config all over again.
        file.sync_all()
    });

    if let Err(e) = written {
        let _ = std::fs::remove_file(&scratch);
        return Err(e);
    }

    if let Err(e) = std::fs::rename(&scratch, path) {
        let _ = std::fs::remove_file(&scratch);
        return Err(e);
    }

    Ok(())
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

    #[test]
    fn a_save_lands_whole_and_leaves_no_scratch_file() {
        let dir = std::env::temp_dir().join(format!("gchat-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        let scratch = path.with_extension("json.tmp");

        let written = |prefs: &Prefs| {
            write_to(&path, &serde_json::to_string_pretty(prefs).unwrap()).unwrap();
            let json = std::fs::read_to_string(&path).unwrap();
            serde_json::from_str::<Prefs>(&json).unwrap()
        };

        let first = written(&Prefs {
            zoom: 1.5,
            ..Default::default()
        });
        assert_eq!(first.zoom, 1.5);
        assert!(!scratch.exists(), "scratch file left behind");

        // And again over the top: the rename has to replace, not refuse.
        let second = written(&Prefs {
            zoom: 0.8,
            start_hidden: true,
            ..Default::default()
        });
        assert_eq!(second.zoom, 0.8);
        assert!(second.start_hidden);
        assert!(!scratch.exists(), "scratch file left behind");

        std::fs::remove_dir_all(&dir).ok();
    }
}
