use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How many times in a row the window may be pulled off a signed-out landing
/// page. If the sign-in form bounces straight back to the landing page, another
/// hop only loops -- and a redirect loop is worse than the dead end, which the
/// user can click their own way out of. See `features::sign_in`.
const MAX_RESCUES: u32 = 2;

#[derive(Default)]
pub struct AppState {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    unread: i64,
    /// From the favicon, which is the only signal that survives the window
    /// being hidden. May be true while `unread` is 0.
    has_unread: bool,
    /// Set once `chat.js` reports in from the Chat page. Until then the tray
    /// shows the muted "not connected yet" icon.
    connected: bool,
    /// Set when the user picks Quit, so the close handler stops hiding to tray.
    quitting: bool,
    /// Redirects away from a signed-out landing page since the last page that
    /// was part of the app.
    rescues: u32,
    /// When the temporary "open every link in this window" grant runs out.
    /// `None` means links follow the ordinary policy.
    links_in_app_until: Option<Instant>,
    /// Bumped every time that grant is switched on or off. It is what lets the
    /// thread waiting to untick the menu tell whether the grant it started is
    /// still the one in force, or whether the user has since turned it off by
    /// hand and there is nothing left to announce.
    links_epoch: u64,
}

impl AppState {
    pub fn unread(&self) -> i64 {
        self.inner.lock().unwrap().unread
    }

    pub fn has_unread(&self) -> bool {
        let g = self.inner.lock().unwrap();
        g.has_unread || g.unread > 0
    }

    /// Records a report from the page. Returns `true` if anything changed.
    pub fn set_unread(&self, count: i64, has_unread: bool) -> bool {
        let mut g = self.inner.lock().unwrap();
        let changed = g.unread != count || g.has_unread != has_unread || !g.connected;
        g.unread = count;
        g.has_unread = has_unread;
        g.connected = true;
        changed
    }

    pub fn is_connected(&self) -> bool {
        self.inner.lock().unwrap().connected
    }

    pub fn is_quitting(&self) -> bool {
        self.inner.lock().unwrap().quitting
    }

    pub fn set_quitting(&self) {
        self.inner.lock().unwrap().quitting = true;
    }

    /// Claim one redirect away from a signed-out landing page.
    ///
    /// `false` means the window has been sent to the sign-in form twice already
    /// without a real page loading in between, so stop asking.
    pub fn claim_rescue(&self) -> bool {
        let mut g = self.inner.lock().unwrap();
        if g.rescues >= MAX_RESCUES {
            return false;
        }
        g.rescues += 1;
        true
    }

    /// A page belonging to the app loaded, so the run of redirects is over.
    pub fn clear_rescues(&self) {
        self.inner.lock().unwrap().rescues = 0;
    }

    /// Is every link currently allowed to open in this window?
    ///
    /// Answers from the clock rather than from a timer having fired, so an
    /// expired grant is expired even if the thread that was going to clear it
    /// never ran.
    pub fn links_open_in_app(&self) -> bool {
        let mut g = self.inner.lock().unwrap();
        match g.links_in_app_until {
            Some(deadline) if Instant::now() < deadline => true,
            Some(_) => {
                g.links_in_app_until = None;
                false
            }
            None => false,
        }
    }

    /// Turn the grant on for `grant`, or off.
    ///
    /// Returns the epoch that identifies this switch, so whoever schedules the
    /// clean-up can ask later whether it is still theirs to do.
    pub fn set_links_in_app(&self, on: bool, grant: Duration) -> u64 {
        let mut g = self.inner.lock().unwrap();
        g.links_in_app_until = if on {
            Some(Instant::now() + grant)
        } else {
            None
        };
        g.links_epoch += 1;
        g.links_epoch
    }

    /// Has the grant been switched since `epoch`?
    pub fn links_epoch_holds(&self, epoch: u64) -> bool {
        self.inner.lock().unwrap().links_epoch == epoch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rescues_stop_before_they_become_a_loop() {
        let state = AppState::default();
        for attempt in 1..=MAX_RESCUES {
            assert!(state.claim_rescue(), "attempt {attempt} should be allowed");
        }
        assert!(!state.claim_rescue(), "the run must end at {MAX_RESCUES}");
    }

    #[test]
    fn a_real_page_starts_the_count_over() {
        let state = AppState::default();
        while state.claim_rescue() {}

        state.clear_rescues();
        assert!(state.claim_rescue(), "a fresh sign-out is a fresh case");
    }

    #[test]
    fn the_link_grant_expires_on_the_clock() {
        let state = AppState::default();
        assert!(!state.links_open_in_app(), "off by default");

        state.set_links_in_app(true, Duration::from_millis(30));
        assert!(state.links_open_in_app());

        std::thread::sleep(Duration::from_millis(60));
        assert!(
            !state.links_open_in_app(),
            "no timer ran; the deadline alone must end it"
        );
    }

    #[test]
    fn the_link_grant_can_be_turned_off_by_hand() {
        // The point of the toggle: once the sign-in is done, the user does not
        // have to wait out the rest of the five minutes.
        let state = AppState::default();
        state.set_links_in_app(true, Duration::from_secs(300));
        state.set_links_in_app(false, Duration::from_secs(300));
        assert!(!state.links_open_in_app());
    }

    #[test]
    fn turning_the_grant_off_disowns_the_thread_waiting_on_it() {
        // Otherwise that thread wakes long after the user switched the grant
        // off, announces a lapse that never happened and swaps the menu out
        // from under them for nothing. Measured before this existed: a manual
        // "off" was followed by "grant lapsed" nine seconds later.
        let state = AppState::default();
        let started = state.set_links_in_app(true, Duration::from_secs(300));
        assert!(state.links_epoch_holds(started));

        state.set_links_in_app(false, Duration::from_secs(300));
        assert!(!state.links_epoch_holds(started));
    }

    #[test]
    fn re_arming_the_grant_disowns_the_older_thread_too() {
        // Two threads, one grant: only the newest may clear the tick, or the
        // older one clears it while the newer grant is still running.
        let state = AppState::default();
        let first = state.set_links_in_app(true, Duration::from_secs(300));
        let second = state.set_links_in_app(true, Duration::from_secs(300));

        assert!(!state.links_epoch_holds(first));
        assert!(state.links_epoch_holds(second));
    }
}
