#!/usr/bin/env node
/*
 * Runs chat.js against a stand-in for Google's page.
 *
 *     node scripts/chatjs-test.js
 *
 * `node --check` proves the file parses; this proves it behaves. The script is
 * an IIFE with no exports, so everything under test is reached the way Chat
 * reaches it: through `window.Notification`, through `window.open`, and through
 * the listeners it registers on `document`. The fakes below implement only what
 * chat.js actually touches -- when it starts touching more, this will say so by
 * failing rather than by pretending.
 *
 * What it cannot cover: anything that needs Google's real markup or a real
 * Tauri bridge. The unread scraper is exercised against a hand-built DOM, not
 * Chat's.
 */

const fs = require('fs');
const path = require('path');
const vm = require('vm');

const SCRIPT = path.join(__dirname, '..', 'src-tauri', 'src', 'inject', 'chat.js');

let failures = 0;

function check(name, ok, detail) {
  console.log(`  ${ok ? 'PASS' : 'FAIL'}  ${name}${ok || !detail ? '' : `  -- ${detail}`}`);
  if (!ok) failures++;
}

/** A document with the handful of methods chat.js uses, and captured listeners. */
function makeDocument(dom = {}) {
  const listeners = {};

  return {
    listeners,
    addEventListener(type, handler, capture) {
      (listeners[type] = listeners[type] || []).push({ handler, capture });
    },
    querySelector: dom.querySelector || (() => null),
    // `head` and `readyState` are only touched by the error-page fingerprint;
    // a head with something in it is what every real page has.
    readyState: dom.readyState || 'complete',
    head: dom.head || { children: [{}] },
    body: dom.body || { children: [{}], querySelectorAll: () => [] }
  };
}

/** Load chat.js into a fresh context and hand back what it exported onto it. */
function load({ dom = {}, onInvoke = () => Promise.resolve(), href } = {}) {
  const calls = [];
  const document = makeDocument(dom);

  const windowListeners = {};

  const window = {
    top: null,
    self: null,
    // `origin` as well as `href`: chat.js compares a link's origin against
    // this one, and a location without it makes every link look external.
    location: {
      href: href || 'https://mail.google.com/chat/u/0/',
      origin: 'https://mail.google.com',
      assign: () => {}
    },
    windowListeners,
    addEventListener(type, handler) {
      (windowListeners[type] = windowListeners[type] || []).push(handler);
    },
    setInterval: () => 0,
    clearInterval: () => {},
    open: function nativeOpen() {},
    ServiceWorkerRegistration: function () {},
    __TAURI_INTERNALS__: {
      invoke(command, args) {
        calls.push({ command, args });
        // A throwing `onInvoke` stands in for a command the ACL turned down,
        // which arrives at the page as a rejected promise, not a throw.
        try {
          return Promise.resolve(onInvoke(command, args));
        } catch (err) {
          return Promise.reject(err);
        }
      }
    }
  };
  window.top = window;
  window.self = window;
  window.ServiceWorkerRegistration.prototype = {
    showNotification: () => Promise.resolve(),
    getNotifications: () => Promise.resolve([])
  };
  // The event bridge is optional; chat.js must survive its absence.
  window.__TAURI__ = { event: null };

  // In a browser the global object *is* window, so `X` and `window.X` are the
  // same thing. Model that rather than a separate globals bag, or the fakes
  // diverge from the page in ways the app would never hit.
  const context = vm.createContext(window);
  context.window = window;
  context.document = document;
  context.console = console;
  // A vm context gets the ECMAScript intrinsics and nothing else: `URL` is a
  // web API, and without it `isCrossOrigin` throws into its own catch and calls
  // every link same-origin. Every webview has it.
  context.URL = URL;

  vm.runInContext(fs.readFileSync(SCRIPT, 'utf8'), context, { filename: 'chat.js' });
  return { window, document, calls };
}

/** Fire a listener chat.js registered on `window` rather than on `document`. */
function fireWindow(window, type) {
  for (const handler of window.windowListeners[type] || []) handler({ type });
}

