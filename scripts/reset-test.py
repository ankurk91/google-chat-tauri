#!/usr/bin/env python3
"""Prove that Reset App Data really wipes the profile and comes back.

Run from the repo root, after `cargo build`:
    python3 scripts/reset-test.py

The reset is a two-process affair -- one process marks it and restarts, the
next one does the deleting -- which no unit test can cover. This drives the
real binary through it and checks what survived.

Everything happens in a sandbox profile: XDG_{CONFIG,DATA,CACHE}_HOME are
pointed at a temporary directory, which both `dirs` (the app) and Tauri honour,
so your real signed-in profile is never touched. Refuses to run if an instance
is already going, because single-instance is keyed on the session bus and the
running app would answer instead of ours.
"""

import os
import pathlib
import shutil
import signal
import subprocess
import sys
import tempfile
import time

from Xlib import X, display

REPO = pathlib.Path(__file__).resolve().parent.parent
BIN = REPO / "src-tauri/target/debug/google-chat-tauri"
IDENT = "com.ankurk91.google-chat-tauri"
WM_CLASS = "google-chat-tauri"
# Planted in the files a session lives in; nothing may still contain it after.
MARKER = b"planted-session"

dpy = display.Display()
root = dpy.screen().root


def find_window(timeout=40):
    deadline = time.time() + timeout
    while time.time() < deadline:
        for w in root.query_tree().children:
            try:
                # The window of the process being replaced can vanish mid-scan.
                children = [w] + list(w.query_tree().children)
            except Exception:
                continue
            for win in children:
                try:
                    cls = win.get_wm_class()
                    geo = win.get_geometry()
                except Exception:
                    continue
                if cls and WM_CLASS in cls and geo.width > 100:
                    return win
        time.sleep(0.5)
    return None


def main():
    if subprocess.run(["pgrep", "-f", f"{WM_CLASS}$"], capture_output=True).returncode == 0:
        print("an instance is already running; stop it first")
        return 1

    sandbox = pathlib.Path(tempfile.mkdtemp(prefix="reset-test-"))
    env = dict(os.environ)
    env["XDG_CONFIG_HOME"] = str(sandbox / "config")
    env["XDG_DATA_HOME"] = str(sandbox / "data")
    env["XDG_CACHE_HOME"] = str(sandbox / "cache")

    config = sandbox / "config" / IDENT
    data = sandbox / "data" / IDENT
    failures = []

    def check(name, ok, detail=""):
        note = f"  -- {detail}" if detail and not ok else ""
        print(f"  {'PASS' if ok else 'FAIL'}  {name}{note}")
        if not ok:
            failures.append(name)

    proc = None
    try:
        print(f"[1/3] first run, sandbox at {sandbox}")
        proc = subprocess.Popen(
            [str(BIN)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
            start_new_session=True,
        )
        assert find_window(), "app window never appeared"
        time.sleep(6)  # let the page load and the cookie jar appear

        # Stand-ins for a signed-in session, next to whatever WebKit wrote.
        data.mkdir(parents=True, exist_ok=True)
        (data / "logs").mkdir(exist_ok=True)
        (data / "logs" / "keep-me.log").write_text("survivor")
        (data / "cookies").write_bytes(MARKER)
        (data / "localstorage").mkdir(exist_ok=True)
        (data / "localstorage" / "chat").write_bytes(MARKER)
        config.mkdir(parents=True, exist_ok=True)
        (config / "config.json").write_text('{"zoom":1.5,"start_hidden":true}')
        (config / ".window-state.json").write_text("{}")

        first_pid = proc.pid

        print("[2/3] reset")
        # Goes over the single-instance channel to the running app, which is how
        # the menu item would reach it.
        subprocess.run([str(BIN), "--test-reset"], env=env, timeout=30,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(2)
        check("original process exited", proc.poll() is not None)

        print("[3/3] after the restart")
        win = find_window()
        check("app came back on its own", win is not None)

        # The relaunched process is not our child; find it to clean up later.
        found = subprocess.run(["pgrep", "-f", f"{BIN}$"], capture_output=True, text=True)
        pids = [int(p) for p in found.stdout.split() if int(p) != first_pid]
        time.sleep(4)

        # A fresh jar appears the moment the new process loads the page, so the
        # question is not whether the file is there but whose session is in it.
        jar = data / "cookies"
        check(
            "signed-out: no old session in the cookie jar",
            not jar.exists() or MARKER not in jar.read_bytes(),
            "old jar survived the reset",
        )
        check("local storage deleted", not (data / "localstorage").exists())
        check("preferences deleted", not (config / "config.json").exists())
        check("window state deleted", not (config / ".window-state.json").exists())
        check("logs kept", (data / "logs" / "keep-me.log").exists())
        check("reset does not repeat", not (config / ".reset-pending").exists())

        webkit_leftovers = sorted(
            p.name for p in data.iterdir()
            if p.name not in {"logs"} and p.stat().st_mtime < time.time() - 30
        ) if data.exists() else []
        check("nothing stale left behind", not webkit_leftovers, ", ".join(webkit_leftovers))
    finally:
        for pid in locals().get("pids", []):
            try:
                os.killpg(os.getpgid(pid), signal.SIGKILL)
            except Exception:
                pass
        if proc and proc.poll() is None:
            os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
        subprocess.run(["pkill", "-f", f"{BIN}$"], capture_output=True)
        shutil.rmtree(sandbox, ignore_errors=True)
        print("app stopped, sandbox removed")

    if failures:
        print(f"\n{len(failures)} FAILED: {', '.join(failures)}")
        return 1
    print("\nall checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
