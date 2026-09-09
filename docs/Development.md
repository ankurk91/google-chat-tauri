# Development

An unofficial desktop wrapper for Google Chat, built with [Tauri v2](https://v2.tauri.app): a Rust backend and the
operating system's own web engine — WebKitGTK on Linux, WKWebView on macOS, WebView2 on Windows.

This page gets you building and running, and explains how the pieces fit. Two companion files carry the rest:

- [Workarounds.md](Workarounds.md) — code that looks wrong until you know why. Read the entry before changing it.
- [Notes.md](Notes.md) — what each platform actually does, and the limits that follow.

## Prerequisites

- **Node 24+**, and **pnpm 12** — `npm install -g pnpm@12`
- **Rust** — install [rustup](https://rustup.rs). The version is pinned by `rust-toolchain.toml` and fetched the first
  time you run `cargo`, so there is nothing to choose.
- **Python 3.12+**, only for `scripts/`

On Debian/Ubuntu/Mint:

```bash
sudo apt install -y libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
  libayatana-appindicator3-dev patchelf file build-essential libssl-dev
sudo apt install -y python3-pil python3-xlib   # for scripts/
pnpm install
```

The Rust pin is the floor our dependencies impose. Raising it means editing `rust-toolchain.toml` and `rust-version` in
`src-tauri/Cargo.toml` together — they are kept identical.

## Everyday commands

```bash
pnpm run dev                     # run, with rebuild on change
pnpm run build:linux             # .deb
pnpm run build:linux-portable    # AppImage
pnpm run build:mac               # .app + .dmg
pnpm run build:windows           # NSIS installer
```

The app closes to the tray, so the window's ✕ will not stop it — and `dev` refuses to start a second copy:

```bash
pkill -f 'target/debug/google-chat-tauri'
```

## Tests

```bash
cargo test --manifest-path src-tauri/Cargo.toml
node scripts/chatjs-test.js             # chat.js against a stand-in page: no browser, no signed-in session

python3 scripts/smoke-test.py           # close-to-tray + window geometry, via real X11 events
python3 scripts/notification-test.py    # notifications must not raise the window by themselves
python3 scripts/reset-test.py           # Reset App Data really wipes the profile, in a sandbox
```

On Windows, in PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\windows-shortcut-test.ps1
```

It reads the Win32 menu back with `GetMenuStringW` and injects real Ctrl+W and Ctrl+Q — the only way to tell a shortcut
that is missing from one that is present, correct and silently never dispatched.

CI runs the first two, alongside the checks below. The rest drive the real desktop, and two rules follow:

- **Stop `pnpm run dev` first.** Each harness needs the single-instance slot, or the running app answers instead.
- **Leave the machine alone while one runs.** They synthesise input against whatever holds focus; each aborts rather
  than reporting a false failure, but your keystrokes will end the run.

The Python harnesses observe X11 only — see [Notes.md](Notes.md) for what that does and does not prove.

### What CI checks

`ci.yml` runs on a push to `main` touching `src-tauri/`, `scripts/`, `package.json`, `pnpm-lock.yaml` or `ci.yml`
itself — and `release.yml` calls it as the `checks` job that its `build` needs, so these five decide whether a release
happens at all. They are the whole gate; run them before pushing a tag:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
node --check src-tauri/src/inject/chat.js
node scripts/chatjs-test.js
```

**A green `cargo test` is not a green CI.** `--check` fails on a line `cargo build` is perfectly happy with, and
`-D warnings` turns every clippy lint into an error. Formatting is the easiest of the five to break without noticing:
edit a call so its arguments would now fit on one line and rustfmt wants them there, though nothing you ran locally
says so.

The Python harnesses are deliberately absent — they drive a real desktop, which a runner does not have.

## How it is put together

**There is no local frontend.** The window is created in Rust and pointed straight at Google's own web app, so this is
not a normal Tauri project — no bundler, no npm build step, no framework. `frontend/index.html` exists only because the
bundler wants `frontendDist` to name a directory; it is never shown.

```
frontend/index.html            placeholder, never rendered
scripts/                       developer tooling; nothing here ships
src-tauri/
  permissions/chat-ipc.toml    what the page may call
  capabilities/                which origins may call it
  src/
    lib.rs                     builder wiring, plugin order, setup
    commands.rs                every command the page can reach
    urls.rs                    which links stay in-app, and which pages are a dead end
    config.rs                  preferences
    state.rs                   unread count, connection, quitting
    icons.rs                   embedded artwork
    inject/chat.js             the entire JS half of the app
    features/                  one module per behaviour
```

Each `features/` module opens with a doc comment explaining why it exists. Start there rather than here.

### chat.js

`src-tauri/src/inject/chat.js` is the equivalent of a preload script: injected into Google's page as a Tauri
initialization script and compiled in with `include_str!`, so cargo rebuilds when you edit it. It runs in the main frame
only and does six jobs — poll the unread count, intercept link clicks, translate keyboard shortcuts, replace
`window.Notification`, listen for notification clicks, and rewrite the webview's own failed-load page.

It is injected twice, at document start and again from `on_page_load` as a fallback, so **everything in it must be
idempotent**.

`scripts/chatjs-test.js` runs it in a `vm` context against fakes for `document`, `window` and the Tauri bridge, reaching
it the way Chat does. A `vm` context has the ECMAScript intrinsics and no web APIs, so the fakes must hand in `URL`, and
the fake `location` needs `origin` as well as `href` — without either, every link silently looks same-origin or
external.

### The ACL

Tauri rejects `invoke` from a remote origin unless the command is named in **both** `permissions/chat-ipc.toml` and a
capability with a matching `remote.urls`. Miss either and every call fails. Adding a command means editing three files:
`commands.rs`, the `invoke_handler!` list in `lib.rs`, and the permission file.

Treat that list as attack surface — it is callable by a page nobody here controls. Commands validate their own input,
and anything destructive stays out: `menu_action` has an allow-list that excludes quit and sign-out.

## Logs

Logs go to the platform log directory (`~/.local/share/com.ankurk91.google-chat-tauri/logs/` on Linux), reachable from
**Help → Show Logs**. Debug builds log at `debug`, release at `info`; `tao` and `wry` are capped at `warn`. Timestamps
are local, not UTC.

Every run opens with a block from `features::diagnostics`: version, platform and kernel, webview engine and version,
desktop and display server, config and log paths, and the preferences in force. Almost every quirk in this app is
specific to one of those, so a log without them cannot be acted on. **Help → Report an Issue** pre-fills a GitHub issue
from the same function, so a report and its log can never disagree about the machine.

Use `log::{debug,info,warn,error}` rather than `eprintln!` — stderr goes nowhere once the app is launched from a desktop
menu. The page logs through the `page_log` command, which honours `error`, `warn` and `debug` and treats anything else
as `info`.

**A log line must not identify anyone.** These files are written to be attached to a public GitHub issue, so nothing
that names the person running the app can go in one. Two things carry that without looking like it, and both have a
redactor to go through:

- **Paths**, because every one of ours starts at the home directory — `/home/jane`, or `C:\Users\Jane Smith`. Print
  them with `redact::path`, which folds the home directory to `~` and reduces anything outside it to `.../name`.
- **URLs**, because Google puts the signed-in address in the query (`Email`, `identifier`, `authuser`), an attachment
  link carries a bearer token there, and the fragment on a Chat URL names the open conversation. Which redactor to use
  depends on **who built the URL**:
  - `redact::url` for one of ours — the releases endpoint, the sign-in target. Those paths are `format!`ed from
    constants, so they describe our own code and name nobody. The path survives; the query becomes a count.
  - `redact::foreign_url` for anything that came from the page — a clicked link, the address the window is on, a
    download. There the path is content, not structure, and only the host survives. The host is kept because the
    allow-list in `urls` routes on it, so it is the part a hand-off bug needs.

  `chat.js` logs nothing but page-supplied URLs, so its `redactUrl` is the `foreign_url` rule.

Message bodies and sender names never reach Rust: `commands::show_notification` hands the title and body to the OS
without logging either, and `chat.js` reports a notification's payload only where it looks like an id.

## Updates and the network

`features::updates` asks GitHub for releases, compares the highest tag against `CARGO_PKG_VERSION` with `semver`, and
offers to open the release page in the browser. It downloads and installs nothing — that is the whole design. Tauri's
updater plugin signs an artifact and swaps it in, which needs a key in CI and on Linux only works for an AppImage, never
the deb most people install; `uploadUpdaterJson` is off in `release.yml` for the same reason.

One thread schedules it: thirty seconds after launch, then every twelve hours. An automatic check is silent unless there
is something new, and silent about a version it has already offered (`offered_version` in the config). **Help → Check
for Updates** always answers, because a manual check that appears to do nothing is indistinguishable from a broken one.
**Preferences → Check for Updates Automatically** turns the scheduled half off.

A release flagged pre-release on GitHub is never offered, whatever is running. `0.x` used to be carved out — every
release of it was tagged pre-release, so someone on 0.0.1 had to hear about 0.0.2 — and running a beta used to opt you
into the next one. Neither holds from 1.0.0 on: the check offers stable releases only, and a beta is something people
go to the releases page for. `release.yml` publishes with `prerelease: false`, so this filter is about anything flagged
by hand.

`features::connectivity` is the other half. The window points at a remote page, so with no route to the internet the
webview shows an error page that says nothing about the app. One TCP connection to `chat.google.com:443` — no TLS, no
HTTP, nothing a captive portal can answer misleadingly — decides it. Launching at login races the network, so it retries
across about a minute and then tells the user once, through the desktop's own notification. It then polls every thirty
seconds until the network answers and loads Chat.

## Debug-only affordances

In a debug build the tray gains **Demo Badge Count** and **Test Notification**, and a second launch doubles as a remote
control — the single-instance plugin hands the running process the new argv:

```bash
cargo build --manifest-path src-tauri/Cargo.toml
./src-tauri/target/debug/google-chat-tauri &
./src-tauri/target/debug/google-chat-tauri --test-notification    # via the window.Notification shim, like Chat's own
./src-tauri/target/debug/google-chat-tauri --test-activation      # stands in for the click on the popup
./src-tauri/target/debug/google-chat-tauri --test-reset           # Reset App Data without answering its modal
./src-tauri/target/debug/google-chat-tauri --test-update-check    # skips the thirty-second wait
./src-tauri/target/debug/google-chat-tauri --test-links-in-app    # ticks Preferences → Open Every Link in This Window
```

Three environment variables, all debug-only — each one can quietly change a policy, which has no place in a release:

|                                        |                                                                                       |
|----------------------------------------|---------------------------------------------------------------------------------------|
| `GOOGLE_CHAT_LINK_GRANT_SECS=10`       | shortens the five-minute link grant, so the lapse fits in one test run                |
| `GOOGLE_CHAT_PROBE_HOST=192.0.2.1:443` | TEST-NET-1 never routes, so the connectivity probe fails without touching the network |
| `GOOGLE_CHAT_NOTIFICATION_ACTIONS=0`   | drops the notification's `default` action                                             |

`GOOGLE_CHAT_UA` overrides the spoofed user agent in any build — an escape hatch for sign-in problems without a rebuild.

## Icons

`scripts/gen-icons.py` regenerates `src-tauri/icons/` from Google Chat's own PWA manifest, the only reliable source for
the current artwork — the `productlogos` path on gstatic still serves a retired design. Google publishes matching "no
dot" and "dot" variants, which become the idle and unread tray icons.

The output is committed, so run this only when Google changes the artwork:

```bash
python3 scripts/gen-icons.py
pnpm run tauri icon src-tauri/icons/source-1024.png
```

## Releasing

Bump the version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, then push a `v*` tag. The
version is in `src-tauri/Cargo.lock` too — cargo rewrites it on the next build, so it belongs in the same commit. The
release matrix builds all five bundles. Builds are unsigned, so macOS and Windows block the first launch; the way past
each is in the release notes `release.yml` writes, and in [Troubleshooting.md](Troubleshooting.md).

Two things about the tag are worth knowing before you push one:

- **The build gates on the checks.** `release.yml` runs `ci.yml` first and bundles nothing if it fails, so a tag on a
  commit that fails so much as `cargo fmt --check` produces no artifacts. Fixing `main` afterwards does not help — the
  build runs at the tagged commit, so the tag has to move. Run [the five checks](#what-ci-checks) *before* tagging.
- **The release arrives as a draft.** `releaseDraft: true`, so a human publishes it. Until someone does, the in-app
  update check cannot see the release: it reads the releases API, and a draft is not there.
