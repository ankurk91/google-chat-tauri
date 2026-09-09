# Notes

What each platform actually does — measured by running the app, not read in documentation. Every entry here is a
constraint on what the app can be, and each has a comment at the relevant code.

For the code written in response to these, see [Workarounds.md](Workarounds.md). To get the project running, see
[Development.md](Development.md).

When you add an entry, say what was measured and on what. An entry that states only its conclusion cannot be checked
later, and one of these was wrong for exactly that reason.

## Webviews and the page

- **`on_navigation` fires for every frame** on wry's WebKitGTK backend, so a host allow-list there rejects legitimate
  third-party iframes.
- **`window.Notification` is unusable in all three webviews.** WebKitGTK denies permission (`requestPermission()` →
  `"denied"`), WKWebView has no such API, and WebView2 drops notifications silently.
- **A hidden window is inert, not merely invisible.** Chat's router does nothing while the page is hidden, and Chat does
  not render its navigation either — so the DOM the unread count scrapes is absent and the count reads zero, exactly
  when the tray is the only thing the user can see.
- **WebKitGTK's failed-load page has no styling whatsoever.** It is built as `<html><body>%s</body></html>` and that is
  the whole template — confirmed by reading it out of the shipped `libwebkit2gtk-4.1`. Unstyled text is black, the
  window's `background_color` is Google's dark grey, so the one line explaining the failure renders black on black. wry
  exposes no `did-fail-load` equivalent for Rust to hang a replacement on. WKWebView leaves the document empty and
  WebView2 draws its own styled page.
- **That page is a near-dead end.** Measured by pointing `APP_URL` at a local port with nothing on it, then starting a
  server there once the load had already failed:

  | from inside the error document | result |
    |---|---|
  | `<a href>` at the URL that failed | click lands, handler runs, page never moves |
  | `location.href = location.href` | nothing |
  | `location.reload()` | nothing |
  | `location.href = <any other URL>` | navigates immediately |
  | `invoke(...)` | rejected: *"Origin header is not a valid URL"* |

  Tauri's IPC **is** injected there (`__TAURI_INTERNALS__` and `__TAURI__.core` both present), but the document has an
  opaque origin — `location.origin` is the string `"null"` — and Tauri rejects that before it looks at a capability, so
  no `remote.urls` entry can open it. The same opaque origin makes `isCrossOrigin` call every link on the page external.
- **`navigate` from inside `on_page_load` re-enters the webview.** `send_user_message` dispatches inline when already on
  the main thread, and `on_page_load` *is* the main thread, inside WebKit's own `load-changed` handler.
  `run_on_main_thread` is no escape — it goes through the same function.
- **An ACL rejection leaves a page's links dead.** The capability names `mail.google.com` and `chat.google.com`, so
  `invoke` from any other origin is refused. An initialization script runs on every top-level document — Tauri's own
  docs say so and recommend checking `window.location` — so the script is *there* on an uncovered origin whatever it
  does; what it must not do is call `preventDefault()` and then swallow the rejection. Worst case, reported: signed out
  on a Google marketing page, where "Sign in" is the only way back. `location.origin` is the right thing to branch on
  because it is what Tauri itself checks, so "the bridge will answer" and "this is Chat" cannot disagree.
- **wry has no window-open handler.** A `target="_blank"` link or a `window.open` call opens nothing at all — not a
  window, not a tab, not the current one. This is why link hand-off is the one piece of `chat.js` that still runs off
  the Chat origins: everything else there is either refused by the ACL or has no page to act on.

## Sign-in

- **Sign-in hops through a country domain** (`accounts.google.co.in/SetSID` and its equivalents). Treat one as external
  and the browser finishes the login instead of the app.
- **A sign-out can land on an advertisement.** `accounts/Logout?continue=<APP_URL>` follows the continue parameter, and
  Google then decides — not consistently — whether a session-less visit gets the sign-in form or
  `workspace.google.com/intl/en-US/gmail/`. The advertisement is a dead end: **History → Go to Chat** bounces off the
  same redirect. Verified by pointing `APP_URL` at the advertisement for one run.
- **Google serves a degraded experience to browsers it does not recognise**, and `accounts.google.com` is actively
  hostile to them. WebKitGTK identifies as Safari on Linux, which trips this.

## Notifications

- **Chat's notifications carry no click handler.** Real ones arrive through `ServiceWorkerRegistration.showNotification`
  (logged as `source=sw`) with neither an `onclick` nor a listener, so dispatching a click on them does nothing: the
  real handler is the service worker's own `notificationclick`, which the page cannot reach.
