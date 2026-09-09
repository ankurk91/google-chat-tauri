//! Tell the user when a newer release exists. Nothing more.
//!
//! Not Tauri's updater plugin: that signs an artifact, downloads it and swaps
//! it in, which needs a signing key in CI and on Linux only ever works for an
//! AppImage -- never the deb most people here install. This asks GitHub what
//! the latest release is, compares the tag with our own version, and if it is
//! newer offers to open the release page in the browser. The user downloads and
//! installs it themselves, exactly as they did the first time.
//!
//! The schedule is one thread that sleeps: once shortly after launch, then
//! every twelve hours. A sleeping thread costs nothing, and there is only ever
//! one of them -- `start` is called once, from `setup`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Deserialize;
use tauri::{AppHandle, Manager};

use crate::config::{self, Config};

/// Twice a day. GitHub allows sixty unauthenticated calls an hour from one
/// address; two a day is not worth a token.
const INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);

/// Long enough for the window, the tray and the page to be up. The check is not
/// what the first seconds of a launch are for.
const STARTUP_DELAY: Duration = Duration::from_secs(30);

const TIMEOUT: Duration = Duration::from_secs(15);

/// GitHub rejects a request without one, and it is the polite thing to send.
const USER_AGENT: &str = concat!("google-chat-tauri/", env!("CARGO_PKG_VERSION"));

/// Guards against a second scheduler if `start` is ever called twice.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// The parts of a GitHub release this app has any use for.
#[derive(Debug, Deserialize)]
struct Release {
    /// The version, as tagged: `v1.2.3`.
    tag_name: String,
    /// Where to send someone who wants the download.
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// What a check found. `Current` and `Available` are both successes.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Current,
    Available {
        version: String,
        url: String,
    },
    /// The repository has no releases yet, or none that are not drafts.
    NoReleases,
    Failed(String),
}

/// Start the twice-daily schedule. Returns immediately.
pub fn start(app: &AppHandle) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        log::warn!("updates: scheduler already running");
        return;
    }

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(STARTUP_DELAY);

        loop {
            if app.state::<Config>().get().check_updates {
                automatic_check(&app);
            } else {
                log::debug!("updates: automatic checks are switched off");
            }

            std::thread::sleep(INTERVAL);
        }
    });
}

/// The scheduled check: silent unless there is something new, and silent about
/// a version it has already offered. Nobody wants the same dialog twice a day
/// until they get round to updating.
fn automatic_check(app: &AppHandle) {
    match check() {
        Outcome::Available { version, url } => {
            let already_offered = app.state::<Config>().get().offered_version == version;
            if already_offered {
                log::info!("updates: {version} is available; already offered, staying quiet");
                return;
            }

            config::update_and_save(app, |prefs| prefs.offered_version = version.clone());

            offer(app, &version, &url);
        }
        outcome => log::info!("updates: {outcome:?}"),
    }
}

/// Help > Check for Updates. Says something whatever the answer is -- a manual
/// check that appears to do nothing is indistinguishable from a broken one.
pub fn check_now(app: &AppHandle) {
    let app = app.clone();

    // Off the calling thread: this is a menu handler, and the menu is on the
    // thread that would otherwise be drawing the window.
    std::thread::spawn(move || {
        let outcome = check();
        // Logged, because a manual check that only opens a dialog leaves no
        // trace of what it decided -- and "I clicked it and nothing happened"
        // is not something a log should be silent about.
        log::info!("updates: checked on request -> {outcome:?}");

        match outcome {
            Outcome::Available { version, url } => {
                config::update_and_save(&app, |prefs| prefs.offered_version = version.clone());

                offer(&app, &version, &url);
            }
            Outcome::Current => {
                tell(
                    &app,
                    "No updates",
                    &format!("You have the latest version ({}).", current()),
                );
            }
            Outcome::NoReleases => {
                tell(&app, "No updates", "There are no published releases yet.");
            }
            Outcome::Failed(why) => {
                log::warn!("updates: check failed: {why}");
                tell(
                    &app,
                    "Could not check for updates",
                    "GitHub could not be reached. Check your connection and try again.",
                );
            }
        }
    });
}

