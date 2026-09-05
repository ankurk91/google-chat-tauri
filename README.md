# Google Chat

An unofficial desktop app for [Google Chat](https://chat.google.com) on Linux,
macOS and Windows.

It puts Chat in a real window with a tray icon, an unread indicator and native
desktop notifications, instead of a browser tab that gets lost among the others.
The app uses your operating system's built-in web engine rather than shipping
its own, so the Linux installer is about 2.5 MB.

> Not affiliated with, endorsed by, or sponsored by Google. "Google Chat" and
> the Chat logo are trademarks of Google LLC.

## Features

- **Unread indicator** — a dot on the tray icon, the count in the window title,
  and a badge on the macOS dock or Windows taskbar.
- **Desktop notifications** — with sound. On Linux, clicking one opens the
  conversation it came from.
- **Lives in the tray** — closing the window hides it rather than quitting;
  the app keeps running and keeps notifying.
- **Remembers your window** — size, position and maximised state come back
  where you left them.
- **One instance** — launching again focuses the window you already have.
- **Menu bar** — File, Edit, View, History, Preferences and Help, with zoom that
  persists between launches.
- **Start with your session** — optionally launch at login, straight into the
  tray. Under **Preferences**.
- **Keyboard shortcuts** — `Ctrl+F` to search, `Ctrl` `+`/`-`/`0` to zoom,
  `Alt+←`/`Alt+→` to go back and forward, `Ctrl+W` to hide to the tray.
- **Downloads go to your Downloads folder** — saving an image keeps the file
  locally instead of bouncing you to a browser.
- **Links open in your browser** — a Docs, Sheets, Drive or Calendar link
  someone shares opens in your real browser, with your extensions and your
  other tabs. Only Chat itself stays in this window.
- **Signs in normally** — including Google Workspace accounts, in any country.

The app does not collect analytics, does not phone home, and has no
auto-updater.

## Install

### Linux (Debian, Ubuntu, Linux Mint)

Download the `.deb` from the
[latest release](https://github.com/ankurk91/google-chat-tauri/releases) and:

```bash
sudo dpkg -i google-chat_*_amd64.deb
```

Then launch **Google Chat** from your applications menu.

Dependencies (`libwebkit2gtk-4.1-0`, `libgtk-3-0`,
`libayatana-appindicator3-1`) come from your distribution and are almost always
already installed. If `dpkg` reports any as missing:

```bash
sudo apt --fix-broken install
```

To uninstall:

```bash
sudo apt remove google-chat
```

### macOS and Windows

Builds are produced but have had less testing than Linux. They are unsigned, so
your system will warn you on first launch — on macOS, right-click the app and
choose **Open**.

## Troubleshooting

If you need to report a problem, **Help → Show Logs** opens the folder
containing the app's log file.

**The window is blank or black.** Some graphics drivers do not get on with the
Linux web engine. Launch it once with rendering acceleration off to check:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=1 google-chat-tauri
```

If that fixes it, make it permanent by adding the variable to the `Exec=` line
in `~/.local/share/applications/`.

**Notifications do not appear.** They come from your desktop's own notification
service, so check Chat's in-app notification settings first
(**⚙ Settings → Notifications**), then your desktop's Do Not Disturb.

**The window pops up on its own after a notification.** Your desktop's
notification service is reporting notifications as clicked when they expire.
Turn off the clickable action:

```bash
GOOGLE_CHAT_NOTIFICATION_ACTIONS=0 google-chat-tauri
```

**Signed out unexpectedly, or sign-in loops.** Quit from the tray, remove
`~/.local/share/com.ankurk91.google-chat-tauri`, and start again. That clears
the app's stored session without touching your browser.

## Contributing

See [docs/Development.md](docs/Development.md) for how to build and run it.

## Licence

[GPL-3.0-only](LICENSE.txt).