- **There is no per-conversation URL to navigate to instead.** Measured on Ubuntu 26.04 with real messages: the payload
  carries a `tag` shaped `<per-message id>/<sender user id>`, whose second field is stable per sender and whose first
  changes with every message —

  ```
  tag=Xvr7Ku2rBq8/100361636183453074426
  tag=_aaFZj4pHik/100361636183453074426     same person, three messages
  tag=aS8iqASaHUg/100361636183453074426
  ```

  so it names the *sender*, not the conversation. And the document URL never moves off
  `chat.google.com/u/0/app/home` while you walk between conversations — confirmed in the app's inspector and in a stock
  browser, so it is not a webview artefact. This is why a notification click raises the window and stops there, and why
  **Copy Current URL** can only return the app root. Opening the right conversation would mean driving Chat's own
  in-page router, and the service worker holds the only handle on it.
- **A notification "activation" is indistinguishable from a real click.** If a desktop's notification service invoked
  `default` on expiry, the window would rise after every message.
- **`notify_rust`'s `show()` is expensive enough to stall the main thread.** Measured against gnome-shell 50.1 over 25
  `Notify` calls: median 48 ms, p90 86 ms, **max 520 ms**. Tauri runs synchronous commands on the main thread, which on
  Linux also draws the window's own titlebar buttons.

## Keyboard and menus

- **GTK menu accelerators do reach the app**, and GTK consumes them before the webview sees the key — so `chat.js` never
  gets a keydown for anything the menu claims, and the two paths cannot double-fire. Measured on Mint 22.3 / Cinnamon /
  X11, idle machine, focus verified inside the webview before and after every keystroke, reproduced on a second run:

  | key | result |
    |---|---|
  | Ctrl+Q | `menu: quit`, process exits |
  | Ctrl+W | `menu: close-to-tray`, window unmapped |
  | Ctrl+= ×3 | three `menu: zoom-in`, stored zoom 1.3 — *one* step per press |

  Any *new* menu accelerator therefore takes that key away from the page. Ctrl+F stays in `chat.js` because no menu item
  claims it.

  **Tauri drops an unparsable accelerator string in silence** — it parses with `.parse().ok()`. `"CmdOrCtrl+Plus"` is
  not a name muda accepts (`Equal`, `Minus` and `NumpadPlus` are; bare `Plus` is not), and an item built that way has no
  shortcut and a blank label rather than an error.
- **Windows menu accelerators are decoration.** Tauri does register the table — muda builds an `HACCEL` and
  `tauri::app` installs a `msg_hook` calling `TranslateAcceleratorW` — but that hook only sees messages reaching tao's
  message loop, and while the webview has focus, which is always, WebView2 has them instead. Measured on Windows 11
  26200 (VirtualBox guest, no 3D, WebView2 152.0.4191.66) with synthetic keystrokes, window checked foreground:

  | key | result |
    |---|---|
  | Ctrl+W | window still visible, no `menu:` line |
  | Ctrl+Q | process still alive, no `menu:` line |
  | clicking the same items | `menu: close-to-tray`, `menu: quit` — so the handler was never the problem |

  The Win32 menu itself is correct: `GetMenuStringW` reads back `Close to Tray\tCtrl+W`, `Quit\tCtrl+Q` and
  `Zoom In\tCtrl+=`, and Windows draws all three.
- **WebView2 reserves no key at all.** Measured against a local probe page: `ctrl+w`, `ctrl+q`, `ctrl+=`, `ctrl+-`,
  `ctrl+0`, `ctrl+f`, `ctrl+shift+w`, `ctrl+shift+q`, `ctrl+m`, `ctrl+h`, `ctrl+shift+h`, `alt+home`, `f11`, `ctrl+n`,
  `ctrl+t` — every one arrived at a `keydown` listener. So **choosing a different shortcut fixes nothing.** What limits
  page-side forwarding is the ACL: it works only on an origin the capability names, so it is dead on the sign-in page.

  `SetHandled(true)` on `AcceleratorKeyPressed` really does take a key off the page rather than merely suppressing
  WebView2's own default — verified against the probe: with the hook installed it sees Ctrl+= and does **not** see
  Ctrl+W. `wry`'s `with_browser_accelerator_keys` is not the answer even if Tauri plumbed it through (it does not; only
  `additional_browser_args` reaches wry from `tauri` 2.11) — it disables WebView2's *own* browser shortcuts, which were
  never in the way.
