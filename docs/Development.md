# Development

Built with [Tauri v2](https://v2.tauri.app): a Rust backend and the operating system's own web engine — WebKitGTK on
Linux, WKWebView on macOS, WebView2 on Windows.

## Prerequisites

- **Node 24+** and **pnpm 12**
- **Rust** stable, from [rustup](https://rustup.rs) — no `sudo` needed
- **Python 3.12+** for `scripts/`, which is what they are developed against (3.12.3 at the time of writing)
- On Debian/Ubuntu/Mint:

```bash
sudo apt install -y libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
  libayatana-appindicator3-dev patchelf file build-essential libssl-dev
```

pnpm 12 is what the lockfile and the `packageManager` field expect:

```bash
npm install -g pnpm@^12
pnpm install
```

Nothing in `scripts/` is pure standard library. `gen-icons.py` resizes artwork with Pillow, and the three test harnesses
drive and observe real X11 events through python-xlib, which is the whole reason they can assert things a unit test
cannot:

```bash
sudo apt install -y python3-pil python3-xlib
```

## Everyday commands

```bash
pnpm run dev            # run, with rebuild on change
pnpm run build:linux    # .deb
pnpm run build:mac      # .app + .dmg
pnpm run build:windows  # NSIS installer

cargo test --manifest-path src-tauri/Cargo.toml
python3 scripts/smoke-test.py          # close-to-tray + window geometry, via real X11 events
python3 scripts/notification-test.py   # notifications must not raise the window by themselves
python3 scripts/reset-test.py          # Reset App Data really wipes the profile, in a sandbox
```

The three harnesses need the single-instance slot to themselves — stop `pnpm run dev` first, or the running app answers
instead of theirs. They observe X11 and so run the app under X11 whatever the session is; see "The harnesses only see
X11" below for what that does and does not prove.

The app closes to the tray, so the window's ✕ will not stop it. Kill it properly, or `dev` will refuse to start a second
copy:

```bash
pkill -f 'target/debug/google-chat-tauri'
```

## How it is put together

**There is no local frontend.** The window is created in Rust and pointed straight at Google's own web app, so this is
not a normal Tauri project — there is no bundler, no npm build step and no framework. `frontend/index.html` exists only
because the bundler wants `frontendDist` to name a directory; it is never shown.

```
frontend/index.html            placeholder, never rendered
docs/Troubleshooting.md        for people using the app, not building it
scripts/                       developer tooling; nothing here ships
src-tauri/
  permissions/chat-ipc.toml    what the page may call
  capabilities/                which origins may call it
  src/
    lib.rs                     builder wiring, plugin order, setup
    commands.rs                every command the page can reach
    urls.rs                    which links stay in-app
    config.rs                  preferences
    state.rs                   unread count, connection, quitting
    icons.rs                   embedded artwork
    inject/chat.js             the entire JS half of the app
    features/                  one module per behaviour
```

### The page-to-Rust bridge

`src-tauri/src/inject/chat.js` is the equivalent of a preload script. It is injected into Google's page as a Tauri
initialization script and compiled into the binary with `include_str!`, which also makes cargo rebuild when you edit it.
It is injected twice — once at document start, and again from `on_page_load` as a fallback — so **everything in it must
be idempotent**.

It runs in the main frame only, and does five jobs: poll the unread count, intercept link clicks, translate keyboard
shortcuts, replace
`window.Notification`, and listen for notification clicks.

### The ACL — the part that is easy to get wrong

Tauri rejects `invoke` from a remote origin unless the command is named in **both** `permissions/chat-ipc.toml` and a
capability with a matching
`remote.urls`. Miss either and every call fails. Adding a command means editing three files: `commands.rs`, the
`invoke_handler!` list in `lib.rs`, and the permission file.

Treat that list as attack surface — it is callable by a page nobody here controls. Commands validate their own input,
and anything destructive stays out of it: `menu_action` has an allow-list that excludes quit and sign-out.

## Things that are not obvious

Each of these was found by running the app, and each has a comment at the relevant code:

- **`on_navigation` cannot hold a host allow-list.** wry's WebKitGTK backend fires it for *every frame*, so an
  allow-list there rejects legitimate third-party iframes. Link policy lives in `chat.js`, which is main-frame-only.
- **Sign-in hops through a country domain** (`accounts.google.co.in/SetSID` and its equivalents). Treat one as external
  and the browser finishes the login instead of the app. See `urls::is_accounts_host`.
- **`window.Notification` is unusable in all three webviews.** WebKitGTK denies permission (`requestPermission()` →
  `"denied"`), WKWebView has no such API, and WebView2 drops notifications silently. `chat.js` replaces it entirely.
  Linux talks to `notify-rust` directly, because the notification plugin's click API is mobile-only.
- **Chat's notifications carry no click handler.** Real ones arrive through
  `ServiceWorkerRegistration.showNotification` (logged as `source=sw`) and have neither an `onclick` nor a listener, so
  dispatching a click on them does nothing: the real handler is the service worker's own `notificationclick`, which the
  page cannot reach. Clicking therefore raises the window but does not open the conversation. `chat.js` logs what each
  notification carries at
  `debug`, and falls back to any Chat link in the payload.
- **There is no per-conversation URL to fall back to.** The link fallback above has nothing to find, and this is a
  property of Chat rather than a gap in the payload. Measured on Ubuntu 26.04 with a debug build and real messages: the
  payload carries a `tag` shaped `<per-message id>/<sender user id>`, whose second field is stable per sender and whose
  first field changes with every message —

  ```
  tag=Xvr7Ku2rBq8/100361636183453074426
  tag=_aaFZj4pHik/100361636183453074426     same person, three messages
  tag=aS8iqASaHUg/100361636183453074426
  ```

  so it names the *sender*, not the conversation. And the document URL never moves off `chat.google.com/u/0/app/home`
  while you walk between conversations — confirmed both in the app's inspector and in a stock browser, so it is not a
  webview artefact. `location.assign` therefore has nothing to assign, which also explains why **Copy Current URL** can
  only ever return the app root. Opening the right conversation would mean driving Chat's own in-page router, and the
  service worker holds the only handle on it.
- **A notification "activation" is indistinguishable from a real click.** If a desktop's notification service invoked
  `default` on expiry it would raise the window after every message; `GOOGLE_CHAT_NOTIFICATION_ACTIONS=0` disables the
  action for that case. Cinnamon was wrongly suspected of this once — the activations turned out to be a human clicking
  the test notifications, which is why `scripts/notification-test.py` samples the pointer and reports *inconclusive*
  rather than passing or failing when the mouse moves.
- **GTK menu accelerators never reach the app** while focus is in the webview. All shortcuts are handled in `chat.js`;
  menu *clicks* work normally.
- **`Window::set_badge_count` works on Ubuntu and nowhere else in this family.** It goes through tao, which `dlopen`s
  `libunity` and then returns early unless `unity_inspector_get_unity_running()` is true — that is, unless something owns
  `com.canonical.Unity` on the session bus. Ubuntu Dock owns it and `libunity9` ships as a dependency of `nautilus`, so
  a stock Ubuntu has both halves; Cinnamon, XFCE, MATE and plain GNOME have neither and the call is silently inert.
  Verified on 26.04 by watching the bus while the count changed:

  ```
  member=Update  string "application://Google Chat.desktop"
    "count" → int64 1    "count-visible" → boolean true
  ```

  The desktop id is derived by Tauri from `productName`, so it matches the entry the deb installs only as long as the two
  agree — rename one without the other and the badge quietly stops. The window title carries the count everywhere.
- **The Linux tray delivers no click events at all.** `tray-icon`'s GTK backend emits none, so the tray menu is the only
  way in. Windows toggles on click.
- **Resetting app data has to happen in the *next* process.** WebKit's storage cannot be deleted from under a live
  webview: `clear_all_browsing_data` is asynchronous, and the network process writes the cookie jar out again as it
  shuts down, so a reset-then-restart leaves the user signed in. `features::
  reset` drops a sentinel and does the deleting at the top of the next launch, before any plugin or webview has opened
  those files.
- **`AppHandle::restart` is the wrong restart when a plugin owns a lock.** It spawns the replacement *before* plugin
  shutdown, so the new process finds the single-instance name still held, hands its argv to the process on its way out
  and exits -- leaving nothing running. It also never returns, which deadlocks a caller on a plugin thread.
  `request_restart` exits through `RunEvent::Exit`
  instead, and needs the same `quitting` flag as Quit or close-to-tray vetoes the window close.
- **A minimised window cannot be deiconified on Cinnamon.** `unminimize()`
  reaches `gtk_window_deiconify`, and the window stays iconic however often it is asked — measured, `WM_STATE` never
  leaves 3. Hiding it and showing it again re-maps it in the normal state. And tao refuses to focus a window it still
  believes is minimised, learning otherwise only when the window manager confirms the deiconify, which is after the call
  returns — so the focus has to be asked for again once that lands. Both are handled in
  `window::show_and_focus`.
- **On Wayland the app cannot raise itself, and this is not worked around.** An application on Wayland cannot *take*
  focus, only *receive* it: the compositor hands out an xdg-activation token in response to a user input event, and an
  activation without one is declined. `set_focus` is tao's `present_with_time(GDK_CURRENT_TIME)`, which carries no token,
  and neither tao nor Tauri expose the protocol. Measured on Ubuntu 26.04 / GNOME 50.1 / Wayland, from the tray's Toggle:

  | window state | result |
  |---|---|
  | minimised | raises and focuses — `show_and_focus` hides it first, so it comes back as a fresh map |
  | visible, unfocused | GNOME posts a *"Google Chat is ready"* notification; the user has to click that instead |

  Notification clicks are unaffected, because gnome-shell activates the app itself and passes a real token — which is
  the path that matters most here.

  It is tempting to widen the hide-then-show above to cover the unfocused case, since it demonstrably gets focus today.
  Do not: it works only because compositors still treat a newly mapped window leniently, and that is precisely what they
  are tightening — KWin is switching focus-stealing prevention on at a low level and making it "gradually stricter as
  applications are being fixed". A trick that depends on the leniency being closed is not a fix, and it would cost real
  behaviour meanwhile, because hiding a *visible* window makes Chat's page inert (see above) and flashes the user.
  `request_user_attention` is no answer either — tao maps it to `gtk_window_set_urgency_hint`, and Wayland has no
  urgency. The honest fix is xdg-activation support in tao; until then this is a documented limitation.
- **The harnesses only see X11.** python-xlib can observe X11 clients and nothing else, and on a Wayland session GTK
  picks the Wayland backend, so the app has no X11 window and every lookup fails *exactly as if the app never started* —
  a 40-second timeout and `app window never appeared`. `smoke-test.py` and `reset-test.py` therefore pin the app with
  `GDK_BACKEND=x11`, which on a Wayland session means XWayland; they consequently test the X11 path only, and the native
  Wayland path has to be checked by hand. `smoke-test.py` also runs in a sandbox profile, because it is otherwise at the
  mercy of the developer's own `start_hidden` — with that set there is no window to find and the failure looks identical
  to the one above, which cost an hour once.
- **A hidden window is not just invisible, it is inert.** Chat's router does nothing while the page is hidden, so a
  notification click has to raise the window *first* and let it paint before the page is told about the click -- hence
  the ordering and the pause in `notifications::activated`.
- **Chat does not render its navigation while the window is hidden.** The DOM the unread count scrapes simply is not
  there, so the count reads zero -- exactly when the tray is the only thing the user can see. The favicon is used for
  the has-unread flag because it is driven by data rather than layout; Google publishes `..._no_dot_` and `..._dot_`
  variants and swaps between them.

## Logs

`tauri-plugin-log` starts with a stdout target *and* a log-directory target, so pass both to `targets()` at once; adding
them with `target()` leaves the defaults in place and writes every line twice.

It writes to the platform log directory (`~/.local/share/com.ankurk91.google-chat-tauri/logs/` on Linux), reachable from
**Help → Show Logs**. Debug builds log at `debug`, release at `info`; `tao` and
`wry` are capped at `warn` because they are chatty.

Every run opens with a block from `features::diagnostics`: version and identifier, platform with distribution and
kernel, the webview engine and its version, the desktop and whether it is X11 or Wayland, where the config and logs
live, and the preferences in force. Almost every quirk in this app is specific to one of those, so a log without them
cannot be acted on. It is written before anything that can fail, so a launch that dies still says where it died.

**Help → Report an Issue** pre-fills a GitHub issue body from `diagnostics::facts` — the same function the header above
calls, so a report and the log attached to it can never disagree about the machine they came from. The finished URL is
held under 2000 characters, the smallest limit anything between the app and GitHub is likely to impose, and it drops its
prose headings before it drops any of the facts: someone can describe their own problem unprompted, but nobody retypes
their WebKitGTK version from memory.

Timestamps are local, not UTC — the first thing anyone does with a log is line it up against when they saw the problem.

The page logs through the `page_log` command, which honours `error`, `warn` and
`debug` and treats anything else as `info`. Diagnostics that should not follow a user into a release build go at
`debug`, which a release build's `info` level drops.

Use `log::{debug,info,warn,error}` rather than `eprintln!` — stderr goes nowhere once the app is launched from a desktop
menu, which is exactly when you need the diagnostics.

## Debug-only affordances

In a debug build the tray gains **Demo Badge Count** and **Test Notification**, and a second launch doubles as a remote
control — the single-instance plugin hands the running process the new argv:

```bash
cargo build --manifest-path src-tauri/Cargo.toml
./src-tauri/target/debug/google-chat-tauri &
./src-tauri/target/debug/google-chat-tauri --test-notification
./src-tauri/target/debug/google-chat-tauri --test-activation
./src-tauri/target/debug/google-chat-tauri --test-reset
```

`--test-notification` creates the notification *from inside the page*, through the `window.Notification` shim, so it
lands in the shim's map exactly like one of Chat's own. `--test-activation` then stands in for the click the popup
cannot be given — Cinnamon draws notifications inside the compositor, so there is no window to target — and drives the
whole page-side path. Between them the notification path is scriptable without a tray menu or a mouse, and
`--test-reset`
does the same for the reset, which otherwise needs its modal answered.

`scripts/reset-test.py` drives the second one with `XDG_CONFIG_HOME`,
`XDG_DATA_HOME` and `XDG_CACHE_HOME` pointed at a temporary directory, so it tests against a throwaway profile instead
of your signed-in one.

## Icons

`scripts/gen-icons.py` regenerates `src-tauri/icons/` from Google Chat's own PWA manifest, which is the only reliable
source for the current artwork — the
`productlogos` path on gstatic still serves a retired design. Google publishes matching "no dot" and "dot" variants,
which become the idle and unread tray icons.

The output is committed, so run it only when Google changes the artwork:

```bash
python3 scripts/gen-icons.py
pnpm run tauri icon src-tauri/icons/source-1024.png
```

## Where this stands

Everything planned works on Linux:
window and sign-in including Workspace accounts, the unread dot and title count, notifications with click-through, tray
and close-to-tray, window geometry, single instance, the menu bar and zoom, launch-at-login and start-hidden, link
policy, downloads, logging, Reset App Data, and deb packaging with purge cleanup. Both CI workflows are green and the
release matrix builds all five bundles.

Verified on two desktops, which between them cover both display servers:

| | Ubuntu 26.04 / GNOME 50.1 / **Wayland** | Linux Mint 22.3 / Cinnamon / **X11** |
|---|---|---|
| tray, menu, close-to-tray | yes | yes |
| notifications + click-through | yes | yes |
| dock badge (`set_badge_count`) | **yes** — Ubuntu Dock owns `com.canonical.Unity` | no — nothing owns the name |
| numbered tray icon + title count | yes | yes |
| raise from tray Toggle | **only from minimised** — see the Wayland quirk | yes |
| Reset App Data, geometry, single instance | yes | yes |

Ubuntu is the primary target; Mint is the secondary one. The split above is a display-server difference rather than a
distribution one, so read "Wayland" wherever it says Ubuntu.

What is left, in the order it matters:

1. **macOS and Windows are unverified.** A manual `release` run produces the dmg, the .app and the NSIS installer, and
   nobody has ever installed or launched one. Specifically unknown: the dock badge (macOS), the taskbar overlay icon
   (Windows), whether notifications arrive at all, and tray left-click toggle (Windows only).
2. **A notification click does not open the conversation, and cannot be made to.** It raises the window and stops there.
   This was open pending a look at a real payload; that has now happened, and the answer is that Chat has no
   per-conversation URL to navigate to and hangs no click handler — see the two quirks above. Anything better needs
   Chat's own router, so treat this as closed rather than pending unless the service worker becomes reachable.
3. **The window cannot raise itself on Wayland** when it is visible but unfocused, so tray → Toggle produces GNOME's
   "window is ready" notification instead. Waiting on xdg-activation support in tao; deliberately not worked around, for
   the reasons in the quirk above.
4. **Attachment links open in the system browser.** Deliberate for now — it works. `on_download` would keep them in-app:
   one line in `urls::is_in_app`.

**Next up: check for updates.** Not Tauri's updater plugin — that signs and installs updates itself, and on Linux only
ever updates an AppImage, never a deb. Ours is smaller: ask GitHub for the latest release, compare its tag against the
app version, and if it is newer say so and offer to open the release page in the browser. At startup and hourly after,
with a preference to turn it off. No signing key and no manifest, which is why `uploadUpdaterJson` is off in
`release.yml`. The README's "no auto-updater" line wants rewording when this lands — it still will not update itself.

**Deliberately not built:** auto-update that installs itself, offline detection, a spellchecker toggle (no Tauri API),
and single-click tray toggle on Linux — the last would mean replacing Tauri's tray with a direct StatusNotifierItem
backend, a parallel implementation judged not worth it. Left-click opens the menu, with Toggle first.

## Releasing

Bump the version in `package.json`, `src-tauri/Cargo.toml` and
`src-tauri/tauri.conf.json`, then push a `v*` tag. Builds are unsigned, so macOS needs right-click → Open and Windows
shows a SmartScreen warning.
