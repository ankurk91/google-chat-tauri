# Development

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
python3 scripts/smoke-test.py   # drives the built binary through real X11 events
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
- **GTK menu accelerators never reach the app** while focus is in the webview.
  All shortcuts are handled in `chat.js`; menu *clicks* work normally.
- **`Window::set_badge_count` does nothing on most Linux desktops.** It goes
  through tao, which `dlopen`s `libunity`. The window title carries the count
  instead.
- **The Linux tray delivers no click events at all.** `tray-icon`'s GTK backend
  emits none, so the tray menu is the only way in. Windows toggles on click.
- **Chat exposes no `<link rel="icon">`**, so the favicon cannot be used to
  detect new messages. The unread count is the signal.

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

## Releasing

Bump the version in `package.json`, `src-tauri/Cargo.toml` and
`src-tauri/tauri.conf.json`, then tag. Builds are unsigned.