- **Chat keeps its search input in the DOM behind a collapsed button.** Measured on the signed-in page with the box
  shut: `input[name="q"]` is present — count 1 — and not visible. The `[role="search"]` landmark holds the input and
  three buttons, of which exactly one is visible:

  ```
  buttons  0:Close search/vis=false/w=0x0  1:Clear search/vis=false/w=0x0  2:Search chat/vis=true/w=42x46
  ```

  "the visible one" identifies the button without reading `aria-label`, which is English here and something else
  wherever the app is used in another language. A plain `.click()` opens it; no synthetic pointer sequence is needed.
  The box takes about **300 ms** to draw on a software-rendered desktop, and **Chat closes it again when nothing inside
  is focused** — so a retry budget counted in frames rather than wall-clock expires first and the shortcut appears to do
  nothing at all.
- **Undo and Redo do not exist on Linux as predefined items.** muda documents them Unsupported, so `.undo()` and
  `.redo()` add nothing and the Edit menu opens with Cut.
- **muda's Fullscreen item is macOS-only, and Windows draws it anyway** — an item that does nothing when clicked. Linux
  renders nothing.
- **`Menu::get` searches the top level only**, so every check item under Preferences is invisible to it. Rebuilding the
  whole menu with `set_menu` gets every tick right, but GTK answers it with one *"no accelerator installed in accel
  group"* warning per accelerator, every time.

## The Linux desktop

- **`Window::set_badge_count` works on Ubuntu and nowhere else in this family.** tao `dlopen`s `libunity` and returns
  early unless `unity_inspector_get_unity_running()` is true — that is, unless something owns `com.canonical.Unity` on
  the session bus. Ubuntu Dock owns it and `libunity9` ships as a dependency of `nautilus`; Cinnamon, XFCE, MATE and
  plain GNOME have neither half and the call is silently inert. Verified on 26.04 by watching the bus:

  ```
  member=Update  string "application://Google Chat.desktop"
    "count" → int64 1    "count-visible" → boolean true
  ```

  The desktop id is derived by Tauri from `productName`, so it matches the entry the deb installs only as long as the
  two agree — rename one without the other and the badge quietly stops.
- **Emitting the LauncherEntry signal ourselves is not a way round that.** The obvious workaround — skip tao and
  `libunity` and put `com.canonical.Unity.LauncherEntry.Update` on the session bus directly — fails, because on Cinnamon
  nothing is listening for it. Measured on Mint 22.3 / Cinnamon / X11 on 2026-09-09, with the app running and the
  signal sent by hand:

  ```
  gdbus emit --session --object-path /com/canonical/Unity/LauncherEntry \
    --signal com.canonical.Unity.LauncherEntry.Update \
    "application://Google Chat.desktop" "{'count': <int64 42>, 'count-visible': <true>}"
  ```

  The dock did not change. The signal was not the problem: `dbus-monitor` caught it on the bus intact — right path,
  right interface, `count` 42 — and the desktop id matched the installed `Google Chat.desktop`, whose
  `StartupWMClass=google-chat-tauri` matches the window's own `WM_CLASS`. So the protocol is simply unimplemented in
  Cinnamon's panel, which is the dock here; Plank, Docky and Cairo-Dock do implement it, and none of them was running.
  Nothing owned `com.canonical.Unity` and `libunity` was not installed or mapped into the process, exactly as the entry
  above predicts.
- **A count on the Cinnamon dock icon is notifications, not unread messages.** It counts popups that have not been
  dismissed, so it tracks the notification tray and nothing else. Worth knowing because it looks precisely like the
  badge working — the number appeared during a notification burst and read 18, which was the number of popups still on
  screen. It cannot be borrowed as an unread indicator either: holding *N* notifications open to mean *N* unreads is
  the waiter-thread cost in `features::notifications` by design, and dismissing any one of them would make the number
  wrong.
- **The Linux tray delivers no click events at all.** `tray-icon`'s GTK backend emits none, so the tray menu is the only
  way in. Windows toggles on click.
- **A minimised window cannot be deiconified on Cinnamon.** `unminimize()` reaches `gtk_window_deiconify` and the window
  stays iconic however often it is asked — measured, `WM_STATE` never leaves 3. And tao refuses to focus a window it
  still believes is minimised, learning otherwise only when the window manager confirms the deiconify, which is after
  the call returns.
