# Development

For current project state and what is left, see [Handoff.md](Handoff.md).

Built with [Tauri v2](https://v2.tauri.app): a Rust backend and the operating
system's own web engine — WebKitGTK on Linux, WKWebView on macOS, WebView2 on
Windows.

## Prerequisites

- **Node 24+** and **pnpm 12** (via corepack, see below)
- **Rust** stable, from [rustup](https://rustup.rs) — no `sudo` needed
- On Debian/Ubuntu/Mint:

```bash
sudo apt install -y libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
  libayatana-appindicator3-dev patchelf file build-essential libssl-dev
```

pnpm is pinned by the `packageManager` field, so `corepack pnpm` uses the right
version without disturbing a globally installed one:

```bash
corepack pnpm install
```

Plain `pnpm` works too if yours is already 12.x.

## Everyday commands

```bash
corepack pnpm dev            # run, with rebuild on change
corepack pnpm build:linux    # .deb
corepack pnpm build:mac      # .app + .dmg
corepack pnpm build:windows  # NSIS installer

cargo test --manifest-path src-tauri/Cargo.toml
python3 scripts/smoke-test.py          # close-to-tray + window geometry, via real X11 events
python3 scripts/notification-test.py   # notifications must not raise the window by themselves
python3 scripts/reset-test.py          # Reset App Data really wipes the profile, in a sandbox
```

The app closes to the tray, so the window's ✕ will not stop it. Kill it
properly, or `dev` will refuse to start a second copy:

```bash
pkill -f 'target/debug/google-chat-tauri'
```

## How it is put together

**There is no local frontend.** The window is created in Rust and pointed
straight at Google's own web app, so this is not a normal Tauri project — there
is no bundler, no npm build step and no framework. `frontend/index.html` exists
only because the bundler wants `frontendDist` to name a directory; it is never
shown.

```
frontend/index.html            placeholder, never rendered
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

`src-tauri/src/inject/chat.js` is the equivalent of a preload script. It is
injected into Google's page as a Tauri initialization script and compiled into
the binary with `include_str!`, which also makes cargo rebuild when you edit it.
It is injected twice — once at document start, and again from `on_page_load` as
a fallback — so **everything in it must be idempotent**.

It runs in the main frame only, and does five jobs: poll the unread count,
intercept link clicks, translate keyboard shortcuts, replace
`window.Notification`, and listen for notification clicks.

### The ACL — the part that is easy to get wrong

Tauri rejects `invoke` from a remote origin unless the command is named in
**both** `permissions/chat-ipc.toml` and a capability with a matching
`remote.urls`. Miss either and every call fails. Adding a command means editing
three files: `commands.rs`, the `invoke_handler!` list in `lib.rs`, and the
permission file.

Treat that list as attack surface — it is callable by a page nobody here
controls. Commands validate their own input, and anything destructive stays out
of it: `menu_action` has an allow-list that excludes quit and sign-out.

## Things that are not obvious

Each of these was found by running the app, and each has a comment at the
relevant code:

- **`on_navigation` cannot hold a host allow-list.** wry's WebKitGTK backend
  fires it for *every frame*, so an allow-list there rejects legitimate
  third-party iframes. Link policy lives in `chat.js`, which is main-frame-only.
- **Sign-in hops through a country domain** (`accounts.google.co.in/SetSID` and
  its equivalents). Treat one as external and the browser finishes the login
  instead of the app. See `urls::is_accounts_host`.
- **`window.Notification` is unusable in all three webviews.** WebKitGTK denies
  permission (`requestPermission()` → `"denied"`), WKWebView has no such API,
  and WebView2 drops notifications silently. `chat.js` replaces it entirely.
  Linux talks to `notify-rust` directly, because the notification plugin's
  click API is mobile-only.
- **Chat's notifications carry no click handler.** Real ones arrive through
  `ServiceWorkerRegistration.showNotification` (logged as `source=sw`) and have
  neither an `onclick` nor a listener, so dispatching a click on them does
  nothing: the real handler is the service worker's own `notificationclick`,
  which the page cannot reach. Clicking therefore raises the window but does not
  open the conversation. `chat.js` logs what each notification carries at
  `debug`, and falls back to any Chat link in the payload; what is missing is a
  payload that names the conversation.
- **A notification "activation" is indistinguishable from a real click.** If a
  desktop's notification service invoked `default` on expiry it would raise the
  window after every message; `GOOGLE_CHAT_NOTIFICATION_ACTIONS=0` disables the
  action for that case. Cinnamon was wrongly suspected of this once — the
  activations turned out to be a human clicking the test notifications, which is
  why `scripts/notification-test.py` samples the pointer and reports
  *inconclusive* rather than passing or failing when the mouse moves.
- **GTK menu accelerators never reach the app** while focus is in the webview.
  All shortcuts are handled in `chat.js`; menu *clicks* work normally.
- **`Window::set_badge_count` does nothing on most Linux desktops.** It goes
  through tao, which `dlopen`s `libunity`. The window title carries the count
  instead.
- **The Linux tray delivers no click events at all.** `tray-icon`'s GTK backend
  emits none, so the tray menu is the only way in. Windows toggles on click.
- **Resetting app data has to happen in the *next* process.** WebKit's storage
  cannot be deleted from under a live webview: `clear_all_browsing_data` is
  asynchronous, and the network process writes the cookie jar out again as it
  shuts down, so a reset-then-restart leaves the user signed in. `features::
  reset` drops a sentinel and does the deleting at the top of the next launch,
  before any plugin or webview has opened those files.
- **`AppHandle::restart` is the wrong restart when a plugin owns a lock.** It
  spawns the replacement *before* plugin shutdown, so the new process finds the
  single-instance name still held, hands its argv to the process on its way out
  and exits -- leaving nothing running. It also never returns, which deadlocks a
  caller on a plugin thread. `request_restart` exits through `RunEvent::Exit`
  instead, and needs the same `quitting` flag as Quit or close-to-tray vetoes
  the window close.
- **A minimised window cannot be deiconified on Cinnamon.** `unminimize()`
  reaches `gtk_window_deiconify`, and the window stays iconic however often it
  is asked — measured, `WM_STATE` never leaves 3. Hiding it and showing it again
  re-maps it in the normal state. And tao refuses to focus a window it still
  believes is minimised, learning otherwise only when the window manager
  confirms the deiconify, which is after the call returns — so the focus has to
  be asked for again once that lands. Both are handled in
  `window::show_and_focus`.
- **A hidden window is not just invisible, it is inert.** Chat's router does
  nothing while the page is hidden, so a notification click has to raise the
  window *first* and let it paint before the page is told about the click --
  hence the ordering and the pause in `notifications::activated`.
- **Chat does not render its navigation while the window is hidden.** The DOM
  the unread count scrapes simply is not there, so the count reads zero --
  exactly when the tray is the only thing the user can see. The favicon is used
  for the has-unread flag because it is driven by data rather than layout;
  Google publishes `..._no_dot_` and `..._dot_` variants and swaps between them.

## Logs

`tauri-plugin-log` starts with a stdout target *and* a log-directory target, so
pass both to `targets()` at once; adding them with `target()` leaves the
defaults in place and writes every line twice.

It writes to the platform log directory
(`~/.local/share/com.ankurk91.google-chat-tauri/logs/` on Linux), reachable from
**Help → Show Logs**. Debug builds log at `debug`, release at `info`; `tao` and
`wry` are capped at `warn` because they are chatty.

The page logs through the `page_log` command, which honours `error`, `warn` and
`debug` and treats anything else as `info`. Diagnostics that should not follow a
user into a release build go at `debug`, which a release build's `info` level
drops.

Use `log::{debug,info,warn,error}` rather than `eprintln!` — stderr goes nowhere
once the app is launched from a desktop menu, which is exactly when you need the
diagnostics.

## Debug-only affordances

In a debug build the tray gains **Demo Badge Count** and **Test Notification**,
and a second launch doubles as a remote control — the single-instance plugin
hands the running process the new argv:

```bash
cargo build --manifest-path src-tauri/Cargo.toml
./src-tauri/target/debug/google-chat-tauri &
./src-tauri/target/debug/google-chat-tauri --test-notification
./src-tauri/target/debug/google-chat-tauri --test-activation
./src-tauri/target/debug/google-chat-tauri --test-reset
```

`--test-notification` creates the notification *from inside the page*, through
the `window.Notification` shim, so it lands in the shim's map exactly like one
of Chat's own. `--test-activation` then stands in for the click the popup
cannot be given — Cinnamon draws notifications inside the compositor, so there
is no window to target — and drives the whole page-side path. Between them the
notification path is scriptable without a tray menu or a mouse, and `--test-reset`
does the same for the reset, which otherwise needs its modal answered.

`scripts/reset-test.py` drives the second one with `XDG_CONFIG_HOME`,
`XDG_DATA_HOME` and `XDG_CACHE_HOME` pointed at a temporary directory, so it
tests against a throwaway profile instead of your signed-in one.

## Icons

`scripts/gen-icons.py` regenerates `src-tauri/icons/` from Google Chat's own PWA
manifest, which is the only reliable source for the current artwork — the
`productlogos` path on gstatic still serves a retired design. Google publishes
matching "no dot" and "dot" variants, which become the idle and unread tray
icons.

The output is committed, so run it only when Google changes the artwork:

```bash
python3 scripts/gen-icons.py
corepack pnpm tauri icon src-tauri/icons/source-1024.png
```

## CI

`ci.yml` runs `cargo fmt --check`, `cargo clippy -D warnings`, the tests, and
`node --check` on `chat.js` — the injected script has no build step, so nothing
else would catch a syntax error before it reached the page.

`release.yml` builds deb + AppImage on ubuntu-24.04, a universal dmg on macOS
and an NSIS installer on Windows. Only semver tags (`v1.2.3`, or
`v1.2.3-beta.1`) start a release. A tag push puts them in a draft release; a
manual run (**Actions → release → Run workflow**) builds the same bundles and
leaves them as **workflow artifacts**, releasing nothing — that is the way to
get something to test without cutting a version.

Bundle targets are passed per platform with `--bundles` rather than read from
`tauri.conf.json`, so a host can never emit something we do not ship — notably
rpm. Building on **ubuntu-24.04** sets the glibc floor at 2.39, so the Linux
bundles need Ubuntu 24.04 / Mint 22 or newer; 22.04 is deliberately not
supported.

Both workflows use a `concurrency` group. CI cancels a superseded run, releases
never do — a half-uploaded draft is worse than a slow one. `ci.yml` is
read-only; only the release job asks for `contents: write`.

A manual run produces every bundle a release would contain. GitHub always zips
workflow artifacts and names the zip after the artifact, so the names carry the
real extension — `google-chat-tauri_1.0.0_linux-amd64.deb`, and so on — and the
zip holds that file. Release assets are named the same way but are uploaded
whole, extension and all, with no zip around them.

Building on **ubuntu-24.04** is deliberate even though newer runners exist: a
binary built against an older glibc runs on newer systems, never the other way
round. Ubuntu 26.04 still ships `libwebkit2gtk-4.1`, so these bundles run there
unchanged.

**Do not build the AppImage locally.** `linuxdeploy`'s GTK plugin copies and
patches the whole GTK/WebKit stack — the AppDir passes 200 MB and the run takes
well over fifteen minutes on a laptop. CI has the time; a laptop should not
spend it.

## Releasing

Bump the version in `package.json`, `src-tauri/Cargo.toml` and
`src-tauri/tauri.conf.json`, then push a `v*` tag. Builds are unsigned, so macOS
needs right-click → Open and Windows shows a SmartScreen warning.
