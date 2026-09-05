//! The block of facts every log file opens with.
//!
//! A log that starts at `unread: count=0` says nothing about the machine it
//! came from, and almost everything this app works around is specific to that
//! machine: the tray delivering no clicks, the badge needing libunity, which
//! notification daemon is listening, a blank window meaning a graphics driver.
//! A bug report is only actionable with the version, the webview and the
//! session underneath it, so they go in first, before anything can go wrong.

use tauri::{AppHandle, Manager};

use crate::config::Prefs;

pub fn log_startup(app: &AppHandle, prefs: &Prefs, launched_hidden: bool) {
    let info = app.package_info();
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    log::info!(
        "{} {} ({profile}) — {}",
        info.name,
        info.version,
        app.config().identifier
    );

    let mut platform = format!(
        "platform: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    if let Some(detail) = os_detail() {
        platform.push_str(&format!(" — {detail}"));
    }
    log::info!("{platform}");

    // The webview *is* the app here, and its version explains more failures
    // than anything else on this list. wry reports a bare number, which means
    // nothing without the engine it belongs to.
    let engine = if cfg!(target_os = "linux") {
        "WebKitGTK"
    } else if cfg!(target_os = "macos") {
        "WKWebView"
    } else {
        "WebView2"
    };
    match tauri::webview_version() {
        Ok(version) => log::info!("webview: {engine} {version}"),
        Err(e) => log::warn!("webview: {engine} version unavailable: {e}"),
    }

    // Which desktop, and X11 or Wayland: the tray, the badge and the
    // notification behaviour all differ along those two axes.
    #[cfg(target_os = "linux")]
    log::info!(
        "session: {} on {}",
        env_or_unknown("XDG_CURRENT_DESKTOP"),
        env_or_unknown("XDG_SESSION_TYPE")
    );

    if let Ok(dir) = app.path().app_config_dir() {
        log::info!("config dir: {}", dir.display());
    }
    if let Ok(dir) = app.path().app_log_dir() {
        log::info!("log dir: {}", dir.display());
    }

    log::info!(
        "prefs: zoom={} start_hidden={} autostart={} launched_hidden={launched_hidden}",
        prefs.zoom,
        prefs.start_hidden,
        crate::features::autostart::is_enabled(app),
    );
}

/// The distribution and kernel, on the one platform where they vary enough to
/// matter and are free to read.
#[cfg(target_os = "linux")]
fn os_detail() -> Option<String> {
    let mut parts = Vec::new();

    if let Ok(release) = std::fs::read_to_string("/etc/os-release") {
        if let Some(name) = release.lines().find_map(|line| {
            line.strip_prefix("PRETTY_NAME=")
                .map(|v| v.trim_matches('"').to_string())
        }) {
            parts.push(name);
        }
    }

    if let Ok(kernel) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        parts.push(format!("kernel {}", kernel.trim()));
    }

    (!parts.is_empty()).then(|| parts.join(", "))
}

/// Nothing worth the cost of shelling out to `sw_vers` or the registry.
#[cfg(not(target_os = "linux"))]
fn os_detail() -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn env_or_unknown(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "linux")]
    fn os_detail_reports_the_running_system() {
        // Every Linux this runs on has at least a kernel version to report.
        let detail = os_detail().expect("no os detail on a Linux host");
        assert!(detail.contains("kernel"), "unexpected detail: {detail}");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn missing_environment_does_not_panic() {
        assert_eq!(env_or_unknown("GOOGLE_CHAT_NOT_A_REAL_VAR"), "unknown");
    }
}
