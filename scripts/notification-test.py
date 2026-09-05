#!/usr/bin/env python3
"""Does a notification raise the window without the user touching it?

    python3 scripts/notification-test.py     # repo root, after `cargo build`

The question this answers is *not* "does clicking work" -- that needs a human.
It is the inverse: whether an activation arrives with no interaction at all,
which would make the window pop up by itself after every message.

Distinguishing the two is the whole difficulty, and asking a human to sit still
is not a test. So this samples the pointer throughout: if an activation arrives
while the pointer has not moved and no button has been pressed, the daemon
invoked it on its own. If the pointer moved, the run is inconclusive rather than
a pass or a fail, and says so.

Clicking the popup is not automated: Cinnamon draws notifications inside the
compositor, so there is no X window to target.

Requires Linux/X11 and python-xlib.
"""

import os
import pathlib
import signal
import subprocess
import sys
import time

from Xlib import display

BIN = pathlib.Path("src-tauri/target/debug/google-chat-tauri").resolve()
LOG = pathlib.Path("/tmp/gchat-notification-test.log")
SETTLE = 18  # comfortably past the daemon's default notification timeout


def watch_pointer(seconds):
    """Sample the pointer while waiting. Returns True if the user touched it."""
    dpy = display.Display()
    root = dpy.screen().root

    def sample():
        p = root.query_pointer()
        return (p.root_x, p.root_y, p.mask & 0x1F00)  # position + button mask

    first = sample()
    deadline = time.time() + seconds
    touched = False
    while time.time() < deadline:
        now = sample()
        if now[:2] != first[:2] or now[2]:
            touched = True
        time.sleep(0.2)
    return touched


def main():
    if not BIN.exists():
        sys.exit(f"{BIN} not found -- run `cargo build` first")

    LOG.write_text("")
    proc = subprocess.Popen(
        [str(BIN)], stdout=open(LOG, "w"), stderr=subprocess.STDOUT, start_new_session=True
    )
    try:
        print("waiting for the app to come up...")
        time.sleep(14)
        if proc.poll() is not None:
            sys.exit("app exited early -- is another instance already running?")

        print(f"firing a notification, watching the pointer for {SETTLE}s...")
        subprocess.run(
            [str(BIN), "--test-notification"],
            timeout=25, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )

        touched = watch_pointer(SETTLE)

        log = LOG.read_text(errors="replace")
        shown = "failed to show notification" not in log
        activations = log.count("[notify] activated")

        print(f"  {'PASS' if shown else 'FAIL'}  notification was sent")

        if touched:
            print(f"  SKIP  self-activation  -- pointer moved or clicked during the "
                  f"wait ({activations} activation(s) seen); rerun without touching "
                  f"the mouse")
            return 0 if shown else 1

        ok = activations == 0
        print(f"  {'PASS' if ok else 'FAIL'}  no self-activation  -- pointer untouched, "
              f"saw {activations} activation(s), want 0")
        return 0 if (shown and ok) else 1
    finally:
        if proc.poll() is None:
            os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
            proc.wait(timeout=10)
        print("app stopped")


if __name__ == "__main__":
    sys.exit(main())
