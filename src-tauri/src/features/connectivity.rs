//! Is there a network at all, at startup?
//!
//! The window points straight at a remote page, so with no route to the
//! internet the app shows whatever the webview shows for a failed load -- which
//! on WebKitGTK is a bare error page with no indication that this is an app
//! that will work again later. Saying so once, in the desktop's own
//! notification, costs one TCP connection.
//!
//! Deliberately not a monitor. Launching at login means racing the network:
//! NetworkManager may still be associating when the session starts, so the
//! probe retries across the first minute. After that it stops. Nothing here
//! reloads the page or watches for the network coming back -- the webview does
//! its own thing, and a background poller that never stops is a poor trade for
//! a message the user only needs once.

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use tauri::AppHandle;

/// Where to knock. Chat itself, so this answers the question that matters --
/// can we reach the thing the window is about -- rather than whether some
/// unrelated captive-portal endpoint is up.
const PROBE_HOST: &str = "chat.google.com:443";

/// How long each attempt may take. Short: a DNS lookup and a handshake-less
/// connect on a working network are well under a second, and a hung one should
/// not hold the retry schedule up.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Waits between attempts, in seconds. About a minute of waiting, front-loaded:
/// a machine that is already online answers the first attempt, and a laptop
/// joining wifi usually manages within the last of them. Measured end to end
/// against an address that never answers: 84s, because every failed attempt
/// also spends its connect timeout.
const BACKOFF_SECS: [u64; 5] = [2, 4, 8, 15, 30];

/// Where to knock instead, for testing. Debug builds only: an environment
/// variable that can silently make the app think it is offline has no business
/// in a release.
///
/// ```text
/// GOOGLE_CHAT_PROBE_HOST=192.0.2.1:443 ./google-chat-tauri   # TEST-NET-1, never routes
/// ```
#[cfg(debug_assertions)]
fn probe_host() -> String {
    std::env::var("GOOGLE_CHAT_PROBE_HOST").unwrap_or_else(|_| PROBE_HOST.to_string())
}

#[cfg(not(debug_assertions))]
fn probe_host() -> String {
    PROBE_HOST.to_string()
}

/// A TCP connection to the host the app is about.
///
/// Not an HTTP request: no TLS, no response to parse, nothing that a captive
/// portal or a proxy can answer misleadingly. DNS plus a completed handshake is
/// what "the network is up" means here.
pub fn is_online() -> bool {
    let host = probe_host();
    let addresses = match host.to_socket_addrs() {
        Ok(addresses) => addresses,
        // DNS failure is itself the answer.
        Err(e) => {
            log::debug!("connectivity: cannot resolve {host}: {e}");
            return false;
        }
    };

    for address in addresses {
        if TcpStream::connect_timeout(&address, PROBE_TIMEOUT).is_ok() {
            return true;
        }
    }

    false
}

/// Probe across the first minute, and tell the user once if nothing answers.
///
/// Returns immediately; the waiting happens on its own thread, because this is
/// called from `setup` and a minute of sleeping there would be a minute of no
/// window.
pub fn check_at_startup(app: &AppHandle) {
    let app = app.clone();

    std::thread::spawn(move || {
        let started = std::time::Instant::now();

        if is_online() {
            log::info!("connectivity: online");
            return;
        }

        for (attempt, wait) in BACKOFF_SECS.iter().enumerate() {
            log::debug!(
                "connectivity: offline, retrying in {wait}s ({}/{})",
                attempt + 1,
                BACKOFF_SECS.len()
            );
            std::thread::sleep(Duration::from_secs(*wait));

            if is_online() {
                log::info!(
                    "connectivity: online after {} attempt(s)",
                    attempt + 2 // the first attempt was before the loop
                );
                return;
            }
        }

        // Elapsed, not the sum of the waits: each failed attempt also spends
        // up to PROBE_TIMEOUT before it gives up, so the real span is longer
        // than the schedule suggests.
        log::warn!(
            "connectivity: no network after {}s; telling the user",
            started.elapsed().as_secs()
        );
        crate::features::notifications::show(
            &app,
            0,
            "No internet connection",
            Some("Google Chat could not be reached. It will load once you are back online."),
        );
    });
}

/// The schedule's own total, for the test that keeps it honest.
#[cfg(test)]
fn total_wait() -> u64 {
    BACKOFF_SECS.iter().sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_retry_schedule_covers_about_a_minute() {
        // Long enough for wifi to associate after a login, short enough that
        // the answer still means something when it arrives.
        let total = total_wait();
        assert!((45..=90).contains(&total), "retries span {total}s");
    }

    #[test]
    fn backoff_never_shrinks() {
        assert!(
            BACKOFF_SECS.windows(2).all(|pair| pair[0] <= pair[1]),
            "waits must not get shorter: {BACKOFF_SECS:?}"
        );
    }

    #[test]
    fn the_probe_target_carries_a_port() {
        // to_socket_addrs on a bare host silently fails; the port is not
        // optional and is easy to lose in an edit.
        assert!(PROBE_HOST.contains(':'), "{PROBE_HOST} has no port");
    }
}