/// Ask GitHub for the latest release and compare it with our own version.
///
/// Blocking, so never call this on the main thread.
fn check() -> Outcome {
    let url = crate::urls::releases_api();
    log::debug!("updates: asking {}", crate::redact::url_str(&url));

    // The provider has to be named, not just compiled in: ureq defaults to
    // Rustls and *panics* mid-request when the default is not the feature that
    // was built. Measured -- "provider is Rustls but feature is not enabled".
    let agent = ureq::Agent::config_builder()
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .build(),
        )
        .timeout_global(Some(TIMEOUT))
        .build()
        .new_agent();

    let response = agent
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call();

    let mut response = match response {
        Ok(response) => response,
        // A private repository and a typo in the URL are both 404 here, and
        // neither is worth a dialog full of jargon.
        Err(ureq::Error::StatusCode(404)) => return Outcome::NoReleases,
        Err(e) => return Outcome::Failed(e.to_string()),
    };

    let releases: Vec<Release> = match response.body_mut().read_json() {
        Ok(releases) => releases,
        Err(e) => return Outcome::Failed(format!("unreadable releases: {e}")),
    };

    let running = match semver::Version::parse(normalise(current())) {
        Ok(running) => running,
        Err(e) => return Outcome::Failed(format!("unreadable own version: {e}")),
    };

    match newest(&releases) {
        Some((release, version)) if version > running => Outcome::Available {
            version: version.to_string(),
            url: release.html_url.clone(),
        },
        Some(_) => Outcome::Current,
        None => Outcome::NoReleases,
    }
}

/// The highest version worth offering, out of what GitHub returned.
///
/// GitHub lists releases newest-first by creation, which is usually the same
/// order as by version and does not have to be -- a patch to an older line gets
/// published after a newer release. Comparing versions is the answer to the
/// question actually being asked.
///
/// Drafts and pre-releases are never offered, whatever is running. A beta
/// reaches people who go looking for it on the releases page, not through
/// this check.
fn newest(releases: &[Release]) -> Option<(&Release, semver::Version)> {
    releases
        .iter()
        .filter(|release| !release.draft)
        .filter(|release| !release.prerelease)
        .filter_map(|release| {
            semver::Version::parse(normalise(&release.tag_name))
                .ok()
                .map(|version| (release, version))
        })
        .max_by(|left, right| left.1.cmp(&right.1))
}

/// Release tags carry a leading `v`; `CARGO_PKG_VERSION` does not.
fn normalise(version: &str) -> &str {
    version.trim().trim_start_matches('v')
}

fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The offer itself: a dialog with somewhere to go.
fn offer(app: &AppHandle, version: &str, url: &str) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

    log::info!("updates: {version} is available at {url}");

    let app = app.clone();
    let url = url.to_string();

    app.clone()
        .dialog()
        .message(format!(
            "Google Chat {version} is available. You have {}.\n\nThe download page \
             opens in your browser; install it the same way you installed this one.",
            current()
        ))
        .title("Update available")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Download".into(),
            "Not now".into(),
        ))
        .show(move |download| {
            if download {
                crate::features::external_links::open_in_browser(&app, &url);
            }
        });
}

