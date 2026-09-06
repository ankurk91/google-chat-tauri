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

Requires Linux/X11 and python-xlib. On a Wayland session the pointer check is
advisory only -- see `watch_pointer`.
"""

import os
import pathlib
import signal
import subprocess
import sys
import time

from Xlib import display

BIN = pathlib.Path("src-tauri/target/debug/google-chat-tauri").resolve()
SOURCE = pathlib.Path("src-tauri/src/features/notifications.rs")
LOG = pathlib.Path("/tmp/gchat-notification-test.log")
SETTLE = 18  # comfortably past the daemon's default notification timeout

# The line `features::notifications::activated` logs, as it reaches the log.
#
# The leading `] ` is what keeps this to activations Rust saw from the daemon:
# the page reports the same event through `page_log`, which arrives as
# `] page: notification activated: ...` and must not be counted twice.
#
# This once read `[notify] activated`, a string the app has never logged, so the
# count was always zero and "no self-activation" could not fail however many
# activations arrived. `check_marker` below is why that cannot happen twice.
ACTIVATED = "] notification activated: id="
# The same text as the format string in the source, which has no log prefix.
ACTIVATED_IN_SOURCE = ACTIVATED.removeprefix("] ")


def check_marker():
    """Fail loudly if the log line this counts has been renamed.

    The whole verdict rests on matching one string in the app's output. When
    that string drifts the count silently goes to zero and every run passes, so
    check it against the source rather than trusting it.
    """
    if not SOURCE.exists():
        return  # run from somewhere else; the count is on its own
    if ACTIVATED_IN_SOURCE not in SOURCE.read_text(errors="replace"):
        sys.exit(
            f"{SOURCE} no longer logs {ACTIVATED_IN_SOURCE!r} -- update ACTIVATED "
            f"in this script, or it will count nothing and pass regardless"
        )


def watch_pointer(seconds):
    """Sample the pointer while waiting. Returns True if the user touched it.

    X11 only. On a Wayland session this reads the XWayland pointer, which tracks
    only while the pointer is over an XWayland surface -- so a user moving the
    mouse across native Wayland windows can register as perfectly still, and the
    run reports a confident verdict it has not earned. `main` warns when it sees
    a Wayland session; treat those runs as advisory.
    """
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

    check_marker()
    if os.environ.get("WAYLAND_DISPLAY") or os.environ.get("XDG_SESSION_TYPE") == "wayland":
        print("note: Wayland session -- the pointer check sees XWayland only, "
              "so an 'untouched' verdict is advisory")

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
        activations = log.count(ACTIVATED)

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