/** Let the promise chain behind an invoke settle. */
const settle = () => new Promise((resolve) => setImmediate(resolve));

/** Fire a captured listener as the page would. */
function fire(document, type, event) {
  for (const { handler } of document.listeners[type] || []) handler(event);
}

const anchor = (href, target) => ({ tagName: 'A', href, target, parentElement: null });

console.log('[1/7] boot');
{
  const { window, document, calls } = load();
  check('reports itself once ready', calls.some((c) => c.command === 'page_log'));
  check('replaces window.Notification', typeof window.Notification === 'function');
  check('keeps the native window.open reachable', typeof window.open.__gchat_native === 'function');
  check('registers click and keydown listeners', !!document.listeners.click && !!document.listeners.keydown);

  // Injected twice on purpose; the second run must do nothing.
  const before = calls.length;
  vm.runInContext(fs.readFileSync(SCRIPT, 'utf8'), window, { filename: 'chat.js' });
  check('is idempotent per document', calls.length === before, `${calls.length - before} extra call(s)`);
}

console.log('[2/7] notifications');
{
  const { window, calls } = load();
  const shown = new window.Notification('Ankur', { body: 'hello', tag: 'dm/42', data: { url: 'x' } });

  const sent = calls.find((c) => c.command === 'show_notification');
  check('forwards to Rust', !!sent && sent.args.title === 'Ankur' && sent.args.body === 'hello');
  check('assigns an id the click can be matched to', !!sent && sent.args.id === shown._id);
  check('reports permission as granted', window.Notification.permission === 'granted');
  check('resolves requestPermission', typeof window.Notification.requestPermission === 'function');

  let ran = 0;
  shown.onclick = () => ran++;
  shown.addEventListener('click', () => ran++);
  const handlers = shown._dispatch('click');
  check('runs both handler styles', ran === 2, `${ran} ran`);
  check('counts what it ran', handlers === 2, `reported ${handlers}`);

  const noHandlers = new window.Notification('Quiet', {});
  check('reports zero when nothing handles a click', noHandlers._dispatch('click') === 0);

  // A throwing handler must not stop the others.
  const messy = new window.Notification('Messy', {});
  let after = 0;
  const noisy = console.error;
  console.error = () => {}; // the throw below is on purpose
  messy.onclick = () => {
    throw new Error('boom');
  };
  messy.addEventListener('click', () => after++);
  messy._dispatch('click');
  console.error = noisy;
  check('survives a handler that throws', after === 1);

  const viaWorker = window.ServiceWorkerRegistration.prototype.showNotification('SW', { body: 'b' });
  // Not `instanceof Promise`: the script runs in its own realm, so its Promise
  // is not this one. Thenable is what Chat actually depends on.
  check('routes the service worker path too', !!viaWorker && typeof viaWorker.then === 'function');
  const swCall = calls.filter((c) => c.command === 'show_notification').pop();
  check('marks where it came from', swCall.args.title === 'SW');
}

console.log('[3/7] links');
{
  const { window, calls } = load();
  window.open('https://example.test/page');
  const opened = calls.find((c) => c.command === 'open_external_url');
  check('hands window.open to Rust', !!opened && opened.args.url === 'https://example.test/page');

  const stub = window.open('https://example.test/other');
  check('returns something usable to Google', !!stub && typeof stub.close === 'function' && stub.closed === false);
}

console.log('[4/7] click interception');
{
  const { document, calls } = load();
  let prevented = 0;
  const clickOn = (target) =>
    fire(document, 'click', {
      target,
      preventDefault: () => prevented++,
      stopPropagation: () => {}
    });

  clickOn(anchor('https://example.test/away'));
  check('sends a cross-origin link to Rust', calls.some((c) => c.command === 'open_external_url'));
  check('swallows the click that it took', prevented === 1, `prevented ${prevented}`);

  const before = calls.filter((c) => c.command === 'open_external_url').length;
  clickOn(anchor('https://mail.google.com/chat/u/0/#chat/home'));
  const after = calls.filter((c) => c.command === 'open_external_url').length;
  check('leaves same-origin routing alone', after === before, 'intercepted an in-app link');

  clickOn(anchor('https://mail.google.com/chat/u/0/thing', '_blank'));
  check(
    'still takes a same-origin link marked _blank',
    calls.filter((c) => c.command === 'open_external_url').length === after + 1
  );

  clickOn({ tagName: 'DIV', parentElement: null });
  check('ignores a click on something that is not a link', prevented === 2, `prevented ${prevented}`);
}

