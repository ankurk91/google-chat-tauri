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

The fired notification is also checked on the bus for the `desktop-entry` and
`suppress-sound` hints, via dbus-monitor. Whether the daemon then keeps its
silence needs ears; see docs/Notes.md.

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
BUS = pathlib.Path("/tmp/gchat-notification-test.bus")
SETTLE = 18  # comfortably past the daemon's default notification timeout

# Each hint: how it renders in the dbus-monitor capture, and the source
# fragment that produces it (checked in `check_marker`, so a removed or
# renamed hint fails loudly instead of passing vacuously).
HINTS = {
    "desktop-entry": {
        "on_bus": 'string "Google Chat"',
        "source": 'Hint::DesktopEntry("Google Chat".into())',
    },
    "suppress-sound": {
        "on_bus": "boolean true",
        "source": 'Hint::SuppressSound(true)',
    },
}

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

# Positive evidence that the daemon accepted a notification.
#
# "no error in the log" is not evidence: a notification that was never even
# attempted satisfies it too. That is not hypothetical -- this fires through
# `--test-notification`, which goes via the page's `invoke`, and the ACL only
# answers on mail.google.com and chat.google.com. Run this signed out, when the
# window sits on accounts.google.com, and the call is rejected, nothing is shown,
# nothing is logged, and the check used to pass anyway.
SHOWN = "] notification: shown id="
SHOWN_IN_SOURCE = SHOWN.removeprefix("] ")


def check_marker():
    """Fail loudly if the log line this counts has been renamed.

    The whole verdict rests on matching one string in the app's output. When
    that string drifts the count silently goes to zero and every run passes, so
    check it against the source rather than trusting it.
    """
    if not SOURCE.exists():
        return  # run from somewhere else; the count is on its own
    src = SOURCE.read_text(errors="replace")
    for marker, name in ((ACTIVATED_IN_SOURCE, "ACTIVATED"), (SHOWN_IN_SOURCE, "SHOWN")):
        if marker not in src:
            sys.exit(
                f"{SOURCE} no longer logs {marker!r} -- update {name} in this "
                f"script, or it will match nothing and pass regardless"
            )
    for hint in HINTS.values():
        if hint["source"] not in src:
            sys.exit(
                f"{SOURCE} no longer contains {hint['source']!r} -- update "
                f"HINTS in this script, or the bus check passes regardless"
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


def watch_bus():
    """Capture session-bus Notify traffic. None when dbus-monitor is missing.

    Scoped to the daemon's Notify method; a notification another app fires
    inside the same window would be captured too, so the machine must be quiet.
    """
    try:
        return subprocess.Popen(
            ["dbus-monitor", "--session",
             "type='method_call',interface='org.freedesktop.Notifications',member='Notify'"],
            stdout=open(BUS, "w"), stderr=subprocess.DEVNULL,
        )
    except FileNotFoundError:
        print("note: dbus-monitor not found -- the D-Bus hints check is skipped")
        return None


def check_bus():
    """Whether the capture carries every hint in HINTS."""
    text = BUS.read_text(errors="replace")
    return all(
        f'string "{name}"' in text and hint["on_bus"] in text
        for name, hint in HINTS.items()
    )


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
    # Capture the bus while the notification goes out.
    bus = watch_bus()
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
        # Both halves: the daemon said yes, and nothing said no.
        shown = SHOWN in log and "failed to show notification" not in log
        activations = log.count(ACTIVATED)

        detail = ""
        if not shown:
            detail = ("  -- the app logged no successful show; if the window is "
                      "signed out the ACL will have refused the page's invoke")
        print(f"  {'PASS' if shown else 'FAIL'}  notification was sent{detail}")

        if bus is not None:
            # The capture has had the whole firing-plus-settle window to be
            # written; read it as it stands.
            bus_ok = check_bus()
            verdict = "PASS" if bus_ok else "FAIL"
            print(f"  {verdict}  D-Bus hints reach the daemon "
                  f"({', '.join(sorted(HINTS))})")
            if not bus_ok and shown:
                print("        capture at", BUS)
        else:
            bus_ok = None

        failed = not shown
        if touched:
            print(f"  SKIP  self-activation  -- pointer moved or clicked during the "
                  f"wait ({activations} activation(s) seen); rerun without touching "
                  f"the mouse")
        else:
            ok = activations == 0
            print(f"  {'PASS' if ok else 'FAIL'}  no self-activation  -- pointer "
                  f"untouched, saw {activations} activation(s), want 0")
            failed = failed or not ok
        if bus_ok is False:
            failed = True
        return 1 if failed else 0
    finally:
        if proc.poll() is None:
            os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
            proc.wait(timeout=10)
        if bus is not None and bus.poll() is None:
            bus.terminate()
            bus.wait(timeout=10)
        print("app stopped")


if __name__ == "__main__":
    sys.exit(main())
