# Notes and workarounds

Findings, not instructions. Every entry here was paid for by running the app and watching it misbehave — a platform
disagreeing with its own documentation, an API that is a no-op on one desktop, a fix that looks obvious and is wrong.
None of it can be recovered by reading source or documentation, which is why it is written down rather than left to be
rediscovered.

Read this before changing the behaviour it describes, and add to it when something costs you an afternoon. Say what was
measured, on what, and what the wrong answer looked like — an entry that only states the conclusion cannot be checked
later, and one of these turned out to be wrong for exactly that reason (see the menu accelerator entry).

Each entry was found by running the app, and each has a comment at the relevant code. For getting the project built and
running, see [Development.md](Development.md).

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
- **GTK menu accelerators do reach the app, and the belief that they do not was a misdiagnosis.** This entry used to
  say the opposite, on the strength of one measurement: Ctrl+Plus produced no menu event with focus in the webview. It
  produced none because the item had no accelerator to fire — `"CmdOrCtrl+Plus"` is not a name muda's parser accepts
  (`Equal`, `Minus` and `NumpadPlus` are; bare `Plus` is not), and Tauri parses accelerator strings with
  `.parse().ok()`, so an unparsable one is dropped in silence rather than reported. The item was built with no shortcut
  at all, which is also why its label was blank while Zoom Out's was not.

  Measured again on Mint 22.3 / Cinnamon / X11, on an idle machine, focus verified inside the webview before *and*
  after every keystroke (`scripts/`-style xtest harness, debug build, reading the `menu: {id}` log line). Every row
  below reproduced on a second clean run:

  | key | result |
    |---|---|
  | Ctrl+Q | `menu: quit`, process exits — and `chat.js` does not map `q`, and `menu_action` refuses `quit` from the page, so this can only be the accelerator |
  | Ctrl+W | `menu: close-to-tray`, window unmapped |
  | Ctrl+= ×3 | three `menu: zoom-in`, stored zoom 1.3 — *one* step per press |

  That last row is the one to keep in mind: GTK consumes an accelerator before the webview sees the key, so `chat.js`
  never gets a keydown for anything the menu claims and the two paths do not double-fire. The `chat.js` forwarding is
  therefore redundant on Linux rather than load-bearing — leave it, because Windows is a different story (see "Where
  this stands" in [Development.md](Development.md)) —
  and any *new* menu accelerator takes that key away from the page. Ctrl+F stays in `chat.js` precisely because no menu
  item claims it.
- **Undo and Redo do not exist on Linux as predefined items.** muda documents them Unsupported there, so `.undo()` and
  `.redo()` add nothing and the Edit menu opened with Cut. Custom items driving `document.execCommand` fill the gap
  (verified: typed into the sign-in field, Edit → Undo cleared it). They deliberately carry no accelerator, per the
  point above — claiming Ctrl+Z would take the webview's own working undo away and route it through `execCommand`,
  which cannot reach an editable inside a cross-origin frame.
- **muda's Fullscreen item is macOS-only, and Windows draws it anyway.** Documented Unsupported on Windows and Linux:
  Linux renders nothing, Windows renders an item that does nothing when clicked. It is now asked for on macOS only.
- **`Window::set_badge_count` works on Ubuntu and nowhere else in this family.** It goes through tao, which `dlopen`s
  `libunity` and then returns early unless `unity_inspector_get_unity_running()` is true — that is, unless something
  owns
  `com.canonical.Unity` on the session bus. Ubuntu Dock owns it and `libunity9` ships as a dependency of `nautilus`, so
  a stock Ubuntu has both halves; Cinnamon, XFCE, MATE and plain GNOME have neither and the call is silently inert.
  Verified on 26.04 by watching the bus while the count changed:

  ```
  member=Update  string "application://Google Chat.desktop"
    "count" → int64 1    "count-visible" → boolean true
  ```

  The desktop id is derived by Tauri from `productName`, so it matches the entry the deb installs only as long as the
  two agree — rename one without the other and the badge quietly stops. The window title carries the count everywhere.
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
  activation without one is declined. `set_focus` is tao's `present_with_time(GDK_CURRENT_TIME)`, which carries no
  token, and neither tao nor Tauri expose the protocol. Measured on Ubuntu 26.04 / GNOME 50.1 / Wayland, from the tray's
  Toggle:

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
  behaviour meanwhile, because hiding a *visible* window makes Chat's page inert (see the entry on hidden windows
  below) and flashes the user.
  `request_user_attention` is no answer either — tao maps it to `gtk_window_set_urgency_hint`, and Wayland has no
  urgency. The honest fix is xdg-activation support in tao; until then this is a documented limitation.
