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

If that fixes it, make it permanent. The `.deb` installs its launcher to `/usr/share/applications/`, which an update
overwrites, so copy it somewhere that belongs to you first and edit the copy:

```bash
cp /usr/share/applications/'Google Chat.desktop' ~/.local/share/applications/
```

Then put `env WEBKIT_DISABLE_DMABUF_RENDERER=1` at the front of that copy's `Exec=` line.

**Notifications do not appear.** They come from your desktop's own notification service, so check Chat's in-app
notification settings first (**⚙ Settings → Notifications**), then your desktop's Do Not Disturb.

**The window pops up on its own after a notification.** Your desktop's notification service is reporting notifications
as clicked when they expire. Turn off the clickable action:

```bash
GOOGLE_CHAT_NOTIFICATION_ACTIONS=0 google-chat-tauri
```

**Clicking a notification does not open that conversation.** It brings the app to the front and leaves you wherever you
were. Google Chat does not tell the app which conversation a notification belongs to, and has no per-conversation
address to jump to, so there is nothing to act on — this is a limitation of Chat rather than a setting you can change.

**"Google Chat is ready" appears instead of the window (Wayland).** Using **Toggle** in the tray while the window is
already open but behind something else produces a notification rather than the window. Wayland does not permit an
application to raise itself, so your desktop offers the notification instead; click it to get the window. Minimising
first, or clicking a message notification, both bring it back directly. On an X11 session this does not happen.

**It opens a Google or Gmail marketing page instead of Chat.** That is what Google serves a signed-out browser, so the
app is not broken — its session is gone. If you had just used **Help → Reset App Data** or **File → Sign Out**, that is
exactly what those do. The app should take itself to the sign-in form within a second; if it does not, the **Sign in**
link on the page works, and so does **History → Go to Chat**. You should never have to wipe the app's data to get back
in — if you do, that is a bug worth reporting.

**It says Google Chat is out of reach.** The connection was not there when the app started. Leave it open — the app
checks every half minute and loads Chat by itself once the network is back, usually before you get round to the
**Try again** button.

**"No internet connection" when you are online.** The app tries to reach
`chat.google.com` for about a minute after it starts and says so if nothing
answers. A VPN, a proxy or a captive portal that blocks direct connections can
produce this while a browser still works. It is only a message — the window
loads Chat as soon as the connection is there.

**Sign-in goes through your company's own login page (Okta, Entra ID, Ping).** Those hosts are not on the short list
the app keeps in its own window, so a link out to one would open in your browser and finish the sign-in there instead.
Turn on **Preferences → Open Every Link in This Window** first: for the next five minutes every link stays in the app,
which is long enough to get through the flow. It switches itself back off, and you can untick it as soon as you are
signed in.

**Signed out unexpectedly, or sign-in loops.** Quit from the tray, remove
`~/.local/share/com.ankurk91.google-chat-tauri`, and start again. That clears the app's stored session without touching
your browser.
