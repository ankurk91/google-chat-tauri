# Development

Built with [Tauri v2](https://v2.tauri.app): a Rust backend and the operating system's own web engine — WebKitGTK on
Linux, WKWebView on macOS, WebView2 on Windows.

This page gets you building and running, and explains how the pieces fit. It deliberately does not carry the platform
findings — the no-op APIs, the workarounds and what was measured to justify them. Those live in
[Notes.md](Notes.md), and are worth reading before you change behaviour one of them describes.

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
node scripts/chatjs-test.js            # chat.js against a stand-in page: no browser, no signed-in session
```

The three harnesses need the single-instance slot to themselves — stop `pnpm run dev` first, or the running app answers
instead of theirs. They observe X11 and so run the app under X11 whatever the session is; see "The harnesses only see
X11" in [Notes.md](Notes.md) for what that does and does not prove, and leave the machine alone while one runs — the
entry above it says what happens if you do not.

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
docs/Notes.md                  platform findings and workarounds, with the measurements behind them
docs/Troubleshooting.md        for people using the app, not building it
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

### The page-to-Rust bridge

`scripts/chatjs-test.js` runs it in a `vm` context against fakes for `document`, `window` and the Tauri bridge, and
reaches everything the way Chat does: `window.Notification`, `window.open`, and the listeners registered on `document`.
It covers the notification shim, link interception and the shortcut table without a browser or a signed-in session, and
CI runs it next to `node --check`. Two things it taught, both about the fakes rather than the app: a `vm` context has
the ECMAScript intrinsics and no web APIs, so `URL` has to be handed in or `isCrossOrigin` throws into its own catch and
calls every link same-origin; and a fake `location` needs `origin` as well as `href`, or every link looks external.


`src-tauri/src/inject/chat.js` is the equivalent of a preload script. It is injected into Google's page as a Tauri
initialization script and compiled into the binary with `include_str!`, which also makes cargo rebuild when you edit it.
It is injected twice — once at document start, and again from `on_page_load` as a fallback — so **everything in it must
be idempotent**.

It runs in the main frame only, and does six jobs: poll the unread count, intercept link clicks, translate keyboard
shortcuts, replace
`window.Notification`, listen for notification clicks, and rewrite the webview's own failed-load page into something
readable.

### The ACL — the part that is easy to get wrong

Tauri rejects `invoke` from a remote origin unless the command is named in **both** `permissions/chat-ipc.toml` and a
capability with a matching
`remote.urls`. Miss either and every call fails. Adding a command means editing three files: `commands.rs`, the
`invoke_handler!` list in `lib.rs`, and the permission file.

Treat that list as attack surface — it is callable by a page nobody here controls. Commands validate their own input,
and anything destructive stays out of it: `menu_action` has an allow-list that excludes quit and sign-out.

## Logs

Logs go to the platform log directory (`~/.local/share/com.ankurk91.google-chat-tauri/logs/` on Linux), reachable from
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

## Updates and the network

`features::updates` asks GitHub for the latest release, compares the tag against `CARGO_PKG_VERSION` with `semver`, and
if it is newer offers to open the release page in the browser. It downloads nothing and installs nothing — that is the
whole design. Tauri's own updater plugin was not used: it signs an artifact and swaps it in, which needs a key in CI and
on Linux only ever works for an AppImage, never the deb most people install. That is also why `uploadUpdaterJson` is off
in `release.yml`.

One thread does the scheduling: thirty seconds after launch, then every twelve hours, sleeping in between. An automatic
check is silent unless there is something new, and silent about a version it has already offered — `offered_version` in
the config file — so nobody gets the same dialog twice a day until they update. **Help → Check for Updates** ignores
that and always answers, because a manual check that appears to do nothing is indistinguishable from a broken one.
**Preferences → Check for Updates Automatically** turns the scheduled half off; the thread stays, and skips the request.

It asks `/releases` rather than `/releases/latest`, skips drafts and takes the highest version rather than the first
listed — all three for reasons in [Notes.md](Notes.md).

Pre-releases count as updates while the running version is itself pre-1.0 or carries a pre-release tag. The whole of
`0.x` is this app's pre-release era, and someone on 0.0.1 who is never told about 0.0.2 is not being served; once it
reaches 1.0.0, a stable user stops being offered betas. A `404` still means "nothing to offer" — a private repository, or
a typo in the URL.

The HTTP client is `ureq` with **native-tls**, so TLS comes from the platform — OpenSSL on Linux, which WebKitGTK
already pulls in, Schannel on Windows, Security.framework on macOS. The provider has to be named in the request config;
[Notes.md](Notes.md) says what happens if it is not.

`features::connectivity` is the other half. The window points at a remote page, so with no route to the internet the
webview shows a bare error page that says nothing about the app. One TCP connection to `chat.google.com:443` — no TLS,
no HTTP, nothing a captive portal can answer misleadingly — decides it. Launching at login races the network, so it
retries across about a minute (2, 4, 8, 15, 30 seconds; 84 seconds end to end, since every failed attempt also spends
its connect timeout) and then tells the user once, through the desktop's own notification.

After that it keeps looking, every thirty seconds, until the network answers — and then loads Chat. That poller only
exists because the error page in the window cannot retry itself (see "That page is a near-dead end" in
[Notes.md](Notes.md)), so without it, joining wifi
after launching offline leaves the app stuck on that page for as long as it stays open. It only ever starts when the
app launched with no network at all, and it stops on the first success, so the cost is one TCP connect twice a minute
for exactly as long as there is nothing to connect to.

## Debug-only affordances

In a debug build the tray gains **Demo Badge Count** and **Test Notification**, and a second launch doubles as a remote
control — the single-instance plugin hands the running process the new argv:

```bash
cargo build --manifest-path src-tauri/Cargo.toml
./src-tauri/target/debug/google-chat-tauri &
./src-tauri/target/debug/google-chat-tauri --test-notification
./src-tauri/target/debug/google-chat-tauri --test-activation
./src-tauri/target/debug/google-chat-tauri --test-reset
./src-tauri/target/debug/google-chat-tauri --test-update-check
./src-tauri/target/debug/google-chat-tauri --test-links-in-app
```

`--test-update-check` runs the check that would otherwise wait half a minute and then twelve hours, dialog and all.
`--test-links-in-app` clicks **Preferences → Open Every Link in This Window** for you, and
`GOOGLE_CHAT_LINK_GRANT_SECS=10` shortens the five-minute grant so the lapse and the menu unticking itself can be
watched inside one test run rather than one coffee break — also debug builds only, since an environment variable that
can quietly widen the link policy has no business in a release.
`GOOGLE_CHAT_PROBE_HOST=192.0.2.1:443` (TEST-NET-1, which never routes) makes the connectivity probe fail without
touching the machine's network, which is how the offline notification is tested — also debug builds only, since an
environment variable that can silently convince the app it is offline has no business in a release.

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

|                                           | Ubuntu 26.04 / GNOME 50.1 / **Wayland**          | Linux Mint 22.3 / Cinnamon / **X11** |
|-------------------------------------------|--------------------------------------------------|--------------------------------------|
| tray, menu, close-to-tray                 | yes                                              | yes                                  |
| notifications + click-through             | yes                                              | yes                                  |
| dock badge (`set_badge_count`)            | **yes** — Ubuntu Dock owns `com.canonical.Unity` | no — nothing owns the name           |
| numbered tray icon + title count          | yes                                              | yes                                  |
| raise from tray Toggle                    | **only from minimised** — see [Notes](Notes.md)  | yes                                  |
| Reset App Data, geometry, single instance | yes                                              | yes                                  |

Ubuntu is the primary target; Mint is the secondary one. The split above is a display-server difference rather than a
distribution one, so read "Wayland" wherever it says Ubuntu.

What is left, in the order it matters:

1. **macOS and Windows have both now been run, briefly.** A `release` run produces the dmg, the .app and the NSIS
   installer, and both have been launched on real hardware by the maintainer — not by anything in `scripts/`, which
   cannot reach either platform, so treat all of it as reported rather than machine-verified.

   macOS: a quick pass found nothing wrong, past the Gatekeeper block every unsigned build gets. Still unconfirmed
   there, because a quick pass would not touch them: the dock badge, and whether notifications arrive at all.

   Windows 11 has been launched and reported on (by the maintainer, on hardware this repo's harnesses cannot reach —
   none of the following is machine-verified here):

   | behaviour | result |
     |---|---|
   | tray left-click toggles the window | **works** — the one thing this list used to call unknown |
   | Edit menu's Undo/Redo | present, as muda's predefined items |
   | View → Toggle Full Screen | appeared and did nothing — muda draws it on Windows but does not implement it. Now macOS-only |
   | taskbar unread counter | not seen. Windows has no numeric badge for an unpackaged app; `badge::apply` sets a taskbar *overlay icon* instead, and whether it renders depends on **Settings → Personalization → Taskbar → Show badges**. Cross-check against the window title, which carries the same count |
   | Ctrl+Q, Ctrl+W | do nothing. WebView2 keeps the key, so the menu accelerator never fires — and `chat.js`'s forwarding cannot cover Ctrl+Q, because `menu_action` refuses `quit` from the page by design. Ctrl+R works, which is WebView2's own reload rather than the menu's |
   | Ctrl+F, zoom keys | untested — worth knowing, because they go through the same forwarding as Ctrl+W and would say whether the page receives *any* of these keys or whether Ctrl+W alone is reserved |

   wry can turn WebView2's accelerator handling off (`with_browser_accelerator_keys`), which would hand these keys to
   the page — but Tauri 2.11 does not plumb it through, so it is not reachable from here today.
2. **A notification click does not open the conversation, and cannot be made to.** It raises the window and stops there.
   This was open pending a look at a real payload; that has now happened, and the answer is that Chat has no
   per-conversation URL to navigate to and hangs no click handler — see the two notification entries in
   [Notes.md](Notes.md). Anything better needs
   Chat's own router, so treat this as closed rather than pending unless the service worker becomes reachable.
3. **The window cannot raise itself on Wayland** when it is visible but unfocused, so tray → Toggle produces GNOME's
   "window is ready" notification instead. Waiting on xdg-activation support in tao; deliberately not worked around, for
   the reasons in [Notes.md](Notes.md).
4. **Attachment links open in the system browser.** Deliberate for now — it works. `on_download` would keep them in-app:
   one line in `urls::is_in_app`.

**Deliberately not built:** auto-update that installs itself, a spellchecker toggle (no Tauri API), and single-click
tray toggle on Linux — the last would mean replacing Tauri's tray with a direct StatusNotifierItem backend, a parallel
implementation judged not worth it. Left-click opens the menu, with Toggle first.

## Releasing

Bump the version in `package.json`, `src-tauri/Cargo.toml` and
`src-tauri/tauri.conf.json`, then push a `v*` tag. Builds are unsigned, so both macOS and Windows block the first
launch; the way past each is in the release notes `release.yml` writes, and in Troubleshooting.md.
