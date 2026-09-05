# Google Chat

[![ci](https://github.com/ankurk91/google-chat-tauri/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/ankurk91/google-chat-tauri/actions/workflows/ci.yml)
[![release](https://github.com/ankurk91/google-chat-tauri/actions/workflows/release.yml/badge.svg)](https://github.com/ankurk91/google-chat-tauri/actions/workflows/release.yml)
[![licence](https://img.shields.io/badge/licence-GPL--3.0--only-blue.svg)](LICENSE.txt)

An unofficial desktop app for [Google Chat](https://chat.google.com) on Linux, macOS and Windows.

It puts Chat in a real window with a tray icon, an unread indicator and native desktop notifications, instead of a
browser tab that gets lost among the others. The app uses your operating system's built-in web engine rather than
shipping its own, so the Linux installer is about 2.5 MB.

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
- **Downloads go to your Downloads folder** — saving an image keeps the file locally instead of bouncing you to a
  browser.
- **Links open in your browser** — a Docs, Sheets, Drive or Calendar link someone shares opens in your real browser,
  with your extensions and your other tabs. Only Chat itself stays in this window.
- **Signs in normally** — including Google Workspace accounts, in any country.

The app does not collect analytics, does not phone home, and has no auto-updater.

## Supported systems

| OS      | Version                                                  | Architecture                        | Download         | State                               |
|---------|----------------------------------------------------------|-------------------------------------|------------------|-------------------------------------|
| Linux   | glibc 2.39+ — Ubuntu 24.04, Mint 22, Debian 13 and newer | x86_64                              | `.deb`           | Tested                              |
| Linux   | as above                                                 | x86_64                              | `.AppImage`      | Builds and launches; lightly tested |
| macOS   | 10.15 Catalina and newer                                 | Apple silicon and Intel (universal) | `.dmg`           | Builds; never launched              |
| Windows | 10 (1803+) and 11                                        | x64                                 | `.exe` installer | Builds; never launched              |

Nothing is built for 32-bit, ARM Linux, or Apple silicon separately from the universal build. Windows needs the WebView2
runtime, which is part of Windows 11 and is installed automatically by the installer on older systems.

The Linux bundles are built on Ubuntu 24.04, which sets the glibc floor; a binary built there runs on newer
distributions but not older ones, so 22.04 and Mint 21 are not supported.

## Install

### Linux (Debian, Ubuntu, Linux Mint)

Download the `.deb` from the
[latest release](https://github.com/ankurk91/google-chat-tauri/releases) and:

```bash
sudo apt install ./google-chat-tauri_*_linux-amd64.deb
```

The leading `./` matters — without a path, `apt` looks for a package by that name in your repositories. Installing this
way pulls in the dependencies (`libwebkit2gtk-4.1-0`, `libgtk-3-0`, `libayatana-appindicator3-1`) in the same step; they
come from your distribution and are usually installed already.

Then launch **Google Chat** from your applications menu.

Prefer something you can run without installing? The same release has an
`.AppImage`. It is much larger, because it carries its own copy of the web engine instead of using yours:

```bash
chmod +x google-chat-tauri_*_linux-amd64.AppImage
./google-chat-tauri_*_linux-amd64.AppImage
```

To uninstall:

```bash
sudo apt remove google-chat          # keeps your session and settings
sudo apt purge google-chat           # also removes them
```

### macOS and Windows

The macOS dmg is universal — Apple silicon and Intel.

Builds are produced but have had less testing than Linux. They are unsigned, so your system will warn you on first
launch — on macOS, right-click the app and choose **Open**.

## Troubleshooting

Blank window, missing notifications, sign-in trouble: see
[docs/Troubleshooting.md](docs/Troubleshooting.md). **Help → Show Logs** opens the log folder.

## Contributing

See [docs/Development.md](docs/Development.md) for how to build and run it.

## Licence

[GPL-3.0-only](LICENSE.txt).
