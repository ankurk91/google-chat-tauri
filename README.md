# Google Chat (Tauri)

[![ci](https://github.com/ankurk91/google-chat-tauri/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/ankurk91/google-chat-tauri/actions/workflows/ci.yml)
[![release](https://github.com/ankurk91/google-chat-tauri/actions/workflows/release.yml/badge.svg)](https://github.com/ankurk91/google-chat-tauri/actions/workflows/release.yml)
[![tauri](https://img.shields.io/badge/built%20with-Tauri%20v2-24C8DB?logo=tauri&logoColor=white)](https://v2.tauri.app)
[![licence](https://img.shields.io/badge/licence-GPL--3.0--only-blue.svg)](LICENSE.txt)

An unofficial desktop app for [Google Chat](https://chat.google.com) on Linux, macOS and Windows.

It puts Chat in a real window with a tray icon, an unread indicator and native desktop notifications, instead of a
browser tab that gets lost among the others. The app uses your operating system's built-in web engine rather than
shipping its own, so the Linux installer is about 3 MB.

> Not affiliated with, endorsed by, or sponsored by Google. "Google Chat" and
> the Chat logo are trademarks of Google LLC.

## Features

- **Unread indicator** — a dot on the tray icon, the count in the window title, and a badge on the macOS dock or Windows
  taskbar.
- **Desktop notifications** — with sound. On Linux, clicking one brings the window back.
- **Lives in the tray** — closing the window hides it rather than quitting; the app keeps running and keeps notifying.
- **Remembers your window** — size, position and maximised state come back where you left them.
- **One instance** — launching again focuses the window you already have.
- **Menu bar** — File, Edit, View, History, Preferences and Help, with zoom that persists between launches.
- **Start with your session** — optionally launch at login, straight into the tray. Under **Preferences**.
- **Keyboard shortcuts** — `Ctrl+F` to search, `Ctrl` `+`/`-`/`0` to zoom,
  `Alt+←`/`Alt+→` to go back and forward, `Ctrl+W` to hide to the tray.
- **Links open in your browser** — a Docs, Sheets, Drive or Calendar link someone shares opens in your real browser,
  with your extensions and your other tabs. Only Chat itself stays in this window.
- **Attachments download through your browser** — clicking one hands the link to your browser, which saves it the way it
  saves anything else. Files the window fetches itself, such as **Save image as** from the right-click menu, go straight
  to your Downloads folder.
- **Signs in normally** — a personal Google account and a paid Google Workspace one both work, in any country: the
  sign-in hop through your local `accounts.google.*` domain stays inside the window instead of stranding you on a login
  page.

The app does not collect analytics and does not update itself. It does ask GitHub once a day or so whether a newer
release exists, and tells you if there is one — you download and install it yourself. That can be turned off in
**Preferences**.

**Sign-in that leaves Google is not supported** — an external identity provider, or SSO through Okta, Entra ID, Ping and
the like. Those flows redirect to a host belonging to your organisation, and the app cannot know that address ahead of
time, so it cannot be on the short list of hosts allowed to stay in the window. Use Chat in your browser instead.

## Supported systems

| OS      | Version                                                  | Architecture                        | Download            |
|---------|----------------------------------------------------------|-------------------------------------|---------------------|
| Linux   | glibc 2.39+ — Ubuntu 24.04, Mint 22, Debian 13 and newer | x86_64                              | `.deb`, `.AppImage` |
| macOS   | 10.15 Catalina and newer                                 | Apple silicon and Intel (universal) | `.dmg`              |
| Windows | 10 (1803+) and 11                                        | x64                                 | `.exe` installer    |

Nothing is built for 32-bit, ARM Linux, or Apple silicon separately from the universal build. Windows needs the WebView2
runtime, which is part of Windows 11 and is installed automatically by the installer on older systems.

The Linux bundles are built on Ubuntu 24.04, which sets the glibc floor; a binary built there runs on newer
distributions but not older ones, so 22.04 and Mint 21 are not supported.

On Linux the app is developed against Ubuntu/GNOME first and Linux Mint/Cinnamon second, and both X11 and Wayland are
supported. Two differences are worth knowing before you file a bug:

- **The dock counter is an Ubuntu feature.** Ubuntu shows the unread count on the dock icon; other desktops have no such
  API and show it in the window title and on the tray icon instead. All three are driven by the same count.
- **On Wayland, bringing the window back from the tray can take an extra click.** If the window is already open but
  behind something else, Wayland does not let an application raise itself, so GNOME offers a *"Google Chat is ready"*
  notification to click instead. Restoring from minimised, and clicking a message notification, both work normally.

## Install

Everything below comes from the [latest release](https://github.com/ankurk91/google-chat-tauri/releases).

### Linux — `.deb` (Debian, Ubuntu, Linux Mint)

```bash
sudo apt install ./google-chat-tauri_*_linux-amd64.deb
```

The leading `./` matters — without a path, `apt` looks for a package by that name in your repositories. Installing this
way pulls in the dependencies (`libwebkit2gtk-4.1-0`, `libgtk-3-0`, `libayatana-appindicator3-1`) in the same step; they
come from your distribution and are usually installed already.

Then launch **Google Chat** from your applications menu.

**Uninstall.** The package is named `google-chat`, not `google-chat-tauri`:

```bash
sudo apt remove google-chat          # keeps your session and settings
sudo apt purge google-chat           # also removes them
```

`purge` runs a removal script that deletes your session, your preferences and the launch-at-login entry — the same three
places listed under the AppImage below. Nothing is left behind and there is nothing to clean up by hand.

### Linux — `.AppImage` (any distribution)

An alternative if you would rather not install anything, or your distribution is not Debian-based. It is much larger
than the `.deb`, because it carries its own copy of the web engine instead of using the one your system already has:

```bash
chmod +x google-chat-tauri_*_linux-amd64.AppImage
./google-chat-tauri_*_linux-amd64.AppImage
```

**Uninstall.** There is no uninstall command, because nothing was installed — an AppImage is one file you downloaded and
ran. Deleting it removes the app, but not the data it wrote, which lives in the same places any Linux app's data does
and has to be removed by hand:

```bash
rm google-chat-tauri_*_linux-amd64.AppImage             # the app itself

rm -rf ~/.local/share/com.ankurk91.google-chat-tauri    # session, cookies, logs
rm -rf ~/.config/com.ankurk91.google-chat-tauri         # preferences, window size and position
rm -f ~/.config/autostart/'Google Chat.desktop'         # only if you turned on Launch at Login
```

If you use AppImageLauncher or `appimaged`, it will have added a menu entry of its own under
`~/.local/share/applications/`; remove that too, or let the tool do it.

### macOS and Windows

The macOS dmg is universal — Apple silicon and Intel.

The builds are unsigned, so your system will warn you on first launch — on macOS, right-click the app and choose
**Open**; on Windows, click through the SmartScreen warning.

To uninstall, drag the app out of **Applications** on macOS, or use **Add or remove programs** on Windows.

## Troubleshooting

Blank window, missing notifications, sign-in trouble: see
[docs/Troubleshooting.md](docs/Troubleshooting.md). **Help → Show Logs** opens the log folder, and **Help → Report an
Issue** opens a new issue with your version, platform and web engine already filled in.

## Contributing

See [docs/Development.md](docs/Development.md) for how to build and run it.

## How this was built

Vibe-coded with [Claude](https://claude.com/claude-code), which wrote the Rust, the JavaScript and these docs. A human
reviewed every line before it landed and tested the result on real hardware — which is where the platform quirks
recorded in [docs/Development.md](docs/Development.md) came from, since none of them are the sort of thing a model finds
by reading documentation.

## Licence

[GPL-3.0-only](LICENSE.txt).