fn tell(app: &AppHandle, title: &str, message: &str) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

    app.dialog()
        .message(message)
        .title(title)
        .buttons(MessageDialogButtons::Ok)
        .show(|_| {});
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leading_v_on_a_tag_is_not_part_of_the_version() {
        // Tags carry it, CARGO_PKG_VERSION does not, and both forms have to
        // mean the same version or every check reports an update.
        assert_eq!(normalise("v1.0.0"), "1.0.0");
        assert_eq!(normalise(" 1.0.0 "), "1.0.0");

        let releases = [release("v1.0.0", false, false)];
        assert_eq!(newest(&releases).unwrap().1, version("1.0.0"));
    }

    #[test]
    fn a_prerelease_sorts_below_the_release_it_precedes() {
        // semver's own rule, and the one the version comparison in check()
        // relies on: 1.1.0-beta.1 is not an upgrade from 1.1.0.
        assert!(version("1.1.0-beta.1") < version("1.1.0"));
    }

    #[test]
    fn our_own_version_parses() {
        // If this fails, every check reports Failed and nobody finds out until
        // a release exists.
        assert!(semver::Version::parse(current()).is_ok(), "{}", current());
    }

    fn release(tag: &str, draft: bool, prerelease: bool) -> Release {
        Release {
            tag_name: tag.into(),
            html_url: format!("https://example.test/tag/{tag}"),
            draft,
            prerelease,
        }
    }

    fn version(v: &str) -> semver::Version {
        semver::Version::parse(v).unwrap()
    }

    #[test]
    fn a_prerelease_is_not_offered_to_someone_on_a_pre_1_0_build() {
        // 0.x used to be carved out, because every 0.x release was itself
        // tagged pre-release. Nothing is carved out now: a release marked
        // pre-release is not an update for anybody.
        let releases = [release("v0.0.2", false, true)];
        assert!(newest(&releases).is_none());
    }

    #[test]
    fn a_prerelease_is_not_offered_to_someone_on_a_stable_build() {
        let releases = [release("v1.1.0-beta.1", false, true)];
        assert!(newest(&releases).is_none());
    }

    #[test]
    fn someone_already_on_a_beta_is_not_offered_the_next_one() {
        // Running a beta no longer opts you in. A beta tester hears about
        // stable releases and finds the next beta himself.
        let releases = [release("v1.1.0-beta.2", false, true)];
        assert!(newest(&releases).is_none());

        let releases = [release("v1.1.0", false, false)];
        assert_eq!(newest(&releases).unwrap().1, version("1.1.0"));
    }

    #[test]
    fn drafts_are_never_offered() {
        let releases = [release("v9.9.9", true, false)];
        assert!(newest(&releases).is_none());
    }

    #[test]
    fn the_highest_version_wins_not_the_first_listed() {
        // GitHub lists by creation date. A patch to an older line published
        // after a newer release would otherwise win.
        let releases = [
            release("v0.9.1", false, false),
            release("v1.2.0", false, false),
            release("v1.0.5", false, false),
        ];
        let picked = newest(&releases).unwrap();
        assert_eq!(picked.1, version("1.2.0"));
    }

    #[test]
    fn a_tag_that_is_not_a_version_is_skipped_not_fatal() {
        let releases = [
            release("nightly", false, false),
            release("v0.0.2", false, false),
        ];
        let picked = newest(&releases).unwrap();
        assert_eq!(picked.1, version("0.0.2"));
    }

    #[test]
    fn no_releases_at_all_is_not_an_error() {
        assert!(newest(&[]).is_none());
    }

    #[test]
    fn the_live_payload_for_v0_0_1_is_read_correctly() {
        // Trimmed from what api.github.com actually returned for this
        // repository on 2026-09-06, pre-release and all.
        let body = r#"[{
            "html_url": "https://github.com/ankurk91/google-chat-tauri/releases/tag/v0.0.1",
            "tag_name": "v0.0.1",
            "name": "Google Chat v0.0.1",
            "draft": false,
            "prerelease": true,
            "assets": [
                {"name": "google-chat-tauri_0.0.1_linux-amd64.deb"},
                {"name": "google-chat-tauri_0.0.1_linux-amd64.AppImage"},
                {"name": "google-chat-tauri_0.0.1_darwin-universal.dmg"},
                {"name": "google-chat-tauri_0.0.1_darwin-universal.app"},
                {"name": "google-chat-tauri_0.0.1_windows-x64_setup.exe"}
            ]
        }]"#;

        let releases: Vec<Release> = serde_json::from_str(body).unwrap();
        assert_eq!(releases.len(), 1);
        assert!(releases[0].prerelease);

        // It parses, and being flagged pre-release it is offered to nobody.
        assert!(newest(&releases).is_none());
    }

    #[test]
    fn a_release_payload_is_read_the_way_github_sends_it() {
        // Trimmed from the documented shape of GET /repos/{o}/{r}/releases/latest.
        let body = r#"{
            "url": "https://api.github.com/repos/ankurk91/google-chat-tauri/releases/1",
            "html_url": "https://github.com/ankurk91/google-chat-tauri/releases/tag/v1.1.0",
            "tag_name": "v1.1.0",
            "name": "Google Chat v1.1.0",
            "draft": false,
            "prerelease": false,
            "published_at": "2026-09-06T10:00:00Z",
            "assets": [{"name": "google-chat-tauri_1.1.0_linux-amd64.deb", "size": 3014656}]
        }"#;

        let release: Release = serde_json::from_str(body).unwrap();
        assert_eq!(release.tag_name, "v1.1.0");
        assert!(release.html_url.ends_with("/tag/v1.1.0"));
        assert!(!release.draft && !release.prerelease);

        let picked = newest(std::slice::from_ref(&release)).unwrap();
        assert!(picked.1 > version("1.0.0"));
    }

    #[test]
    fn draft_and_prerelease_default_to_false_when_absent() {
        let release: Release =
            serde_json::from_str(r#"{"tag_name":"v2.0.0","html_url":"https://example.test"}"#)
                .unwrap();
        assert!(!release.draft && !release.prerelease);
    }
}
