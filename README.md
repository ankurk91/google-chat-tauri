# Google Chat (Tauri)

Unofficial desktop app for [Google Chat](https://chat.google.com), built with
[Tauri v2](https://v2.tauri.app). A port of
[google-chat-electron](https://github.com/ankurk91/google-chat-electron).

Unlike the Electron original this uses the **host OS webview** (WebKitGTK on
Linux, WKWebView on macOS, WebView2 on Windows) rather than bundling Chromium,
so the installer is a few MB instead of ~180 MB.

Installs alongside the Electron app — different binary name, bundle identifier
and data directory — so you can run both while comparing.

## Status

Core wrapper works: window, sign-in, tray icon, unread counter, close-to-tray,
window-state persistence, single-instance, external-link handling.
Menus, preferences, autostart and desktop notifications are not done yet.

## Development

Prerequisites: Node 24+, Rust (stable), and on Debian/Ubuntu/Mint:

```bash
sudo apt install -y libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
  libayatana-appindicator3-dev patchelf file build-essential libssl-dev
```

```bash
corepack pnpm install
corepack pnpm dev            # run
corepack pnpm build:linux    # build a .deb
cargo test --manifest-path src-tauri/Cargo.toml
```

`corepack pnpm` pins pnpm to the version in `packageManager` without touching a
globally installed pnpm. Plain `pnpm` works too if yours is 12.x.

## How it works

There is no local frontend. The window is created in Rust
([`src-tauri/src/features/window.rs`](src-tauri/src/features/window.rs)) and
points straight at Google's own web app, so the interesting parts are:

- **[`src-tauri/src/inject/chat.js`](src-tauri/src/inject/chat.js)** — the whole
  JS half of the app, injected into Google's page as a Tauri initialization
  script (the equivalent of Electron's preload). Compiled into the binary with
  `include_str!`; no bundler, no npm build step.
- **[`src-tauri/permissions/chat-ipc.toml`](src-tauri/permissions/chat-ipc.toml)**
  + **[`src-tauri/capabilities/remote-chat.json`](src-tauri/capabilities/remote-chat.json)**
  — Tauri rejects `invoke` from a remote origin unless the command is named in
  *both* an app permission and a capability with a matching `remote.urls`.
  Everything `chat.js` can call is listed there, and nothing else.

A Firefox user-agent is sent
([`src-tauri/src/features/user_agent.rs`](src-tauri/src/features/user_agent.rs));
Google serves a degraded experience to WebKitGTK's default Safari-on-Linux
string. Override it with `GOOGLE_CHAT_UA` when debugging sign-in.

## Troubleshooting

**Blank window on some GPUs** — a known WebKitGTK issue. Add to the `Exec=` line
of the `.desktop` file, or export before launching:

```
WEBKIT_DISABLE_DMABUF_RENDERER=1
```

## Licence

GPL-3.0-only, as the original.
