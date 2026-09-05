# Project state

Snapshot for picking this up in a fresh session. Written 2026-09-05.

## What this is

Unofficial Google Chat desktop app, ported from an Electron original
(`/home/ankurk/projects/rub/google-chat-electron`) to **Tauri v2.11**. Uses the
host OS webview instead of bundling Chromium, so the Linux installer is ~2.6 MB
against ~80–90 MB before.

Ships as a **separate app** from the Electron one — binary `google-chat-tauri`,
identifier `com.ankurk91.google-chat-tauri` — so both can be installed at once.

## Status: feature-complete for Linux

Everything planned is done and verified on Linux Mint 22.3 / Cinnamon / X11,
except the items under "Not done" below.

| Area | State |
|---|---|
| Window, sign-in (incl. Workspace, any country) | working |
| Unread: tray dot, window title, dock/taskbar badge | working |
| Desktop notifications + click-through | working; click raises the window, does not open the conversation |
| Tray icon + menu, close-to-tray | working |
| Window geometry persistence | working |
| Single instance | working |
| Menu bar, zoom (persisted), keyboard shortcuts | working |
| Preferences: launch at login, start hidden | working |
| Links: only Chat in-app, everything else to browser | working |
| Downloads to `~/Downloads` | working for webview-initiated downloads |
| Logging to disk, Show Logs | working |
| Reset App Data | working; wipe + restart verified against a sandbox profile |
| deb packaging + purge cleanup | working |
| CI (fmt, clippy, tests, release matrix) | written, never executed |
| AppImage | built by CI only; far too slow to bundle on a laptop |

Confirmed by hand on 2026-09-05 with real incoming messages: the popup appears
while the window is hidden, the tray dot follows, starting hidden still receives
notifications, and a download hands off to the browser.

19 Rust tests. `cargo fmt --check` and `cargo clippy -- -D warnings` both clean.

## Not done

1. **macOS and Windows verification.** Both build in CI but nothing has been run
   there. Specifically unverified: dock badge (macOS), taskbar overlay icon
   (Windows), notification delivery, tray left-click toggle (Windows only).
2. **Notification click does not open the conversation.** Verified with a real
   message on 2026-09-05: the popup appears, clicking it raises the window, but
   the page stays where it was. The cause is now known — Chat creates its
   notifications through `ServiceWorkerRegistration.showNotification` and hangs
   no click handler on the object, so there is nothing to dispatch to and the
   payload carried no link either (`source=sw handlers=0`, no `link=`). The
   remaining question is whether anything in that payload names the
   conversation; `chat.js` logs it at `debug`, so the next real message on a
   debug build will say. `--test-notification` and `--test-activation` drive the
   whole path without needing a message or a mouse.
3. **Attachment links still open in the system browser.** Tested 2026-09-05 and
   the browser hand-off works fine, so this stays as it is. `on_download` is
   implemented and would keep them in-app: one line in `urls::is_in_app`.
4. **CI has never run.** `origin` exists and has commits, but everything since
   `91eaa40` — the CI workflows included — is local only. Both workflows now
   target **ubuntu-24.04**; 22.04 is not supported.
5. **AppImage has never been built to completion.** Attempted locally on
   2026-09-05 and abandoned: `linuxdeploy`'s GTK plugin was still copying
   libraries after fifteen minutes, with the AppDir past 200 MB. It is a CI job
   now — run `release` by hand and take the workflow artifacts. Expect ~100 MB,
   since it bundles WebKitGTK.

## Deliberately not built

Auto-update, offline detection, spellchecker toggle (no Tauri API), and
single-click tray toggle on Linux — the last would need replacing Tauri's tray
with a direct StatusNotifierItem backend, which was judged not worth a parallel
implementation. Left-click opens the menu, with Toggle first.

## Things that will bite you

Each of these cost real time to find, and each is commented at the code.

- **`on_navigation` cannot hold a host allow-list.** wry's WebKitGTK backend
  fires it for *every frame*, so an allow-list there rejects legitimate
  third-party iframes. Link policy lives in `chat.js`, which is main-frame-only.
- **Sign-in hops through a country domain** (`accounts.google.co.in/SetSID`).
  Send that to the browser and the browser finishes the login, stranding the
  app on the sign-in page. `urls::is_accounts_host` handles every ccTLD.
- **Chat does not render its navigation while the window is hidden.** The unread
  count scrapes a DOM that is not there, so it reads zero — exactly when the
  tray matters. The favicon (`_no_dot_` vs `_dot_`) is the signal that survives.
- **`window.Notification` is unusable in all three webviews.** WebKitGTK denies
  permission and Tauri 2.11 cannot grant it; WKWebView has no such API;
  WebView2 drops them. `chat.js` replaces it wholesale.
- **GTK menu accelerators never reach the app** while focus is in the webview.
  Shortcuts are handled in `chat.js`; menu clicks work normally.
- **Chat hangs no click handler on its notifications.** They come through the
  service worker registration, whose `notificationclick` handler the page cannot
  reach, so a synthetic click on the object runs nothing. Raising the window is
  all a click can currently do.
- **A hidden window is inert, not just invisible.** Chat's router ignores a
  navigation while the page is hidden, so a click has to raise the window and
  let it paint *before* the page hears about it -- see the ordering in
  `notifications::activated`.
- **App data cannot be reset from inside the process holding it.**
  `clear_all_browsing_data` is asynchronous and WebKit rewrites the cookie jar
  as it shuts down, so the first version of this restarted straight back into
  the same signed-in session. `features::reset` now drops a sentinel and deletes
  at the top of the *next* launch, before any plugin or webview opens a file.
- **`AppHandle::restart` breaks single instance.** It spawns the replacement
  before plugin shutdown, so the new process finds the DBus name still held,
  hands its argv to the dying one and exits — nothing left running. It also
  never returns, deadlocking a caller on a plugin thread. Use `request_restart`,
  and set the same `quitting` flag Quit uses or close-to-tray vetoes the exit.
- **`tauri-plugin-log` ships with stdout and log-dir targets already set.**
  Adding them with `target()` kept the defaults and wrote every line twice; use
  `targets()`.
- **`Window::set_badge_count` does nothing on most Linux desktops** — tao
  `dlopen`s `libunity`. The window title carries the count.
- **The Linux tray delivers no click events at all.**
- **A remote origin needs both** an app permission TOML *and* a capability with
  matching `remote.urls`, or every `invoke` fails silently.

## Where to look

- `src-tauri/src/inject/chat.js` — the whole JS half; injected into Google's
  page, compiled in with `include_str!`, must stay idempotent.
- `src-tauri/src/commands.rs` + `permissions/chat-ipc.toml` — everything the
  remote page can call. Treat as attack surface; keep it short.
- `src-tauri/src/features/` — one module per behaviour.
- `docs/Development.md` — build, architecture, CI.

## Handy

```bash
corepack pnpm dev                      # run
corepack pnpm build:linux              # .deb
cargo test --manifest-path src-tauri/Cargo.toml
python3 scripts/smoke-test.py          # close-to-tray + geometry, real X11
python3 scripts/notification-test.py   # no self-raising notifications
python3 scripts/reset-test.py          # reset wipes the profile, in a sandbox
./src-tauri/target/debug/google-chat-tauri --test-notification   # debug builds
./src-tauri/target/debug/google-chat-tauri --test-activation     # fake a click
./src-tauri/target/debug/google-chat-tauri --test-reset          # debug builds
```

The app closes to tray, so the window ✕ will not stop it:

```bash
pkill -f 'target/debug/google-chat-tauri'
```
