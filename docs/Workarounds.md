# Workarounds

Code in this repo that looks wrong, redundant or over-complicated until you know what it is working around. Each entry
below is one deliberate deviation from the obvious implementation. **Read the entry before changing the code it names**
— the obvious simplification has usually been tried already.

The measurements and platform behaviour behind these are in [Notes.md](Notes.md).

## The page

### Link policy lives in `chat.js`, not in `on_navigation`

`on_navigation` looks like the natural place for a host allow-list, and it is the wrong one: wry's WebKitGTK backend
fires it for every frame, so an allow-list there rejects legitimate third-party iframes. The injected script runs in the
main frame only, which is exactly the granularity the policy needs.

### `chat.js` installs three different amounts of itself

An initialization script is attached to the webview, not to a page: Tauri runs it at document-start on every top-level
navigation, and this window goes well beyond Chat — a country sign-in domain, an employer's identity provider while the
link grant is open, a marketing page after a sign out, the webview's own failed-load document. So the boot block at the
end of the file branches on `location.origin`, which is the same thing Tauri's ACL keys on:

| where | what runs |
|---|---|
| `mail.google.com`, `chat.google.com` | all of it |
| no origin at all (`"null"`) | the failed-load page rewrite, and nothing else |
| anywhere else | link hand-off, and nothing else |

Off Chat the bridge refuses every call, so the shortcuts would be swallowed and then refused, the notification shim
would promise a permission it cannot honour, and the poller would scrape a page with no unread count in it. None of that
is a security boundary — the ACL is, and it holds regardless — it is about not rearranging pages the app does not own.
Third-party sign-in is the case that made it matter: an identity provider's own click handler is where the sign-in
happens, and the interceptor used to call `stopPropagation()` on it.

### `handOff` navigates the window itself when `invoke` is refused

Link hand-off is the one thing that still runs off Chat, and only for a link that asks for a second window — `_blank`,
or `window.open`. wry has no handler to give it one, so without this nothing happens at all and the link looks dead;
worst case, signed out on a Google marketing page where "Sign in" is the only way back. The ACL refuses the hand-off on
those origins, so `handOff` falls back to navigating this window. Nothing is given up by it: the allow-list exists to
keep links shared *inside Chat* out of this window, and off the Chat origins there are none.

An ordinary cross-origin link is *not* taken off those pages. The page can follow it perfectly well by itself, and
taking it costs the page its own click handler for nothing.

### `urls::is_accounts_host` accepts country domains

Sign-in hops through `accounts.google.co.in/SetSID` and its equivalents. Treat one of those as an external link and the
system browser finishes the login instead of the app.

### `features::user_agent` spoofs Firefox

Google serves a degraded experience to browsers it does not recognise, and `accounts.google.com` is actively hostile to
them; WebKitGTK identifies as Safari on Linux. `GOOGLE_CHAT_UA` overrides the spoof without a rebuild, which is the
first thing to reach for when sign-in misbehaves.

### The has-unread flag comes from the favicon, not the DOM

Chat does not render its navigation while the window is hidden, so the elements the unread count scrapes are simply
absent and the count reads zero — precisely when the tray is the only thing the user can see. The favicon is driven by
data rather than layout, and Google publishes matching `..._no_dot_` and `..._dot_` variants to swap between.

### Everything in `chat.js` must be idempotent

It is injected twice: once at document start, and again from `on_page_load` as a fallback. Anything that appends,
increments or registers unconditionally will do so twice.

## Notifications

### `window.Notification` is replaced wholesale, and Linux talks to `notify-rust` directly

The web API is unusable in all three webviews, so there is nothing to fall back to. Linux bypasses Tauri's notification
plugin as well, because that plugin's click API is mobile-only and a click is the whole point here.

### Notifications are delivered on one long-lived worker thread

`show_notification` is a synchronous Tauri command and Tauri runs those on the main thread — which on Linux is also the
thread drawing the window's own titlebar buttons. `notify_rust`'s `show()` round-trip costs up to 520 ms, so every
notification used to stall those buttons for that long and a burst compounded it. `features::notifications::show_linux`
hands the work to `deliver` on a worker instead.

It is deliberately *one* worker rather than a thread per notification: a burst then neither spawns threads unboundedly
nor lets popups reach the daemon out of order, which serialising on the main thread used to give for free.

### The click-waiter thread is named explicitly