- **The window's own close, minimise and maximise buttons belong to the app, so a busy main thread kills them.** mutter
  offers Wayland clients no server-side titlebar, and the window is built with decorations left on, so GTK draws those
  three buttons *inside this process* and services them on the GTK main loop. Confirmed by interrupting a healthy run
  under gdb: thread 1 is `ppoll` -> `g_main_context_iteration` -> `gtk_main_iteration_do` -> tao's
  `event_loop.rs:1154`. The page renders in a separate `WebKitWebProcess`, so it keeps working while the buttons are
  dead -- which is why the symptom reads as "the buttons are broken" rather than "the app is busy".

  Reported on Ubuntu 26.04 / GNOME 50.1 / Wayland: all three unresponsive for a while just after launch, fine
  afterwards. That fits what the machine is doing at the time -- a VirtualBox guest with no 3D acceleration, so Mesa
  falls back to `llvmpipe` (six of its threads sit in the process) and Chat's first paint competes with the main loop
  for six vCPUs. **Not proven**, because no backtrace was captured while it was happening: if it recurs, interrupt the
  process and read thread 1 before theorising. gdb has to *launch* the app to do that, since
  `kernel.yama.ptrace_scope` is 1 here and attaching after the fact is refused.

  One main-thread block was certain, and is now fixed. `show_notification` is a synchronous Tauri command, and Tauri
  runs those on the main thread, so `notify_rust`'s blocking `show()` round-trip used to happen there -- measured
  against gnome-shell 50.1 over 25 `Notify` calls: median 48 ms, p90 86 ms, **max 520 ms**, including about 20 ms of
  `gdbus` spawn the app does not pay. Every notification stalled the titlebar for that long, and a burst compounded it.
  `features::notifications::show_linux` now hands the work to one long-lived worker thread; `deliver` does the talking.
  One worker rather than a thread per notification, so a burst neither spawns threads unboundedly nor lets popups reach
  the daemon out of order, which serialising on the main thread used to give for free. Verified on Ubuntu 26.04 with a
  real notification: the process gains a `notifications` thread parked on its channel and a `notif-wait-<id>` thread
  parked on the click, while the main thread stays in `poll_schedule_timeout`.

  Two callers still block that thread and were left alone, being far cheaper than a notification: `set_unread_count`
  (badge, tray icon and title) and the tray menu handler. Measure before assuming they are free.

  Naming those threads is not cosmetic: Linux gives a new thread the *creating* thread's name, so the click-waiter
  spawned from the worker inherited `notifications` and the process showed two threads by that name, only one of which
  was the worker. That is confusing at exactly the moment you are reading a thread list to explain a freeze.
- **Chat costs about 1.8 GB of WebKitGTK, and the debug build is not why.** Measured on Ubuntu 26.04 / GNOME 50.1 /
  Wayland, signed in, sampling RSS every 30 seconds: 690 MB parked on the sign-in page, then 1764-2070 MB with Chat
  loaded, mean 1817 MB across twenty samples. In one 2107 MB reading the `WebKitWebProcess` holding the page was
  1693 MB and the Rust side 237 MB -- 11% of the total. The figure oscillates by roughly 100 MB as WebKit's collector
  runs, which is what makes any single reading misleading; over ten post-load minutes it drifted -33 MB, so this is a
  steady state rather than a leak. The maintainer measured the **release deb from GitHub on the same machine at the
  same 2 GB**, which rules out debug-build overhead as the explanation. This guest has no 3D acceleration, so WebKit
  composites through `llvmpipe` in CPU memory. Treat 2 GB as what Chat costs in a software-rendered WebKitGTK, not as
  something this app can shrink.
