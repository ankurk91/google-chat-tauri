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
| Desktop notifications + click-through | working; click-opens-conversation unverified |
| Tray icon + menu, close-to-tray | working |
| Window geometry persistence | working |
| Single instance | working |
| Menu bar, zoom (persisted), keyboard shortcuts | working |
| Preferences: launch at login, start hidden | working |
| Links: only Chat in-app, everything else to browser | working |
| Downloads to `~/Downloads` | working for webview-initiated downloads |
| Logging to disk, Show Logs, Reset App Data | working |
| deb packaging + purge cleanup | working |
| CI (fmt, clippy, tests, release matrix) | written, never executed |

16 Rust tests. `cargo fmt --check` and `cargo clippy -- -D warnings` both clean.

## Not done

1. **macOS and Windows verification.** Both build in CI but nothing has been run
   there. Specifically unverified: dock badge (macOS), taskbar overlay icon
   (Windows), notification delivery, tray left-click toggle (Windows only).
2. **Notification click opens the right conversation.** The plumbing is proven
   end to end on the DBus wire, but clicking the popup cannot be automated —
   Cinnamon draws notifications inside the compositor, so there is no X window
   to click. Needs a human and a real incoming message.
3. **Attachment links still open in the system browser.** `on_download` is
   implemented and would handle them in-app, but the existing path works and
   flipping it cannot be verified without a real attachment to click. One-line
   change in `urls::is_in_app` if wanted.
4. **CI has never run.** No GitHub remote has been pushed to.
5. **AppImage never built locally.** Configured; expect ~100 MB since it bundles
   WebKitGTK.

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
./src-tauri/target/debug/google-chat-tauri --test-notification   # debug builds
```

The app closes to tray, so the window ✕ will not stop it:

```bash
pkill -f 'target/debug/google-chat-tauri'
```
