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
    body: dom.body || { querySelectorAll: () => [] }
  };
}

/** Load chat.js into a fresh context and hand back what it exported onto it. */
function load({ dom = {}, onInvoke = () => Promise.resolve() } = {}) {
  const calls = [];
  const document = makeDocument(dom);

  const window = {
    top: null,
    self: null,
    // `origin` as well as `href`: chat.js compares a link's origin against
    // this one, and a location without it makes every link look external.
    location: {
      href: 'https://mail.google.com/chat/u/0/',
      origin: 'https://mail.google.com',
      assign: () => {}
    },
    setInterval: () => 0,
    clearInterval: () => {},
    open: function nativeOpen() {},
    ServiceWorkerRegistration: function () {},
    __TAURI_INTERNALS__: {
      invoke(command, args) {
        calls.push({ command, args });
        return Promise.resolve(onInvoke(command, args));
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

/** Fire a captured listener as the page would. */
function fire(document, type, event) {
  for (const { handler } of document.listeners[type] || []) handler(event);
}

const anchor = (href, target) => ({ tagName: 'A', href, target, parentElement: null });

console.log('[1/5] boot');
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

console.log('[2/5] notifications');
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

console.log('[3/5] links');
{
  const { window, calls } = load();
  window.open('https://example.test/page');
  const opened = calls.find((c) => c.command === 'open_external_url');
  check('hands window.open to Rust', !!opened && opened.args.url === 'https://example.test/page');

  const stub = window.open('https://example.test/other');
  check('returns something usable to Google', !!stub && typeof stub.close === 'function' && stub.closed === false);
}

console.log('[4/5] click interception');
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

console.log('[5/5] keyboard shortcuts');
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
  check(
    'forwards the rest to Rust',
    JSON.stringify(actions()) ===
      JSON.stringify(['zoom-in', 'zoom-out', 'zoom-reset', 'close-to-tray', 'back', 'forward']),
    actions().join(',')
  );

  const before = actions().length;
  press('a', { ctrl: true });
  press('ArrowLeft');
  check('leaves everything else to the page', actions().length === before);

  press('+', { ctrl: true, shift: true });
  check('accepts Ctrl+Shift+= as zoom in', actions().length === before + 1);
}

console.log(failures ? `\n${failures} FAILED` : '\nall checks passed');
process.exit(failures ? 1 : 0);
