#!/usr/bin/env python3
"""Drive the built app through the behaviours that need a real window.

Run from the repo root, after `cargo build`:
    python3 scripts/smoke-test.py

Covers close-to-tray and window-state persistence, which cannot be asserted
from a unit test because they only happen in response to real X11 events.
Always kills the app it starts, including on failure.
"""

import json
import os
import pathlib
import signal
import subprocess
import sys
import time

from Xlib import X, Xatom, display

BIN = pathlib.Path("src-tauri/target/debug/google-chat-tauri").resolve()
STATE = pathlib.Path.home() / ".config/com.ankurk91.google-chat-tauri/.window-state.json"
WM_CLASS = "google-chat-tauri"

dpy = display.Display()
root = dpy.screen().root


def find_window(timeout=40):
    """The app's top-level window, once it is mapped and sized."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        for w in root.query_tree().children:
            for win in [w] + list(w.query_tree().children):
                try:
                    cls = win.get_wm_class()
                    geo = win.get_geometry()
                except Exception:
                    continue
                if cls and WM_CLASS in cls and geo.width > 100:
                    return win
        time.sleep(0.5)
    return None


def is_viewable(win):
    try:
        return win.get_attributes().map_state == X.IsViewable
    except Exception:
        return False


def close_window(win):
    """Send WM_DELETE_WINDOW, exactly as clicking the titlebar X does."""
    ev = win.get_wm_protocols()  # ensure the protocol is understood
    assert dpy.intern_atom("WM_DELETE_WINDOW") in ev, "window does not accept WM_DELETE"
    data = [dpy.intern_atom("WM_DELETE_WINDOW"), X.CurrentTime, 0, 0, 0]
    msg = win.get_wm_protocols  # noqa: F841  (kept for clarity)
    from Xlib.protocol import event as xevent

    win.send_event(
        xevent.ClientMessage(
            window=win,
            client_type=dpy.intern_atom("WM_PROTOCOLS"),
            data=(32, data),
        )
    )
    dpy.sync()


def launch():
    return subprocess.Popen(
        [str(BIN)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True
    )


def stop(proc):
    if proc and proc.poll() is None:
        os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
        proc.wait(timeout=10)


def main():
    failures = []

    def check(name, ok, detail=""):
        print(f"  {'PASS' if ok else 'FAIL'}  {name}{'  -- ' + detail if detail else ''}")
        if not ok:
            failures.append(name)

    STATE.unlink(missing_ok=True)
    proc = None
    try:
        print("[1/2] close-to-tray + geometry save")
        proc = launch()
        win = find_window()
        assert win, "app window never appeared"

        # A distinctive geometry we can assert on after a restart.
        win.configure(x=300, y=200, width=910, height=640)
        dpy.sync()
        time.sleep(3)

        close_window(win)
        time.sleep(4)

        check("survives window close", proc.poll() is None, "process still alive")
        check("window is hidden", not is_viewable(win))
        check("geometry saved on hide", STATE.exists(), str(STATE))

        saved = json.loads(STATE.read_text()).get("main", {}) if STATE.exists() else {}
        stop(proc)
        proc = None
        time.sleep(2)

        print("[2/2] geometry restored on relaunch")
        proc = launch()
        win2 = find_window()
        assert win2, "app window never reappeared"
        time.sleep(3)
        geo = win2.get_geometry()

        want_w, want_h = saved.get("width"), saved.get("height")
        check(
            "size restored",
            want_w and abs(geo.width - want_w) <= 4 and abs(geo.height - want_h) <= 4,
            f"saved {want_w}x{want_h}, got {geo.width}x{geo.height}",
        )
    finally:
        stop(proc)
        print("app stopped")

    if failures:
        print(f"\n{len(failures)} FAILED: {', '.join(failures)}")
        return 1
    print("\nall checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
