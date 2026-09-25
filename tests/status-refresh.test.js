import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createStatusRefresh } from '../src/status-refresh.js';

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function fixture() {
  const reads = [], applied = [], errors = [], pending = [];
  const controller = createStatusRefresh({
    read: () => {
      const result = deferred();
      reads.push(result);
      return result.promise;
    },
    apply: value => applied.push(value),
    onError: error => errors.push(error.message),
    onPending: value => pending.push(value),
  });
  return { controller, reads, applied, errors, pending };
}

test('a startup read cannot replace configuration, restore, repair or image sync readback', async () => {
  for (const operation of ['configure', 'restore', 'repair', 'image sync']) {
    const f = fixture();
    const startup = f.controller.refresh();
    f.controller.setMutationActive(true);
    const readback = f.controller.refresh({ allowDuringMutation: true });
    f.reads[1].resolve(operation);
    assert.equal(await readback, 'applied');
    f.controller.setMutationActive(false);
    const pending = [...f.pending];
    f.reads[0].resolve('old startup snapshot');
    assert.equal(await startup, 'superseded');
    assert.deepEqual(f.applied, [operation]);
    assert.deepEqual(f.pending, pending);
    assert.equal(f.controller.pending, false);
  }
});

test('obsolete success or error cannot mutate state or unlock a newer refresh', async () => {
  for (const rejected of [false, true]) {
    const f = fixture();
    const old = f.controller.refresh();
    const latest = f.controller.refresh();
    if (rejected) f.reads[0].reject(new Error('obsolete failure'));
    else f.reads[0].resolve('obsolete snapshot');
    assert.equal(await old, 'superseded');
    assert.equal(f.controller.pending, true);
    assert.deepEqual(f.errors, []);
    assert.deepEqual(f.applied, []);
    f.reads[1].resolve('latest snapshot');
    assert.equal(await latest, 'applied');
    assert.equal(f.controller.pending, false);
  }
});

test('current failure is not masked by an older success', async () => {
  const f = fixture();
  const old = f.controller.refresh();
  const latest = f.controller.refresh();
  f.reads[1].reject(new Error('readback failed'));
  assert.equal(await latest, 'failed');
  f.reads[0].resolve('stale success');
  assert.equal(await old, 'superseded');
  assert.deepEqual(f.errors, ['readback failed']);
  assert.deepEqual(f.applied, []);
  assert.equal(f.controller.pending, false);
});

test('ordinary refreshes are suppressed during writes; failed writes release ownership', async () => {
  const f = fixture();
  const old = f.controller.refresh();
  f.controller.setMutationActive(true);
  assert.equal(await f.controller.refresh(), 'superseded');
  assert.equal(f.reads.length, 1);
  f.controller.setMutationActive(false);
  f.reads[0].resolve('before failed write');
  assert.equal(await old, 'superseded');
  const next = f.controller.refresh();
  f.reads[1].resolve('current');
  assert.equal(await next, 'applied');
  assert.deepEqual(f.applied, ['current']);
});

test('authoritative configuration commit invalidates pending work', async () => {
  const f = fixture();
  const read = f.controller.refresh();
  f.controller.invalidate();
  f.reads[0].reject(new Error('too late'));
  assert.equal(await read, 'superseded');
  assert.deepEqual(f.errors, []);
  assert.equal(f.controller.pending, false);
});
