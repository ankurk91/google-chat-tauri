#!/usr/bin/env python3
"""Regression test: a notification must not raise the window by itself.

    python3 scripts/notification-test.py     # repo root, after `cargo build`

Cinnamon's notification service emits ActionInvoked("default") when a
notification merely expires, with no user interaction. Honouring that pops the
window up a few seconds after every message. The behaviour is intermittent,
which is exactly why it needs a test rather than a manual check.

This fires a notification through the debug `--test-notification` flag, waits
past the daemon's timeout, and fails if the app saw an activation.

Clicking the popup is deliberately not automated: Cinnamon draws notifications
inside the compositor rather than as X windows, so there is nothing to click at
the X level. Verify click-through by hand, with
GOOGLE_CHAT_NOTIFICATION_ACTIONS=1 set.

Requires Linux and python-xlib.
"""

import os
import pathlib
import signal
import subprocess
import sys
import time

BIN = pathlib.Path("src-tauri/target/debug/google-chat-tauri").resolve()
LOG = pathlib.Path("/tmp/gchat-notification-test.log")
SETTLE = 18  # comfortably past the daemon's default notification timeout


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

        print("firing a notification, then waiting without touching anything...")
        subprocess.run(
            [str(BIN), "--test-notification"],
            timeout=25, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        time.sleep(SETTLE)

        log = LOG.read_text(errors="replace")
        shown = "failed to show notification" not in log
        activations = log.count("[notify] activated")

        print(f"  {'PASS' if shown else 'FAIL'}  notification was sent")
        ok = activations == 0
        print(f"  {'PASS' if ok else 'FAIL'}  no self-activation  -- saw {activations}, want 0")
        return 0 if (shown and ok) else 1
    finally:
        if proc.poll() is None:
            os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
            proc.wait(timeout=10)
        print("app stopped")


if __name__ == "__main__":
    sys.exit(main())
