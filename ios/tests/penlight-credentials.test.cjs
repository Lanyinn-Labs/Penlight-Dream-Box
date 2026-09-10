const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const source = fs.readFileSync(path.join(__dirname, '../common/penlight-credentials.js'), 'utf8');
function client(qx = false) {
  const store = {};
  return (request, argument = 'capture') => {
    const notifications = [], logs = [], done = [];
    const context = {
      $argument: argument,
      $done: value => done.push(value),
      console: { log: value => logs.push(value) },
    };
    if (request !== undefined) context.$request = request;
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
    assert.equal(JSON.stringify(done[0]), '{}');
    return { notifications, logs };
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