console.log('[5/7] keyboard shortcuts');
{
  const focused = [];
  const searchBox = {
    focus: () => focused.push('search'),
    offsetWidth: 100,
    offsetHeight: 20,
    getClientRects: () => [{}]
  };
  const { document, calls } = load({
    dom: { querySelector: (sel) => (sel === 'input[name="q"]' ? searchBox : null) }
  });

  const press = (key, mods = {}) =>
    fire(document, 'keydown', {
      key,
      ctrlKey: !!mods.ctrl,
      metaKey: !!mods.meta,
      altKey: !!mods.alt,
      shiftKey: !!mods.shift,
      preventDefault: () => {},
      stopPropagation: () => {}
    });

  const actions = () => calls.filter((c) => c.command === 'menu_action').map((c) => c.args.action);

  press('f', { ctrl: true });
  check('Ctrl+F focuses the search box locally', focused.length === 1 && !actions().includes('search'));

  press('=', { ctrl: true });
  press('-', { ctrl: true });
  press('0', { ctrl: true });
  press('w', { ctrl: true });
  press('ArrowLeft', { alt: true });
  press('ArrowRight', { alt: true });
  // Alt+Home is on the History menu item, and menu accelerators do not reach
  // the app while focus is in the webview, so this is the only thing answering.
  press('Home', { alt: true });
  check(
    'forwards the rest to Rust',
    JSON.stringify(actions()) ===
      JSON.stringify([
        'zoom-in',
        'zoom-out',
        'zoom-reset',
        'close-to-tray',
        'back',
        'forward',
        'home'
      ]),
    actions().join(',')
  );

  const before = actions().length;
  press('a', { ctrl: true });
  press('ArrowLeft');
  check('leaves everything else to the page', actions().length === before);

  press('+', { ctrl: true, shift: true });
  check('accepts Ctrl+Shift+= as zoom in', actions().length === before + 1);
}

/*
 * The last two need a turn of the microtask queue, so they live in an async
 * main and the report moves in with them.
 */
