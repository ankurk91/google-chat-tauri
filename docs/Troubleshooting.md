# Troubleshooting

Problems people hit while using the app, and what to do about them. For building and hacking on it,
see [Development.md](Development.md).

## Reporting a problem

**Help → Show Logs** opens the folder holding the log file. Every run starts with the app version, your platform and
distribution, the web engine and its version, and your desktop session — attach that block to any report, it answers
most of the first round of questions.

## Common problems

**The window is blank or black.** Some graphics drivers do not get on with the Linux web engine. Launch it once with
rendering acceleration off to check:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=1 google-chat-tauri
```

If that fixes it, make it permanent by adding the variable to the `Exec=` line in `~/.local/share/applications/`.

**Notifications do not appear.** They come from your desktop's own notification service, so check Chat's in-app
notification settings first (**⚙ Settings → Notifications**), then your desktop's Do Not Disturb.

**The window pops up on its own after a notification.** Your desktop's notification service is reporting notifications
as clicked when they expire. Turn off the clickable action:

```bash
GOOGLE_CHAT_NOTIFICATION_ACTIONS=0 google-chat-tauri
```

**It opens a Google or Gmail marketing page instead of Chat.** That is what
`chat.google.com` serves to a signed-out browser, so the app is not broken — its session is gone. Sign in again from
that page. If you had just used **Help → Reset App Data**, that is exactly what it does: signs you out.

**Signed out unexpectedly, or sign-in loops.** Quit from the tray, remove
`~/.local/share/com.ankurk91.google-chat-tauri`, and start again. That clears the app's stored session without touching
your browser.
