#!/usr/bin/env python3
"""Drive the built app through the behaviours that need a real window.

Run from the repo root, after `cargo build`:
    python3 scripts/smoke-test.py

Covers close-to-tray, window-state persistence and coming back from minimised,
none of which can be asserted from a unit test: they only happen in response to
real X11 events. Always kills the app it starts, including on failure.

**This observes X11, so it launches the app under X11** -- see `app_env`. On a
Wayland session that means XWayland, and the X11 path is then the only one under
test; the native Wayland path has to be checked by hand. See "The harnesses only
see X11" in docs/Notes.md.

Everything happens in a sandbox profile, for the same reason `reset-test.py`
uses one: the app's own preferences decide whether a window appears at all, and
`start_hidden` on the developer's real profile would otherwise strand this in
`find_window` with nothing to report but a timeout.
"""

import json
import os
import pathlib
import shutil
import signal
import subprocess
import sys
import tempfile
import time

from Xlib import X, Xatom, display

BIN = pathlib.Path("src-tauri/target/debug/google-chat-tauri").resolve()
IDENT = "com.ankurk91.google-chat-tauri"
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


def wm_state(win):
    """ICCCM WM_STATE: 1 normal, 3 iconic.

    Not map_state -- Cinnamon leaves an iconified window's client mapped, so
    it still reads as viewable while minimised.
    """
    prop = win.get_full_property(dpy.intern_atom("WM_STATE"), X.AnyPropertyType)
    return prop.value[0] if prop else None


def has_focus(win):
    """Is the input focus inside this window? GTK focuses an inner child."""
    focused = dpy.get_input_focus().focus
    if not hasattr(focused, "query_tree"):
        return False
    target = toplevel(win).id
    while True:
        if focused.id == target:
            return True
        parent = focused.query_tree().parent
        if parent is None or parent.id == root.id:
            return focused.id == target
        focused = parent


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


def toplevel(win):
    """The window the WM manages -- find_window may return an inner child."""
    while True:
        parent = win.query_tree().parent
        if parent is None or parent.id == root.id:
            return win
        win = parent


def minimize(win):
    """Iconify the window the way a titlebar minimise button does."""
    from Xlib.protocol import event as xevent

    # Addressed to the client window, per ICCCM -- not the frame the window
    # manager wraps it in.
    root.send_event(
        xevent.ClientMessage(
            window=win,
            client_type=dpy.intern_atom("WM_CHANGE_STATE"),
            data=(32, [3, 0, 0, 0, 0]),  # 3 = IconicState
        ),
        event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask,
    )
    dpy.sync()


def app_env(sandbox):
    """Environment for the app: a throwaway profile, forced onto X11.

    `GDK_BACKEND=x11` is not a preference, it is what makes this file work.
    python-xlib can only see X11 clients, and on a Wayland session GTK picks the
    Wayland backend, so the app has no X11 window and every lookup here fails --
    identically to the app never starting, which is the confusing part.
    """
    env = dict(os.environ)
    env["XDG_CONFIG_HOME"] = str(sandbox / "config")
    env["XDG_DATA_HOME"] = str(sandbox / "data")
    env["XDG_CACHE_HOME"] = str(sandbox / "cache")
    env["GDK_BACKEND"] = "x11"
    return env


def launch(env):
    return subprocess.Popen(
        [str(BIN)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        start_new_session=True,
    )


def stop(proc):
    if proc and proc.poll() is None:
        os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
        proc.wait(timeout=10)


def main():
    failures = []

    def check(name, ok, detail=""):
        note = f"  -- {detail}" if detail and not ok else ""
        print(f"  {'PASS' if ok else 'FAIL'}  {name}{note}")
        if not ok:
            failures.append(name)

    sandbox = pathlib.Path(tempfile.mkdtemp(prefix="smoke-test-"))
    env = app_env(sandbox)
    state = sandbox / "config" / IDENT / ".window-state.json"

    proc = None
    try:
        print(f"[1/3] close-to-tray + geometry save, sandbox at {sandbox}")
        proc = launch(env)
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
        check("geometry saved on hide", state.exists(), str(state))

        saved = json.loads(state.read_text()).get("main", {}) if state.exists() else {}
        stop(proc)
        proc = None
        time.sleep(2)

        print("[2/3] geometry restored on relaunch")
        proc = launch(env)
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

        # The tray's Toggle ends in the same show_and_focus a second launch
        # calls, so this exercises the path without clicking a tray menu that
        # Cinnamon draws where no automation can reach it.
        print("[3/3] comes back from minimised")
        minimize(win2)
        time.sleep(2)
        check("minimised", wm_state(win2) == 3, f"WM_STATE is {wm_state(win2)}, wanted 3")

        subprocess.run([str(BIN)], env=env, timeout=30, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL)
        time.sleep(3)
        check("restored from minimised", wm_state(win2) == 1,
              f"WM_STATE is {wm_state(win2)}, wanted 1")
        check("focused after restore", has_focus(win2), "window came back unfocused")
    finally:
        stop(proc)
        shutil.rmtree(sandbox, ignore_errors=True)
        print("app stopped, sandbox removed")

    if failures:
        print(f"\n{len(failures)} FAILED: {', '.join(failures)}")
        return 1
    print("\nall checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