async function rest() {
  console.log('[6/7] hand-off fallback');
  {
    // The capability names mail.google.com and chat.google.com and nothing
    // else, so on Google's post-sign-out marketing page every invoke is turned
    // down. The link has to go somewhere anyway, or the page is a dead end.
    const { window, document, calls } = load({
      onInvoke: (command) => {
        if (command === 'open_external_url') throw new Error('ACL: origin not allowed');
      }
    });

    // The rejection below is the point of the test, so let neither chat.js's
    // own report of it nor the fallback's warning clutter the run.
    const quiet = { warn: console.warn, error: console.error };
    console.warn = () => {};
    console.error = () => {};
    fire(document, 'click', {
      target: anchor('https://accounts.google.com/ServiceLogin'),
      preventDefault: () => {},
      stopPropagation: () => {}
    });
    await settle();
    console.warn = quiet.warn;
    console.error = quiet.error;

    check('asks Rust first', calls.some((c) => c.command === 'open_external_url'));
    check(
      'navigates this window when Rust will not answer',
      window.location.href === 'https://accounts.google.com/ServiceLogin',
      window.location.href
    );
  }
  {
    // And the opposite: where the policy does apply, Rust owns the outcome and
    // the window must stay where it is.
    const { window, document } = load();
    fire(document, 'click', {
      target: anchor('https://docs.google.com/document/d/abc/edit'),
      preventDefault: () => {},
      stopPropagation: () => {}
    });
    await settle();
    check(
      'leaves the window alone when Rust took the link',
      window.location.href === 'https://mail.google.com/chat/u/0/',
      window.location.href
    );
  }

  console.log('[7/7] webview error page');
  {
    // What WebKitGTK builds for a failed load: an empty head, and a body with
    // one line of text and no elements. Unstyled, so black on the window's
    // dark background.
    const body = {
      children: [],
      textContent: 'Could not connect to server',
      innerHTML: '',
      querySelectorAll: () => []
    };
    const button = { listeners: [], addEventListener: (t, h) => button.listeners.push(h) };
    // The URL that actually fails at startup, without the trailing slash:
    // Google's 302 to the canonical form never happened.
    const FAILED = 'https://mail.google.com/chat/u/0';
    const { window, calls } = load({
      href: FAILED,
      dom: {
        head: { children: [] },
        body,
        readyState: 'loading',
        querySelector: (sel) => (sel === 'button.retry' ? button : null)
      }
    });

    check('waits for the document', body.innerHTML === '', 'rewrote a page still parsing');

    fireWindow(window, 'load');
    check('rewrites the unreadable page', body.innerHTML.includes('out of reach'));
    check('keeps the reason the webview gave', body.innerHTML.includes('Could not connect to server'));
    check('paints over the window background colour', body.innerHTML.includes('#202124'));
    check('offers a retry button', body.innerHTML.includes('<button class="retry"'));
    check(
      'says so in the app log',
      calls.some((c) => c.command === 'page_log' && /load failed/.test(c.args.message))
    );

    // The retry must aim somewhere other than the URL that failed: WebKit's
    // stand-in document will not navigate to the one it stands in for, and the
    // bridge is closed to it because the origin is opaque.
    for (const handler of button.listeners) handler({ type: 'click' });
    check(
      'retries at a URL other than the one that failed',
      window.location.href !== FAILED,
      window.location.href
    );
    check(
      'and it is still Chat',
      window.location.href === 'https://mail.google.com/chat/u/0/',
      window.location.href
    );

    window.location.href = FAILED;
    fireWindow(window, 'online');
    check('retries when the network comes back', window.location.href !== FAILED);
  }
  {
    // The message is the webview's, not ours, and it carries the URL that
    // failed -- so it goes in escaped.
    const body = {
      children: [],
      textContent: '<img src=x onerror=alert(1)>',
      innerHTML: '',
      querySelectorAll: () => []
    };
    load({ dom: { head: { children: [] }, body, readyState: 'complete' } });
    check(
      'escapes the message it was handed',
      !body.innerHTML.includes('<img') && body.innerHTML.includes('&lt;img'),
      body.innerHTML.slice(0, 80)
    );
  }
  {
    // If the canonical form is somehow the one that failed, the retry has to
    // move anyway -- aiming at the same string again is the one thing that
    // provably does nothing.
    const CANONICAL = 'https://mail.google.com/chat/u/0/';
    const body = {
      children: [],
      textContent: 'Could not connect to server',
      innerHTML: '',
      querySelectorAll: () => []
    };
    const button = { listeners: [], addEventListener: (t, h) => button.listeners.push(h) };
    const { window } = load({
      href: CANONICAL,
      dom: {
        head: { children: [] },
        body,
        readyState: 'complete',
        querySelector: (sel) => (sel === 'button.retry' ? button : null)
      }
    });
    for (const handler of button.listeners) handler({ type: 'click' });
    check('never retries at the URL it is already on', window.location.href !== CANONICAL,
      window.location.href);
  }
  {
    // A real page must be left entirely alone, including one caught mid-parse.
    const body = {
      children: [{}],
      textContent: 'Chat',
      innerHTML: '<div>real</div>',
      querySelectorAll: () => []
    };
    load({ dom: { body, readyState: 'complete' } });
    check('leaves a real page alone', body.innerHTML === '<div>real</div>', body.innerHTML);

    const parsing = { children: [], textContent: '', innerHTML: '', querySelectorAll: () => [] };
    load({ dom: { head: { children: [] }, body: parsing, readyState: 'complete' } });
    check('leaves an empty document alone', parsing.innerHTML === '', parsing.innerHTML);
  }

  console.log(failures ? `\n${failures} FAILED` : '\nall checks passed');
  process.exit(failures ? 1 : 0);
}

rest();
