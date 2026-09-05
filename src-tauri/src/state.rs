use std::sync::Mutex;

#[derive(Default)]
pub struct AppState {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    unread: i64,
    /// Set once `chat.js` reports in from the Chat page. Until then the tray
    /// shows the muted "not connected yet" icon.
    connected: bool,
    /// Set when the user picks Quit, so the close handler stops hiding to tray.
    quitting: bool,
}

impl AppState {
    pub fn unread(&self) -> i64 {
        self.inner.lock().unwrap().unread
    }

    /// Records a report from the page. Returns `true` if anything changed.
    pub fn set_unread(&self, count: i64) -> bool {
        let mut g = self.inner.lock().unwrap();
        let changed = g.unread != count || !g.connected;
        g.unread = count;
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
}