- **On Wayland the app cannot raise itself.** An application there cannot *take* focus, only *receive* it: the
  compositor hands out an xdg-activation token in response to a user input event, and an activation without one is
  declined. `set_focus` is tao's `present_with_time(GDK_CURRENT_TIME)`, which carries no token, and neither tao nor
  Tauri expose the protocol. Measured on Ubuntu 26.04 / GNOME 50.1 / Wayland, from the tray's Toggle:

  | window state | result |
    |---|---|
  | minimised | raises and focuses — it is hidden first, so it comes back as a fresh map |
  | visible, unfocused | GNOME posts a *"Google Chat is ready"* notification; the user has to click that |

  Notification clicks are unaffected: gnome-shell activates the app itself and passes a real token.

  Do not widen the hide-then-show to the unfocused case. It works only because compositors still treat a newly mapped
  window leniently, and that is precisely what they are tightening — KWin is switching focus-stealing prevention on and
  making it "gradually stricter as applications are being fixed". It would also cost real behaviour, since hiding a
  *visible* window makes Chat's page inert and flashes the user. `request_user_attention` is no answer either — tao maps
  it to `gtk_window_set_urgency_hint`, and Wayland has no urgency. The honest fix is xdg-activation support in tao.
- **The window's own close, minimise and maximise buttons belong to the app.** mutter offers Wayland clients no
  server-side titlebar, and the window keeps decorations, so GTK draws those three buttons *inside this process* and
  services them on the GTK main loop — confirmed under gdb: thread 1 is `ppoll` → `g_main_context_iteration` →
  `gtk_main_iteration_do` → tao's `event_loop.rs`. The page renders in a separate `WebKitWebProcess` and keeps working
  meanwhile, which is why a blocked main thread reads as "the buttons are broken" rather than "the app is busy".
  `set_unread_count` and the tray menu handler still block it; measure before assuming they are free.
- **arboard cannot use the Wayland clipboard, and it does not matter.** Every launch on GNOME Wayland warns that neither
  `ext-data-control` nor `wlr-data-control` is supported — mutter implements neither — and falls back to X11. **Copy
  Current URL** still lands in a Wayland application's paste buffer, verified by pasting one. Do not go hunting a
  clipboard bug on the strength of that warning.
- **Naming threads is not cosmetic.** Linux gives a new thread the *creating* thread's name, so an unnamed click-waiter
  inherits `notifications` and the process shows two threads by that name — confusing at exactly the moment you are
  reading a thread list to explain a freeze.

## Storage, restart and HTTP

- **WebKit's storage cannot be deleted from under a live webview.** `clear_all_browsing_data` is asynchronous, and the
  network process writes the cookie jar out again as it shuts down, so a reset-then-restart leaves the user signed in.
- **`AppHandle::restart` is the wrong restart when a plugin owns a lock.** It spawns the replacement *before* plugin
  shutdown, so the new process finds the single-instance name still held, hands its argv to the process on its way out
  and exits — leaving nothing running. It also never returns, which deadlocks a caller on a plugin thread.
- **`tauri-plugin-log` already has both targets.** It starts with a stdout target *and* a log-directory target, so pass
  both to `targets()` at once; adding them with `target()` leaves the defaults in place and writes every line twice.
- **`/releases/latest` 404s for a repository that has only ever pre-released.** GitHub documents it as "the most recent
  non-prerelease, non-draft release", so with nothing but pre-releases there is no latest at all — and the 404 is
  indistinguishable from having never released anything. Measured 2026-09-06 with v0.0.1 as a pre-release:
  `/releases/latest` 404, `/releases` one entry. GitHub also orders `/releases` by creation date, so a patch to an older
  line can appear ahead of a newer release.
- **ureq panics mid-request if the TLS provider is not named.** It defaults to Rustls, and if that is not the feature
  compiled in it neither falls back nor errors — it panics, inside the request.

## The test harnesses

- **They drive the real display, so using the machine during a run corrupts it.** They synthesise clicks and keys
  through XTEST against whatever holds focus. Keystrokes that land elsewhere produce a *null* result — no menu events, a
  shortcut that "does nothing" — which reads exactly like a bug in the app, and once produced a confidently wrong
  conclusion about menu accelerators. Each harness checks `dpy.get_input_focus()` is inside the app window before and
  after every synthetic event and aborts loudly if it is not.
- **They only see X11.** python-xlib can observe X11 clients and nothing else, and on a Wayland session GTK picks the
  Wayland backend — so the app has no X11 window and every lookup fails *exactly as if the app never started*.
  `smoke-test.py` and `reset-test.py` pin the app with `GDK_BACKEND=x11`, which on a Wayland session means XWayland;
  they test the X11 path only, and the native Wayland path has to be checked by hand. `smoke-test.py` also runs in a
  sandbox profile, or the developer's own `start_hidden` leaves no window to find and the failure looks identical.
