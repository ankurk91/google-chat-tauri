/*
 * Injected into the remote Google Chat page.
 *
 * This is the Tauri equivalent of electron's src/preload/*.ts. It is injected
 * twice on purpose -- once as an initialization script (document-start, the
 * real mechanism) and again from on_page_load(Finished) as a fallback -- so
 * everything below must be idempotent per document.
 *
 * No bundler and no build step: what is written here is what runs, so it has to
 * be what every supported webview already understands. That is ES2015-2018 --
 * const, let, arrow functions, classes, template literals, Map, for..of. It is
 * *not* `?.` or `??`: those need Safari 13.1 and the macOS floor is 10.15.0,
 * which shipped Safari 13.0.
 */
(function () {
  'use strict';

  if (window.__gchat_init) return;
  window.__gchat_init = true;

  // Main frame only. Tauri injects initialization scripts into the main frame
  // on Linux/macOS, but "Windows: scripts are always added to subframes" -- and
  // none of this (unread counts, link policy, notifications) makes sense inside
  // one of Google's embedded iframes.
  if (window.top !== window.self) return;

  const POLL_MS = 1000;

  /* ---------------------------------------------------------------- bridge */

  // Errors here are worth seeing. Swallowing them silently makes an ACL
  // rejection (which is what happens on any origin outside the capability's
  // remote.urls) indistinguishable from "the script never ran". Report the
  // first failure per command, then go quiet so a polling loop cannot spam.
  const reportedFailures = new Set();

  function invoke(command, args) {
    const tauri = window.__TAURI__;
    const invokeFn =
      (tauri && tauri.core && tauri.core.invoke) ||
      (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke);
    if (!invokeFn) return Promise.reject(new Error('tauri ipc not ready'));

    return invokeFn(command, args || {}).catch((err) => {
      if (!reportedFailures.has(command)) {
        reportedFailures.add(command);
        console.error(`[gchat] invoke(${command}) failed:`, err);
      }
      throw err;
    });
  }

  const ignore = () => {};

  const log = (level, message) =>
    invoke('page_log', { level, message: String(message) }).catch(ignore);

  // Ordering against Tauri's own init scripts is not guaranteed, so wait for
  // the IPC internals rather than assuming they are already there.
  function whenReady(callback) {
    if (window.__TAURI_INTERNALS__) return callback();

    let tries = 0;
    const timer = setInterval(() => {
      if (window.__TAURI_INTERNALS__ || ++tries > 100) {
        clearInterval(timer);
        if (window.__TAURI_INTERNALS__) callback();
      }
    }, 50);
  }

  /* ------------------------------------------------- unread message counter */
  /* Ported verbatim from electron src/preload/unreadCount.ts */

  const UNREAD_SELECTORS = [
    'div[data-tooltip="Chat"][role="group"]',
    'div[data-tooltip="Spaces"][role="group"]'
  ].join(',');

  function readUnreadCount() {
    const groups = document.body ? document.body.querySelectorAll(UNREAD_SELECTORS) : [];
    let total = 0;

    for (const group of groups) {
      const heading = group.querySelector('span[role="heading"]');
      const badge = heading && heading.nextElementSibling;
      if (!badge) continue;

      const count = Number(badge.textContent);
      if (!isNaN(count)) total += count;
    }
    return total;
  }

  /* Google swaps the favicon between two published variants --
   * "..._no_dot_64px.png" when everything is read and "..._dot_64px.png" when
   * something is not. That is a far more dependable signal than the DOM:
   *
   *  - it survives the window being hidden to the tray, which is this app's
   *    main use case. Chat does not render its navigation while the window is
   *    unmapped, so the selectors below find nothing and the count silently
   *    reads zero -- exactly when the tray is the only thing you can see.
   *  - it does not depend on Google's internal markup staying still.
   *
   * The favicon only says whether there is anything unread, not how many, so
   * both signals are reported: the favicon drives the tray, the count fills in
   * the window title when the DOM is available. */

  function readHasUnread() {
    const icon = document.querySelector('link[rel~="icon" i]');
    const href = (icon && icon.href) || '';
    if (!href) return null; // unknown -- do not overwrite what we last knew
    return /_dot_/.test(href) && !/_no_dot_/.test(href);
  }

  let lastCount = -1;
  let lastHasUnread = null;

  function pollUnread() {
    const count = readUnreadCount();
    let hasUnread = readHasUnread();
    if (hasUnread === null) hasUnread = lastHasUnread === null ? count > 0 : lastHasUnread;

    if (count === lastCount && hasUnread === lastHasUnread) return;
    lastCount = count;
    lastHasUnread = hasUnread;

    invoke('set_unread_count', { count, hasUnread }).catch(ignore);
  }

  /* ------------------------------------------------------- external links */
  /* Ported from electron src/main/features/externalLinks.ts, which used
   * setWindowOpenHandler. Tauri has no equivalent hook for window.open, so the
   * interception happens here and the policy decision stays in Rust. */

  function handOff(url) {
    if (!url) return;
    invoke('open_external_url', { url: String(url) }).catch(ignore);
  }

  const nativeOpen = window.open;
  window.open = function (url) {
    log('info', `window.open intercepted: ${url}`);
    handOff(url);
    // Returning null makes some Google flows throw; hand back an inert stub.
    return {
      closed: false,
      close: ignore,
      focus: ignore,
      blur: ignore,
      postMessage: ignore,
      document: null,
      location: { href: url || '' }
    };
  };
  window.open.__gchat_native = nativeOpen;

  function isCrossOrigin(href) {
    try {
      const target = new URL(href, location.href);
      if (target.protocol !== 'http:' && target.protocol !== 'https:') return false;
      return target.origin !== location.origin;
    } catch (err) {
      return false;
    }
  }

  document.addEventListener(
    'click',
    (event) => {
      let anchor = event.target;
      while (anchor && anchor.tagName !== 'A') anchor = anchor.parentElement;
      if (!anchor || !anchor.href) return;

      const opensNewWindow = anchor.target === '_blank' || anchor.target === '_new';
      if (!opensNewWindow && !isCrossOrigin(anchor.href)) {
        // Same-origin in-page navigation: Chat's own SPA routing. Leave it be.
        return;
      }

      event.preventDefault();
      event.stopPropagation();
      log('info', `link intercepted: ${anchor.href}`);
      // Rust decides whether this opens in the system browser or navigates the
      // main window -- one source of truth for the allow-list.
      handOff(anchor.href);
    },
    true
  );

  /* --------------------------------------------------- keyboard shortcuts */
  /* Every shortcut lives here rather than as a menu accelerator, because GTK
   * menu accelerators are not delivered while focus is inside the WebKitGTK
   * webview -- measured: Ctrl+Plus produced no menu event at all. Menu *clicks*
   * still work; this is only about the keyboard.
   *
   * Ctrl+F is handled locally (it just focuses an input). The rest are
   * forwarded to Rust, which owns zoom persistence and navigation. */

  const isVisible = (element) =>
    !!(element.offsetWidth || element.offsetHeight || element.getClientRects().length);

  function focusSearch() {
    const search = document.querySelector('input[name="q"]');
    if (search && isVisible(search)) {
      search.focus();
      return true;
    }
    return false;
  }

  function shortcutFor(event) {
    const mod = event.ctrlKey || event.metaKey;
    const key = String(event.key).toLowerCase();

    if (mod && !event.altKey && !event.shiftKey) {
      if (key === 'f') return 'search';
      if (key === '=' || key === '+') return 'zoom-in';
      if (key === '-' || key === '_') return 'zoom-out';
      if (key === '0') return 'zoom-reset';
      if (key === 'w') return 'close-to-tray';
    }
    // Ctrl+Shift+= is how "+" arrives on many layouts.
    if (mod && event.shiftKey && !event.altKey && (key === '+' || key === '=')) return 'zoom-in';

    if (event.altKey && !mod && !event.shiftKey) {
      if (key === 'arrowleft') return 'back';
      if (key === 'arrowright') return 'forward';
    }
    return null;
  }

  document.addEventListener(
    'keydown',
    (event) => {
      const action = shortcutFor(event);
      if (!action) return;

      if (action === 'search') {
        // Only swallow the key if there is actually a search box to focus,
        // so Chat's own find-in-page behaviour is not broken when there isn't.
        if (focusSearch()) {
          event.preventDefault();
          event.stopPropagation();
        }
        return;
      }

      event.preventDefault();
      event.stopPropagation();
      invoke('menu_action', { action }).catch(ignore);
    },
    true
  );

  /* ------------------------------------------------------ notifications */
  /* Replaces electron src/preload/overrideNotifications.ts.
   *
   * Electron only had to *wrap* window.Notification, because Chromium
   * implements it. None of the three system webviews can be used directly:
   * WebKitGTK denies permission (measured: requestPermission() -> "denied",
   * because Tauri 2.11 cannot handle WebKitWebView::permission-request),
   * WKWebView has no Notification API at all, and WebView2 drops notifications
   * unless the host handles NotificationReceived. So this replaces the API
   * wholesale and forwards to Rust.
   *
   * Reporting "granted" is what makes it work: Chat only ever asks the shim. */

  let notifySeq = 0;
  const liveNotifications = new Map();

  // Notification objects are kept so a click can be dispatched back onto the
  // one Chat created. Chat does not reliably call close(), and this app runs
  // for days, so without a cap the map grows for every message ever received.
  // Anything older than this is far past the point where clicking its
  // notification is possible -- the desktop stopped showing it long ago.
  const MAX_LIVE_NOTIFICATIONS = 50;

  function rememberNotification(notification) {
    liveNotifications.set(notification._id, notification);

    // Ids increment, so anything at or below the cutoff is stale. Only the
    // boundary is checked each time; earlier ones were dropped on their turn.
    const cutoff = notification._id - MAX_LIVE_NOTIFICATIONS;
    if (cutoff > 0) liveNotifications.delete(cutoff);
  }

  // What a notification carries, at `debug` -- so it is in a development log and
  // never in a release one, where the level is `info`.
  //
  // Chat's own object carries no click handler, so the payload is the only place
  // a conversation could be named. Values are reported only when they look like
  // an id or a URL: a message body is not something to write to a log file.
  const IDISH = /^[\w:@.\-\/?=&+%#]{1,160}$/;

  const redact = (text) => (IDISH.test(text) ? text : '<text>');

  function summarise(value) {
    if (typeof value === 'string') return redact(value);
    if (value === null || typeof value !== 'object') return String(value);
    if (Array.isArray(value)) return `<array:${value.length}>`;
    return `<object:${Object.keys(value).join('|')}>`;
  }

  function describe(notification) {
    const parts = [`notification created: id=${notification._id} source=${notification._source}`];
    if (notification.tag) parts.push(`tag=${redact(notification.tag)}`);

    const data = notification.data;
    if (data && typeof data === 'object') {
      for (const [key, value] of Object.entries(data)) {
        parts.push(`data.${key}=${summarise(value)}`);
      }
    } else if (typeof data === 'string') {
      parts.push(`data=${redact(data)}`);
    }

    log('debug', parts.join(' '));
  }

  /* Chat calls this with `new`, so it has to be constructible -- a class is,
   * an arrow function is not. `source` is ours: Chat passes two arguments, the
   * service worker shim below passes a third. Which path a notification came
   * through decides whether a click has anything to dispatch to. */
  class GChatNotification {
    constructor(title, options, source) {
      options = options || {};

      // Underscored because these ride on an object Google's own code holds:
      // the standard Notification interface has no `_id`, and nothing of ours
      // should collide with a field Chat decides to set later.
      this._id = ++notifySeq;
      this._source = source || 'page';
      this._listeners = { click: [], close: [], show: [], error: [] };

      this.title = String(title);
      this.body = options.body || '';
      this.icon = options.icon || '';
      this.tag = options.tag || '';
      this.data = options.data;
      this.onclick = null;
      this.onclose = null;
      this.onshow = null;
      this.onerror = null;

      rememberNotification(this);
      describe(this);

      invoke('show_notification', {
        id: this._id,
        title: this.title,
        body: options.body || null
      }).catch(ignore);
    }

    addEventListener(type, handler) {
      if (this._listeners[type] && typeof handler === 'function') {
        this._listeners[type].push(handler);
      }
    }

    removeEventListener(type, handler) {
      const list = this._listeners[type];
      if (!list) return;

      const at = list.indexOf(handler);
      if (at !== -1) list.splice(at, 1);
    }

    close() {
      liveNotifications.delete(this._id);
      this._dispatch('close');
    }

    // Returns how many handlers ran, which is the only way to tell a click that
    // Chat acted on from one that went nowhere.
    _dispatch(type) {
      const event = {
        type,
        target: this,
        currentTarget: this,
        preventDefault: ignore,
        stopPropagation: ignore
      };

      const handlers = [];
      if (typeof this[`on${type}`] === 'function') handlers.push(this[`on${type}`]);
      handlers.push(...(this._listeners[type] || []));

      for (const handler of handlers) {
        try {
          handler.call(this, event);
        } catch (err) {
          console.error(`[gchat] notification ${type} handler threw:`, err);
        }
      }

      return handlers.length;
    }

    static requestPermission(callback) {
      if (typeof callback === 'function') callback('granted');
      return Promise.resolve('granted');
    }
  }

  GChatNotification.permission = 'granted';
  GChatNotification.maxActions = 0;

  window.Notification = GChatNotification;

  // Chat may deliver notifications through a service worker rather than
  // constructing them directly; route those to the same place.
  const registration = window.ServiceWorkerRegistration;
  if (registration && registration.prototype.showNotification) {
    registration.prototype.showNotification = function (title, options) {
      new GChatNotification(title, options, 'sw');
      return Promise.resolve();
    };
    registration.prototype.getNotifications = () => Promise.resolve([]);
  }

  // A notification created through the service worker registration has no
  // handler on the object -- the page never sees the click, the worker's own
  // `notificationclick` listener would, and that is out of reach from here. Any
  // Chat link the payload carries is the next best thing. (Measured: what Chat
  // actually sends is a tag of <message id>/<sender id> and no link, so this
  // fallback has nothing to work with -- it stays for the day that changes.)
  const CHAT_LINK = /https:\/\/chat\.google\.com\/[^\s"']+/;
  // Avatars and emoji come from the same host; navigating to one would be worse
  // than doing nothing.
  const IMAGE_LINK = /\.(png|jpe?g|gif|webp|svg|ico)($|[?#])/i;

  function findChatLink(value, depth) {
    if (value === null || value === undefined || depth > 4) return null;

    if (typeof value === 'string') {
      const match = value.match(CHAT_LINK);
      return match && !IMAGE_LINK.test(match[0]) ? match[0] : null;
    }
    if (typeof value !== 'object') return null;

    for (const nested of Object.values(value)) {
      const found = findChatLink(nested, depth + 1);
      if (found) return found;
    }
    return null;
  }

  // Rust reports a click here (Linux only -- macOS/Windows have no such hook).
  // Dispatching on the original object runs Google's own handler, if it has
  // one. Rust raises the window before sending this, because Chat's router does
  // nothing while the page is hidden.
  function listenForActivation() {
    const events = window.__TAURI__ && window.__TAURI__.event;
    if (!events || !events.listen) return;

    events
      .listen('notification-activated', (message) => {
        const notification = liveNotifications.get(message.payload);
        if (!notification) {
          log('info', `notification activated: id=${message.payload} (no live object)`);
          return;
        }

        const handlers = notification._dispatch('click');
        const link = handlers
          ? null
          : findChatLink(notification.data, 0) || findChatLink(notification.tag, 0);

        log(
          'info',
          `notification activated: id=${message.payload} source=${notification._source} ` +
            `handlers=${handlers}${link ? ` link=${link}` : ''}`
        );

        if (link) location.assign(link);
      })
      .catch(ignore);
  }

  /* ------------------------------------------------------------------ boot */

  whenReady(() => {
    log('info', `chat.js attached to ${location.href}`);
    listenForActivation();
    pollUnread();
    setInterval(pollUnread, POLL_MS);
  });
})();
