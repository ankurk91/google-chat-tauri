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

  var lastUnread = -1;
  function pollUnread() {
    var count = readUnreadCount();
    if (count === lastUnread) return;
    lastUnread = count;
    invoke('set_unread_count', { count: count })['catch'](ignore);
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

  /* ------------------------------------------------------------- Ctrl+F */
  /* Ported from electron src/preload/searchShortcut.ts. Done entirely in JS
   * rather than via a menu accelerator, which is not reliably delivered to the
   * webview on Linux/GTK. */

  function isVisible(el) {
    return !!(el.offsetWidth || el.offsetHeight || el.getClientRects().length);
  }

  document.addEventListener(
    'keydown',
    function (e) {
      if (!(e.ctrlKey || e.metaKey) || e.shiftKey || e.altKey) return;
      if (String(e.key).toLowerCase() !== 'f') return;

      var search = document.querySelector('input[name="q"]');
      if (search && isVisible(search)) {
        e.preventDefault();
        e.stopPropagation();
        search.focus();
      }
    },
    true
  );

  /* ------------------------------------------------- notification probe (P0) */
  /* Not the shim yet -- this only answers the open question of whether Chat
   * still calls `new Notification()` or has moved to a service-worker push.
   * The shim lands in P1 once we know. */

  if (window.Notification) {
    var Native = window.Notification;
    var Probe = function (title, options) {
      log('info', 'Notification constructed: ' + title);
      return new Native(title, options);
    };
    Probe.requestPermission = function (cb) {
      log('info', 'Notification.requestPermission() called');
      return Native.requestPermission(cb);
    };
    Object.defineProperty(Probe, 'permission', {
      get: function () {
        return Native.permission;
      }
    });
    Probe.prototype = Native.prototype;
    window.Notification = Probe;
    log('info', 'Notification API present, permission=' + Native.permission);
  } else {
    log('info', 'Notification API ABSENT in this webview');
  }

  if (window.ServiceWorkerRegistration && ServiceWorkerRegistration.prototype.showNotification) {
    var nativeShow = ServiceWorkerRegistration.prototype.showNotification;
    ServiceWorkerRegistration.prototype.showNotification = function (title, options) {
      log('info', 'SW showNotification: ' + title);
      return nativeShow.apply(this, arguments);
    };
  }

  /* ------------------------------------------------------------------ boot */

  whenReady(function () {
    log('info', 'chat.js attached to ' + location.href);
    pollUnread();
    setInterval(pollUnread, POLL_MS);
  });
})();