Linux gives a new thread the creating thread's name, so the waiter spawned from the worker would inherit
`notifications` and the process would show two threads by that name — only one of them the worker. That is confusing at
exactly the moment you are reading a thread list to explain a freeze.

### `notifications::activated` raises the window, pauses, then tells the page

A hidden window is inert, not merely invisible: Chat's router does nothing until the page has painted. The ordering and
the pause are both load-bearing.

### A notification click raises the window and stops there

Chat's notifications carry no click handler that the page can reach, and there is no per-conversation URL to navigate to
instead — both properties of Chat rather than gaps in this app. `chat.js` still falls back to any Chat link in the
payload, and logs what each notification carries at `debug` so the next person can check whether that has changed.

### `GOOGLE_CHAT_NOTIFICATION_ACTIONS=0` exists

If a desktop's notification service invoked the `default` action on expiry rather than on a click, the window would rise
by itself after every message. The switch is there to prove or disprove that on a suspect desktop without a rebuild.

## Keyboard and menus

### `features::accelerators` asks WebView2 for Ctrl+W and Ctrl+Q directly (Windows only)

Menu accelerators never reach tao's message loop on Windows, so the accelerator text next to those two items was
decoration. Page-side forwarding cannot serve them either: forwarding only works on an origin the capability names, so
Ctrl+W died on the sign-in page, and `menu_action` refuses `quit` from the page on purpose. Taking the keys before the
page — `AcceleratorKeyPressed` on the controller Tauri hands out — fixes both and widens the IPC surface by nothing.

### `chat.js` forwards shortcuts the menu already claims

This looks redundant, and on Linux it is: GTK consumes an accelerator before the webview sees the key, so the page never
gets a keydown for anything the menu claims and the two paths cannot double-fire. Leave it anyway — Windows is the other
way round, and this table is the only thing running there.

The corollary matters when adding items: any *new* menu accelerator takes that key away from the page. Ctrl+F stays in
`chat.js` precisely because no menu item claims it.

### Ctrl+F expands Chat's collapsed search first, on a wall-clock budget

Chat collapses search to a button and leaves the input in the DOM behind it, invisible — so looking the input up
directly fails in exactly the state the key is pressed in. Expanding means clicking Chat's own button, and then waiting:
the box takes around 300 ms to draw on a software-rendered desktop, and Chat closes it again if nothing inside gains
focus. A retry budget counted in animation frames runs out first, which is why the budget is wall-clock. A frame is not
a unit of time.

The button is identified as the visible one inside the `[role="search"]` landmark rather than by `aria-label`, which is
English here and something else wherever the app is used in another language.

### Edit → Undo and Redo are custom items driving `document.execCommand`

muda's predefined Undo and Redo are documented Unsupported on Linux, so asking for them adds nothing and the Edit menu
opens with Cut.

They deliberately carry no accelerator. Claiming Ctrl+Z would take the webview's own working undo away and route it
through `execCommand`, which cannot reach an editable inside a cross-origin frame — a clear downgrade.

### Fullscreen is asked for on macOS only

muda draws the item on Windows and it does nothing when clicked, which is worse than not offering it. Linux renders
nothing at all.

### `app_menu::nested_check_item` walks the submenus by hand

`Menu::get` searches the top level only, so every check item under Preferences is invisible to it. Rebuilding the whole
menu with `set_menu` does get every tick right, but GTK answers it with one *"no accelerator installed in accel group"*
warning per accelerator, every time. Walking by hand avoids both. This is also why the Preferences toggles keep their
own state rather than reading a tick back — the link grant is the one setting that changes without a click, so it is the
one that needs to clear its own tick.

## Windows, restart and reset

### `window::show_and_focus` hides a minimised window before showing it

Cinnamon will not deiconify: `unminimize()` reaches `gtk_window_deiconify` and the window stays iconic however often it
is asked. Hiding and showing re-maps it in the normal state. Focus is then asked for a second time, because tao refuses
to focus a window it still believes is minimised and only learns otherwise when the window manager confirms — which is
after the first call has returned.

### …and does not do the same for a visible, unfocused window on Wayland

Tempting, since the hide-then-show demonstrably gets focus today. Don't. It works only because compositors still treat a
newly mapped window leniently, and that leniency is exactly what they are tightening. It would also cost real behaviour
meanwhile: hiding a *visible* window makes Chat's page inert and flashes the user. The gap — tray → Toggle on a visible,
unfocused window produces GNOME's "window is ready" notification instead of a raise — is a documented limitation waiting
on xdg-activation support in tao, not an oversight.

