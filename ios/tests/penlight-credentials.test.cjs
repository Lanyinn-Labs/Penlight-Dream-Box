const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const source = fs.readFileSync(path.join(__dirname, '../common/penlight-credentials.js'), 'utf8');
function client(qx = false, script = undefined, extraContext = {}) {
  const store = {};
  return (request, argument = 'capture', { assertEmptyDone = true } = {}) => {
    const notifications = [], logs = [], done = [];
    const context = {
      $argument: argument,
      $done: value => done.push(value),
      console: { log: value => logs.push(value) },
      ...extraContext,
    };
    if (request !== undefined) context.$request = request;
    if (script !== undefined) context.$script = script;
    if (qx) {
      context.$prefs = { valueForKey: key => store[key], setValueForKey: (value, key) => (store[key] = value, true) };
      context.$notify = (...args) => notifications.push(args);
    } else {
      context.$persistentStore = { read: key => store[key], write: (value, key) => (store[key] = value, true) };
      // Deliberately ignore clipboard options, as a client may silently do.
      context.$notification = { post: (...args) => notifications.push(args) };
    }
    vm.runInNewContext(source, context);
    assert.equal(done.length, 1);
    if (assertEmptyDone) assert.equal(JSON.stringify(done[0]), '{}');
    return { notifications, logs, done: done[0] };
  };
}
const record = { uid: '123456789', uuid: '01234567-89ab-cdef-0123-456789abcdef' };
const request = { url: `https://api.garupa.jp/api/user/${record.uid}/profile`, headers: { 'x-SIGNATURE': [record.uuid] } };
for (const qx of [false, true]) {
  test(`complete JSON and repeated capture (${qx ? 'QX' : 'post'})`, () => {
    const run = client(qx);
    for (let i = 0; i < 2; i++) {
      const result = run(request);
      assert.equal(result.notifications.length, 1);
      assert.deepEqual(JSON.parse(result.notifications[0][2]), record);
      assert.deepEqual(result.logs, []);
      const options = result.notifications[0][3];
      assert.deepEqual(JSON.parse(qx ? options['update-pasteboard'] : options.text), record);
    }
    const shown = run(undefined);
    assert.deepEqual(JSON.parse(shown.logs[0]), record);
    assert.deepEqual(JSON.parse(shown.notifications[0][2]), record);
  });
}
test('incomplete request cannot mix accounts or overwrite saved credentials', () => {
  const run = client();
  run(request);
  assert.equal(run({ url: 'https://api.garupa.jp/api/user/987/profile', headers: {} }).notifications.length, 0);
  assert.deepEqual(JSON.parse(run(undefined, 'show').logs[0]), record);
});
test('manual display before capture reports empty storage without credential output', () => {
  const result = client()(undefined);
  assert.equal(result.notifications.length, 1);
  assert.match(result.notifications[0][1], /还没有捕获/);
  assert.deepEqual(result.logs, []);
});
test('quoted $argument values are trimmed when capturing and showing', () => {
  const run = client();
  const captured = run(request, '"capture"');
  assert.equal(captured.notifications.length, 1);
  assert.deepEqual(JSON.parse(captured.notifications[0][2]), record);

  const shownDouble = run(request, ' "show" ');
  assert.deepEqual(JSON.parse(shownDouble.logs[0]), record);
  assert.deepEqual(JSON.parse(shownDouble.notifications[0][2]), record);

  const shownSingle = run(request, "'show'");
  assert.deepEqual(JSON.parse(shownSingle.logs[0]), record);
});
test('stash tile mode returns tile payload for empty and captured states', () => {
  const run = client(false, { type: 'tile' });
  const emptyTile = run(undefined, 'show', { assertEmptyDone: false });
  assert.equal(emptyTile.notifications.length, 0);
  assert.deepEqual(emptyTile.logs, []);
  assert.equal(emptyTile.done.title, 'Penlight UID / UUID');
  assert.match(emptyTile.done.content, /还没有捕获/);
  assert.equal(emptyTile.done.icon, 'exclamationmark.circle');
  assert.equal(emptyTile.done.backgroundColor, '#8e8e93');

  run(request, 'capture');
  const filledTile = run(undefined, 'show', { assertEmptyDone: false });
  assert.equal(filledTile.notifications.length, 0);
  assert.deepEqual(filledTile.logs, []);
  assert.equal(filledTile.done.title, 'Penlight UID / UUID');
  assert.deepEqual(JSON.parse(filledTile.done.content), record);
  assert.equal(filledTile.done.icon, 'doc.on.clipboard');
  assert.equal(filledTile.done.backgroundColor, '#1d1d1f');
});
test('surge uses official clipboard attachment and omits 4th arg when empty', () => {
  const run = client(false, undefined, { $environment: { 'surge-version': '5.14.0' } });
  const emptyShow = run(undefined, 'show');
  assert.equal(emptyShow.notifications.length, 1);
  assert.equal(emptyShow.notifications[0].length, 3);

  const captured = run(request, 'capture');
  assert.equal(captured.notifications.length, 1);
  assert.equal(captured.notifications[0].length, 4);
  assert.deepEqual(Object.keys(captured.notifications[0][3]).sort(), ['action', 'text']);
  assert.equal(captured.notifications[0][3].action, 'clipboard');
  assert.deepEqual(JSON.parse(captured.notifications[0][3].text), record);

  const shown = run(undefined, 'show');
  assert.equal(shown.notifications[0].length, 4);
  assert.deepEqual(Object.keys(shown.notifications[0][3]).sort(), ['action', 'text']);
  assert.deepEqual(JSON.parse(shown.logs[0]), record);
});
test('stash request and manual script runner emit 3-arg notifications', () => {
  const run = client(false, { type: 'request' }, { $environment: { 'stash-version': '3.6.0' } });
  const captured = run(request, 'capture');
  assert.equal(captured.notifications.length, 1);
  assert.equal(captured.notifications[0].length, 3);
  assert.deepEqual(JSON.parse(captured.notifications[0][2]), record);

  const manualRun = client(false, undefined, { $environment: { 'stash-version': '3.6.0' } });
  manualRun(request, 'capture');
  const shown = manualRun(undefined, 'show');
  assert.equal(shown.notifications.length, 1);
  assert.equal(shown.notifications[0].length, 3);
  assert.deepEqual(JSON.parse(shown.logs[0]), record);
});
test('loon uses clipboard-only attach and returns generic payload when no request', () => {
  const run = client(false, undefined, { $loon: 'iPhone15,2, 17.0, 3.5.2' });

  const emptyGeneric = run(undefined, 'show', { assertEmptyDone: false });
  assert.equal(emptyGeneric.notifications.length, 1);
  assert.equal(emptyGeneric.notifications[0].length, 3);
  assert.equal(emptyGeneric.done.title, 'Penlight UID / UUID');
  assert.match(emptyGeneric.done.content, /还没有捕获/);

  const captured = run(request, 'capture');
  assert.equal(captured.notifications.length, 1);
  assert.equal(captured.notifications[0].length, 4);
  assert.deepEqual(Object.keys(captured.notifications[0][3]), ['clipboard']);
  assert.deepEqual(JSON.parse(captured.notifications[0][3].clipboard), record);

  const populatedGeneric = run({}, 'show', { assertEmptyDone: false });
  assert.equal(populatedGeneric.notifications.length, 1);
  assert.equal(populatedGeneric.notifications[0].length, 4);
  assert.deepEqual(Object.keys(populatedGeneric.notifications[0][3]), ['clipboard']);
  assert.deepEqual(JSON.parse(populatedGeneric.logs[0]), record);
  assert.equal(populatedGeneric.done.title, 'Penlight UID / UUID');
  assert.deepEqual(JSON.parse(populatedGeneric.done.content), record);

  const httpWithShowArg = run(request, 'show');
  assert.equal(httpWithShowArg.notifications[0].length, 4);
  assert.equal(JSON.stringify(httpWithShowArg.done), '{}');
});
test('quantumult x omits update-pasteboard when empty', () => {
  const run = client(true);
  const emptyShow = run(undefined, 'show');
  assert.equal(emptyShow.notifications.length, 1);
  assert.equal(emptyShow.notifications[0].length, 3);

  const captured = run(request);
  assert.equal(captured.notifications.length, 1);
  assert.equal(captured.notifications[0].length, 4);
  assert.deepEqual(JSON.parse(captured.notifications[0][3]['update-pasteboard']), record);
});
test('object $argument and missing $request.url fall back safely to show', () => {
  const run = client();
  run(request, 'capture');

  for (const arg of [{ mode: 'show' }, { show: true }, '{"mode":"show"}']) {
    const shown = run(request, arg);
    assert.deepEqual(JSON.parse(shown.logs[0]), record);
    assert.equal(shown.notifications.length, 1);
  }

  const missingUrl = run({}, '');
  assert.deepEqual(JSON.parse(missingUrl.logs[0]), record);
  assert.equal(missingUrl.notifications.length, 1);
});
test('client configs and regexes preserve Shadowrocket and fix updated clients', () => {
  const readCfg = rel => fs.readFileSync(path.join(__dirname, '..', rel), 'utf8');
  const shadowrocket = readCfg('shadowrocket/penlight-credentials.module');
  const surge = readCfg('surge/penlight-credentials.sgmodule');
  const stash = readCfg('stash/penlight-credentials.stoverride');
  const loon = readCfg('loon/penlight-credentials.plugin');
  const qx = readCfg('quantumultx/penlight-credentials.remote.snippet');

  assert.match(shadowrocket, /engine=jsc/);
  assert.match(shadowrocket, /requires-body=false/);
  assert.match(shadowrocket, /^hostname = %APPEND% api\.garupa\.jp$/m);

  assert.doesNotMatch(surge, /engine=jsc/);
  assert.doesNotMatch(surge, /^#!system=/m);
  assert.match(surge, /pattern=\^https:\/\/api\\\.garupa\\\.jp\/api\/user\/\[0-9\]\+\(\?:\[\/\?\]\|\$\)/);

  assert.doesNotMatch(stash, /engine:\s*jsc/);
  assert.match(stash, /require-body:\s*false/);

  assert.match(loon, /^#!loon_version\s*=\s*3\.5\.1\(983\)$/m);
  assert.doesNotMatch(loon, /^#!system\s*=/m);
  assert.match(loon, /^request if \$\{url\} ~= \/\^https:\\\/\\\/api\\\.garupa\\\.jp\\\/api\\\/user\\\/\[0-9\]\+\(\?:\[\\\/\?\]\|\$\)\/i then script\(/m);
  assert.match(loon, /^generic then script\(.*\) with tag="显示已保存 UID UUID", timeout=10, enable=true$/m);
  assert.doesNotMatch(loon, /^generic .*requires_body/m);
  assert.match(loon, /^\[Mitm\]$/m);

  assert.match(qx, /^; /m);
  assert.match(qx, /^hostname = api\.garupa\.jp\r?\n\r?\n\^https:/m);
  assert.match(qx, /url script-request-header https:\/\/raw\.githubusercontent\.com\//);

  const extractRegex = [
    new RegExp(shadowrocket.match(/pattern=([^,]+)/)[1]),
    new RegExp(surge.match(/pattern=([^,]+)/)[1]),
    new RegExp(stash.match(/match:\s*'([^']+)'/)[1]),
    new RegExp(loon.match(/~= \/(.+)\/i /)[1], 'i'),
    new RegExp(qx.match(/^(\^[^\s]+) url script-request-header/m)[1]),
  ];
  const validUrls = [
    'https://api.garupa.jp/api/user/123456789',
    'https://api.garupa.jp/api/user/123456789/',
    'https://api.garupa.jp/api/user/123456789/profile',
    'https://api.garupa.jp/api/user/123456789?lang=ja',
  ];
  const invalidUrls = [
    'https://api.garupa.jp/api/user/',
    'https://api.garupa.jp/api/user/123456789abc',
    'https://api.garupa.jp/api/login',
    'http://api.garupa.jp/api/user/123456789',
  ];
  for (const re of extractRegex) {
    for (const url of validUrls) assert.equal(re.test(url), true, `${re} should match ${url}`);
    for (const url of invalidUrls) assert.equal(re.test(url), false, `${re} should not match ${url}`);
  }
});
test('web/app.js builds valid install URLs for all five clients', () => {
  const appSource = fs.readFileSync(path.join(__dirname, '../../web/app.js'), 'utf8');
  const installLinks = ['shadowrocket', 'surge', 'stash', 'loon', 'quantumultx'].map(clientName => ({
    dataset: { installClient: clientName },
    href: '',
    addEventListener() {},
    querySelector() { return { textContent: clientName }; },
  }));
  const stubElement = () => ({
    value: '',
    textContent: '',
    className: '',
    type: 'password',
    addEventListener() {},
    setAttribute() {},
    focus() {},
    classList: { add() {}, toggle() {} },
    querySelector() { return { textContent: '' }; },
  });
  const context = {
    window: { PENLIGHT_CONFIG: { apiPrefix: '/api' } },
    document: {
      querySelector: () => stubElement(),
      querySelectorAll: selector => (selector === '[data-install-client]' ? installLinks : []),
      createElement: () => stubElement(),
    },
    encodeURIComponent,
    URL,
    setTimeout,
  };
  vm.runInNewContext(appSource, context);

  const byClient = Object.fromEntries(installLinks.map(link => [link.dataset.installClient, link.href]));
  assert.match(byClient.shadowrocket, /^shadowrocket:\/\/install\?module=/);
  assert.match(byClient.surge, /^surge:\/\/\/install-module\?url=/);
  assert.match(byClient.stash, /^stash:\/\/install-override\?url=/);
  assert.match(byClient.loon, /^loon:\/\/import\?plugin=/);
  assert.match(byClient.quantumultx, /^quantumult-x:\/\/\/add-resource\?remote-resource=/);

  const qxPayload = JSON.parse(decodeURIComponent(byClient.quantumultx.split('remote-resource=')[1]));
  assert.equal(qxPayload.rewrite_remote.length, 1);
  assert.match(
    qxPayload.rewrite_remote[0],
    /penlight-credentials\.remote\.snippet, tag=Penlight Dream Box, update-interval=86400, opt-parser=false, enabled=true$/
  );
});
