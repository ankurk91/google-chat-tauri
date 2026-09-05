/*
 * Injected into the remote Google Chat page.
 *
 * This is the Tauri equivalent of electron's src/preload/*.ts. It is injected
 * twice on purpose -- once as an initialization script (document-start, the
 * real mechanism) and again from on_page_load(Finished) as a fallback -- so
 * everything below must be idempotent per document.
 *
 * Plain ES5-flavoured JS on purpose: there is no bundler and no build step.
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

  var POLL_MS = 1000;

  /* ---------------------------------------------------------------- bridge */

  // Errors here are worth seeing. Swallowing them silently makes an ACL
  // rejection (which is what happens on any origin outside the capability's
  // remote.urls) indistinguishable from "the script never ran". Report the
  // first failure per command, then go quiet so a polling loop cannot spam.
  var reported = Object.create(null);

  function invoke(cmd, args) {
    var g = window.__TAURI__;
    var fn =
      (g && g.core && g.core.invoke) ||
      (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke);
    if (!fn) return Promise.reject(new Error('tauri ipc not ready'));

    return fn(cmd, args || {})['catch'](function (err) {
      if (!reported[cmd]) {
        reported[cmd] = true;
        console.error('[gchat] invoke(' + cmd + ') failed:', err);
      }
      throw err;
    });
  }

  function ignore() {}

  function log(level, message) {
    invoke('page_log', { level: level, message: String(message) })['catch'](ignore);
  }

  // Ordering against Tauri's own init scripts is not guaranteed, so wait for
  // the IPC internals rather than assuming they are already there.
  function whenReady(cb) {
    if (window.__TAURI_INTERNALS__) return cb();
    var tries = 0;
    var t = setInterval(function () {
      if (window.__TAURI_INTERNALS__ || ++tries > 100) {
        clearInterval(t);
        if (window.__TAURI_INTERNALS__) cb();
      }
    }, 50);
  }

  /* ------------------------------------------------- unread message counter */
  /* Ported verbatim from electron src/preload/unreadCount.ts */

  var UNREAD_SELECTORS = [
    'div[data-tooltip="Chat"][role="group"]',
    'div[data-tooltip="Spaces"][role="group"]'
  ].join(',');

  function readUnreadCount() {
    var total = 0;
    var groups = document.body ? document.body.querySelectorAll(UNREAD_SELECTORS) : [];

    for (var i = 0; i < groups.length; i++) {
      var heading = groups[i].querySelector('span[role="heading"]');
      var badge = heading && heading.nextElementSibling;
      if (badge) {
        var n = Number(badge.textContent);
        if (!isNaN(n)) total += n;
      }
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
    var link = document.querySelector('link[rel~="icon" i]');
    var href = (link && link.href) || '';
    if (!href) return null; // unknown -- do not overwrite what we last knew
    return /_dot_/.test(href) && !/_no_dot_/.test(href);
  }

  var lastCount = -1;
  var lastHasUnread = null;

  function pollUnread() {
    var count = readUnreadCount();
    var hasUnread = readHasUnread();
    if (hasUnread === null) hasUnread = lastHasUnread === null ? count > 0 : lastHasUnread;

    if (count === lastCount && hasUnread === lastHasUnread) return;
    lastCount = count;
    lastHasUnread = hasUnread;

    invoke('set_unread_count', { count: count, hasUnread: hasUnread })['catch'](ignore);
  }

  /* ------------------------------------------------------- external links */
  /* Ported from electron src/main/features/externalLinks.ts, which used
   * setWindowOpenHandler. Tauri has no equivalent hook for window.open, so the
   * interception happens here and the policy decision stays in Rust. */

  function handOff(url) {
    if (!url) return;
    invoke('open_external_url', { url: String(url) })['catch'](ignore);
  }

  var nativeOpen = window.open;
  window.open = function (url, name, features) {
    log('info', 'window.open intercepted: ' + url);
    handOff(url);
    // Returning null makes some Google flows throw; hand back an inert stub.
    return {
      closed: false,
      close: function () {},
      focus: function () {},
      blur: function () {},
      postMessage: function () {},
      document: null,
      location: { href: url || '' }
    };
  };
  window.open.__gchat_native = nativeOpen;

  function isCrossOrigin(href) {
    try {
      var u = new URL(href, location.href);
      if (u.protocol !== 'http:' && u.protocol !== 'https:') return false;
      return u.origin !== location.origin;
    } catch (err) {
      return false;
    }
  }

  document.addEventListener(
    'click',
    function (e) {
      var el = e.target;
      while (el && el.tagName !== 'A') el = el.parentElement;
      if (!el || !el.href) return;

      var opensNewWindow = el.target === '_blank' || el.target === '_new';
      if (!opensNewWindow && !isCrossOrigin(el.href)) {
        // Same-origin in-page navigation: Chat's own SPA routing. Leave it be.
        return;
      }

      e.preventDefault();
      e.stopPropagation();
      log('info', 'link intercepted: ' + el.href);
      // Rust decides whether this opens in the system browser or navigates the
      // main window -- one source of truth for the allow-list.
      handOff(el.href);
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

  function isVisible(el) {
    return !!(el.offsetWidth || el.offsetHeight || el.getClientRects().length);
  }

  function focusSearch() {
    var search = document.querySelector('input[name="q"]');
    if (search && isVisible(search)) {
      search.focus();
      return true;
    }
    return false;
  }

  function shortcutFor(e) {
    var mod = e.ctrlKey || e.metaKey;
    var key = String(e.key).toLowerCase();

    if (mod && !e.altKey && !e.shiftKey) {
      if (key === 'f') return 'search';
      if (key === '=' || key === '+') return 'zoom-in';
      if (key === '-' || key === '_') return 'zoom-out';
      if (key === '0') return 'zoom-reset';
      if (key === 'w') return 'close-to-tray';
    }
    // Ctrl+Shift+= is how "+" arrives on many layouts.
    if (mod && e.shiftKey && !e.altKey && (key === '+' || key === '=')) return 'zoom-in';

    if (e.altKey && !mod && !e.shiftKey) {
      if (key === 'arrowleft') return 'back';
      if (key === 'arrowright') return 'forward';
    }
    return null;
  }

  document.addEventListener(
    'keydown',
    function (e) {
      var action = shortcutFor(e);
      if (!action) return;

      if (action === 'search') {
        // Only swallow the key if there is actually a search box to focus,
        // so Chat's own find-in-page behaviour is not broken when there isn't.
        if (focusSearch()) {
          e.preventDefault();
          e.stopPropagation();
        }
        return;
      }

      e.preventDefault();
      e.stopPropagation();
      invoke('menu_action', { action: action })['catch'](ignore);
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

  var notifySeq = 0;
  var liveNotifications = Object.create(null);

  // Notification objects are kept so a click can be dispatched back onto the
  // one Chat created. Chat does not reliably call close(), and this app runs
  // for days, so without a cap the map grows for every message ever received.
  // Anything older than this is far past the point where clicking its
  // notification is possible -- the desktop stopped showing it long ago.
  var MAX_LIVE_NOTIFICATIONS = 50;

  function rememberNotification(n) {
    liveNotifications[n._id] = n;

    var cutoff = n._id - MAX_LIVE_NOTIFICATIONS;
    if (cutoff > 0 && liveNotifications[cutoff]) {
      // Ids increment, so anything at or below the cutoff is stale. Only the
      // boundary is checked each time; earlier ones were dropped on their turn.
      delete liveNotifications[cutoff];
    }
  }

  // Deliberately an ES5 constructor: Chat calls it with `new`, and arrow
  // functions cannot be constructed.
  function GChatNotification(title, options) {
    options = options || {};

    this._id = ++notifySeq;
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

    invoke('show_notification', {
      id: this._id,
      title: this.title,
      body: options.body || null
    })['catch'](ignore);
  }

  GChatNotification.prototype.addEventListener = function (type, cb) {
    if (this._listeners[type] && typeof cb === 'function') {
      this._listeners[type].push(cb);
    }
  };

  GChatNotification.prototype.removeEventListener = function (type, cb) {
    var list = this._listeners[type];
    if (!list) return;
    var i = list.indexOf(cb);
    if (i !== -1) list.splice(i, 1);
  };

  GChatNotification.prototype.close = function () {
    delete liveNotifications[this._id];
    this._dispatch('close');
  };

  GChatNotification.prototype._dispatch = function (type) {
    var event = {
      type: type,
      target: this,
      currentTarget: this,
      preventDefault: function () {},
      stopPropagation: function () {}
    };

    var handler = this['on' + type];
    if (typeof handler === 'function') {
      try {
        handler.call(this, event);
      } catch (e) {
        console.error('[gchat] notification on' + type + ' threw:', e);
      }
    }

    var list = this._listeners[type] || [];
    for (var i = 0; i < list.length; i++) {
      try {
        list[i].call(this, event);
      } catch (e) {
        console.error('[gchat] notification listener threw:', e);
      }
    }
  };

  GChatNotification.permission = 'granted';
  GChatNotification.maxActions = 0;
  GChatNotification.requestPermission = function (cb) {
    if (typeof cb === 'function') cb('granted');
    return Promise.resolve('granted');
  };

  window.Notification = GChatNotification;

  // Chat may deliver notifications through a service worker rather than
  // constructing them directly; route those to the same place.
  if (window.ServiceWorkerRegistration && ServiceWorkerRegistration.prototype.showNotification) {
    ServiceWorkerRegistration.prototype.showNotification = function (title, options) {
      new GChatNotification(title, options);
      return Promise.resolve();
    };
    ServiceWorkerRegistration.prototype.getNotifications = function () {
      return Promise.resolve([]);
    };
  }

  // Rust reports a click here (Linux only -- macOS/Windows have no such hook).
  // Dispatching on the original object runs Google's own handler, which opens
  // the conversation the notification was about.
  function listenForActivation() {
    var ev = window.__TAURI__ && window.__TAURI__.event;
    if (!ev || !ev.listen) return;

    ev.listen('notification-activated', function (msg) {
      var n = liveNotifications[msg.payload];
      log('info', 'notification activated: id=' + msg.payload + (n ? ' (dispatching click)' : ' (no live object)'));
      if (n) n._dispatch('click');
    })['catch'](ignore);
  }

  /* ------------------------------------------------------------------ boot */

  whenReady(function () {
    log('info', 'chat.js attached to ' + location.href);
    listenForActivation();
    pollUnread();
    setInterval(pollUnread, POLL_MS);
  });
})();