### `features::reset` deletes at the top of the *next* launch

WebKit's storage cannot be deleted from under a live webview: `clear_all_browsing_data` is asynchronous, and the network
process writes the cookie jar out again as it shuts down, so a reset-then-restart leaves the user still signed in. The
reset drops a sentinel and does the deleting before any plugin or webview has opened those files.

### `request_restart` exits through `RunEvent::Exit`, not `AppHandle::restart`

`AppHandle::restart` spawns the replacement *before* plugin shutdown, so the new process finds the single-instance name
still held, hands its argv to the process on its way out, and exits — leaving nothing running at all. It also never
returns, which deadlocks a caller on a plugin thread. The replacement path needs the same `quitting` flag as Quit and
close-to-tray, or the window-close veto cancels it.

## Offline and sign-in

### `chat.js` rewrites the webview's failed-load page

WebKitGTK builds that page as `<html><body>%s</body></html>` — the whole template — so the one line explaining the
failure renders as black text on the window's dark grey background. wry exposes no `did-fail-load` equivalent for Rust
to hang a replacement on, so the page-side script does it.

The fingerprint it matches — empty head, a body with text and no elements — is narrow on purpose: WKWebView leaves the
document empty and WebView2 draws its own styled page, so neither is touched.

### **Try again** is a `<button>` aimed at Chat's canonical trailing-slash URL

Two constraints, both from the same page. Its origin is opaque, so `isCrossOrigin` calls every link on it external and
the interceptor silently swallows an ordinary anchor — a `<button>` with a handler sidesteps that. And WebKit refuses to
navigate the stand-in document to the URL it is standing in for, while any *differently spelled* URL navigates
immediately; Google answers `/chat/u/0` with a 302 to `/chat/u/0/`, so the trailing slash is a spelling that works.

### `features::connectivity` keeps polling after it has reported offline

The error page cannot retry itself, so without the poller, joining wifi after an offline launch leaves the app stuck on
that page for as long as it stays open. It only ever starts when the app launched with no network at all and stops on
the first success, so the cost is one TCP connect twice a minute for exactly as long as there is nothing to connect to.

### `features::sign_in` sends its redirect from a spawned thread

`navigate` from inside `on_page_load` re-enters the webview: `send_user_message` dispatches inline when it is already on
the main thread, and `on_page_load` *is* the main thread, inside WebKit's own `load-changed` handler.
`run_on_main_thread` is no escape — it goes through the same function. A spawned thread routes the navigation through
the event loop, which runs it once the load has settled.

### …and redirects at most twice in a row

A sign-out can land on a Google advertisement that **History → Go to Chat** only bounces off, so the redirect to
`accounts.google.com/ServiceLogin` is worth having. But a redirect loop would be worse than the dead end, and the dead
end is clickable now anyway, so the guard gives up and warns.

## Updates

### `features::updates` asks `/releases`, skips drafts, and takes the highest version

Not `/releases/latest`, which 404s for a repository that has only ever pre-released — indistinguishable from having
never released anything. And not the first entry either: GitHub orders `/releases` by creation date, so a patch to an
older line can be published after, and appear ahead of, a newer release.

### The HTTP client names its TLS provider explicitly

ureq defaults to Rustls and, if that is not the compiled feature, neither falls back nor errors — it *panics, mid
request*. `native-tls` is the feature here, so TLS comes from the platform: OpenSSL on Linux, which WebKitGTK already
pulls in, Schannel on Windows, Security.framework on macOS.

### Nothing is downloaded or installed

Tauri's own updater plugin signs an artifact and swaps it in, which needs a signing key in CI and on Linux only ever
works for an AppImage, never the deb most people install. This app opens the release page in the browser instead.
`uploadUpdaterJson` is off in `release.yml` for the same reason.

### `tauri-plugin-log` gets both targets in one `targets()` call

It starts with a stdout target *and* a log-directory target already. Adding them with `target()` leaves the defaults in
place and writes every line twice.

## Deliberately not built

Auto-update that installs itself; a spellchecker toggle, for which Tauri exposes no API; and single-click tray toggle on
Linux — the GTK tray backend emits no click events at all, so that would mean replacing Tauri's tray with a direct
StatusNotifierItem backend, a parallel implementation judged not worth it. Left-click opens the menu, with Toggle first.

Attachment links open in the system browser, which works. `on_download` would keep them in-app: one line in
`urls::is_in_app`.