- **arboard cannot use the Wayland clipboard, and it does not matter.** Every launch on GNOME Wayland opens with a
  warning that neither `ext-data-control` nor `wlr-data-control` is supported -- mutter implements neither -- and that
  it is falling back to the X11 protocol. **Copy Current URL** still lands in a Wayland application's paste buffer,
  verified by pasting one. The warning is cosmetic; do not go hunting a clipboard bug on the strength of it.
- **The harnesses drive the real display, so using the machine during a run corrupts it.** They synthesise clicks and
  keys through XTEST against whatever currently holds focus. If the developer types while one runs, the keystrokes land
  in their window instead and the probe reports a *null* result — no menu events, a shortcut that "does nothing" — which
  reads exactly like a bug in the app. This cost an hour: it produced a confidently wrong conclusion about menu
  accelerators, and made `smoke-test.py`'s *focused after restore* look like a standing failure when it passes every
  time on an idle machine. Two defences, both cheap: check `dpy.get_input_focus()` is inside the app window before and
  after each synthetic event and abort loudly if it is not, and ask for the machine to be left alone for the run.
- **The harnesses only see X11.** python-xlib can observe X11 clients and nothing else, and on a Wayland session GTK
  picks the Wayland backend, so the app has no X11 window and every lookup fails *exactly as if the app never started* —
  a 40-second timeout and `app window never appeared`. `smoke-test.py` and `reset-test.py` therefore pin the app with
  `GDK_BACKEND=x11`, which on a Wayland session means XWayland; they consequently test the X11 path only, and the native
  Wayland path has to be checked by hand. `smoke-test.py` also runs in a sandbox profile, because it is otherwise at the
  mercy of the developer's own `start_hidden` — with that set there is no window to find and the failure looks identical
  to the one above, which cost an hour once.
- **An ACL rejection makes a page's links dead, and that is how someone gets stranded.** The capability names
  `mail.google.com` and `chat.google.com`, so every `invoke` from any other origin is turned down. `chat.js` intercepts
  cross-origin and `target=_blank` clicks *everywhere*, because an initialization script has no way to run on some
  documents and not others — so on an origin the ACL does not cover it was calling `preventDefault()` and then
  swallowing the rejection, leaving the page with links that do nothing. The worst case is the one that was reported:
  signed out on a Google marketing page, where the "Sign in" link is the only way back. `handOff` now navigates the
  window itself when Rust will not answer. Nothing is given up by it — the allow-list exists to keep links *shared
  inside Chat* out of this window, and off the Chat origins there are none.
- **A sign-out can land on an advertisement.** `accounts/Logout?continue=<APP_URL>` follows the continue parameter, and
  Google then decides — not consistently, which is why this is hard to reproduce — whether a session-less visit to Chat
  gets the sign-in form or `workspace.google.com/intl/en-US/gmail/`. The advertisement is a dead end: **History → Go to
  Chat** only bounces off the same redirect, so the only way out used to be **Reset App Data**. `features::sign_in`
  watches what commits and sends the window at `accounts.google.com/ServiceLogin` instead, at most twice in a row —
  a redirect loop would be worse than the dead end, and the dead end is now clickable anyway. Verified against the real
  page by pointing `APP_URL` at `workspace.google.com/intl/en-US/gmail/` for one run: the app launched onto the
  advertisement and arrived at Google's sign-in form. Pointing `sign_in_url` back at the advertisement as well produced
  two redirects and then the warning, which is the loop guard doing its job.
- **A menu item can be found again, but not through `Menu::get`.** That only searches the top level, so every check
  item under Preferences is invisible to it — which is why the toggles keep their own state rather than reading a tick
  back. The link grant is the one setting that changes without a click, so it needs to clear its own tick;
  `app_menu::nested_check_item` walks the submenus by hand to reach it. Rebuilding the whole menu with `set_menu` works
  too and gets every tick right, but GTK answers it with one *"no accelerator installed in accel group"* warning per
  accelerator, every time.
- **`navigate` from inside `on_page_load` re-enters the webview.** `send_user_message` dispatches inline when it is
  already on the main thread, and `on_page_load` *is* the main thread, inside WebKit's own `load-changed` handler — so
  a redirect there asks WebKit to start a second load from within the first one's callback. `run_on_main_thread` is no
  escape; it goes through the same function. `features::sign_in` sends the navigation from a spawned thread, which
  routes it through the event loop and runs it once the load has settled.
- **WebKitGTK's failed-load page has no styling whatsoever.** It is built as
  `<html><body>%s</body></html>` and that is the whole template — confirmed by reading it out of the shipped
  `libwebkit2gtk-4.1`. Unstyled text is black, the window's `background_color` is Google's dark grey, and the result is
  the one line explaining the failure rendered black on black. Electron would answer this with `did-fail-load` and a
  local error page; wry exposes no equivalent hook, so there is nothing for Rust to hang a replacement on. `chat.js`
  rewrites the document instead. The fingerprint it matches (empty head, a body with text and no elements) is narrow on
  purpose: WKWebView leaves the document empty and WebView2 draws its own styled page, so neither is touched.
- **That page is a near-dead end, and both obvious ways out of it are closed.** All measured by pointing `APP_URL` at a
  local port with nothing on it, then starting a server there once the load had already failed:

  | from inside the error document | result |
    |---|---|
  | `<a href>` at the URL that failed | click lands, handler runs, page never moves |
  | `location.href = location.href` | nothing |
  | `location.reload()` | nothing |
  | `location.href = <any other URL>` | navigates immediately |
  | `invoke(...)` | rejected: *"Origin header is not a valid URL"* |

  So WebKit will not let the stand-in document navigate to the URL it is standing in for. And the bridge is no help
  either: Tauri's IPC **is** injected there (`__TAURI_INTERNALS__` and `__TAURI__.core` both present), but the document
  has an opaque origin — `location.origin` is the string `"null"` — and Tauri rejects that before it ever looks at a
  capability, so no `remote.urls` entry can open it.

  Two things follow. The **Try again** button aims at Chat's canonical trailing-slash URL rather than at whatever
  failed, because Google treats `/chat/u/0` and `/chat/u/0/` as the same page (measured: the first answers 302 to the
  second) and a differently spelled URL is the one thing that does move. And the same opaque origin is why
  `isCrossOrigin` calls *every* link on that page external — which is what silently swallowed the first version of the
  button when it was an ordinary anchor. A `<button>` with a handler sidesteps the interceptor as well.
- **A hidden window is not just invisible, it is inert.** Chat's router does nothing while the page is hidden, so a
  notification click has to raise the window *first* and let it paint before the page is told about the click -- hence
  the ordering and the pause in `notifications::activated`.
- **Chat does not render its navigation while the window is hidden.** The DOM the unread count scrapes simply is not
  there, so the count reads zero -- exactly when the tray is the only thing the user can see. The favicon is used for
  the has-unread flag because it is driven by data rather than layout; Google publishes `..._no_dot_` and `..._dot_`
  variants and swaps between them.
- **`tauri-plugin-log` already has both targets.** It starts with a stdout target *and* a log-directory target, so pass
  both to `targets()` at once; adding them with `target()` leaves the defaults in place and writes every line twice.
- **`/releases/latest` 404s for a repository that has only ever pre-released.** GitHub documents it as "the most recent
  non-prerelease, non-draft release", so with nothing but pre-releases there is no latest at all — and a 404 is
  indistinguishable from having never released anything. Measured on 2026-09-06 with v0.0.1 published as a pre-release:
  `/releases/latest` 404, `/releases` one entry. `features::updates` therefore asks `/releases`, skips drafts, and takes
  the highest version rather than the first listed, since GitHub orders by creation date and a patch to an older line
  can be published after a newer release.
- **ureq panics mid-request if the TLS provider is not named.** It defaults to Rustls, and if that is not the feature
  compiled in it does not fall back or error — it panics, inside the request. The provider is named in the request
  config for that reason, and `native-tls` is the feature, so TLS comes from the platform.
