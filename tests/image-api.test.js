import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildImageRequest, DEFAULT_IMAGE_MODEL, imageTestBlockReason, createImageEvidence, createImageJobController, retryableImageCount, safeImagePreview } from '../src/image-api.js';

const request = { model: DEFAULT_IMAGE_MODEL, prompt: ' A red square ', count: 1 };
function makeJob(status = 'running', items = [{ index: 1, status: 'running' }]) {
  return { id: 'job-1', status, model: DEFAULT_IMAGE_MODEL, mode: 'generate', total: items.length,
    completed: items.filter(x => x.status === 'succeeded').length,
    failed: items.filter(x => x.status === 'failed').length,
    cancelled: items.filter(x => x.status === 'cancelled').length, items, message: 'simulation' };
}
function harness(invoke) {
  const changes = [], errors = [], scheduled = new Map();
  let next = 0;
  const controller = createImageJobController({
    invoke, onChange: state => changes.push(state), onError: error => errors.push(String(error)),
    schedule: callback => { scheduled.set(++next, callback); return next; },
    unschedule: id => scheduled.delete(id),
  });
  return { controller, changes, errors, scheduled };
}

test('request defaults only unset counts and accepts counts above former small caps', () => {
  assert.equal(DEFAULT_IMAGE_MODEL, 'gpt-image-2');
  for (const count of ['', undefined, null]) assert.equal(buildImageRequest({ ...request, count }).count, 1);
  for (const count of [1, 17, '12345', 100000]) assert.equal(buildImageRequest({ ...request, count }).count, Number(count));
  for (const count of [0, -1, 1.5, 'no', Infinity, NaN, Number.MAX_SAFE_INTEGER + 1, ' ']) {
    assert.throws(() => buildImageRequest({ ...request, count }), /正整数/);
  }
  assert.deepEqual(buildImageRequest({ ...request, referencePaths: ['a.png', 'b.png', 'a.png'] }),
    { model: 'gpt-image-2', prompt: 'A red square', count: 1, referencePaths: ['a.png', 'b.png', 'a.png'], size: '1024x1024' });
  assert.throws(() => buildImageRequest({ ...request, prompt: ' ' }), /提示词/);
  assert.throws(() => buildImageRequest({ ...request, model: '' }), /模型/);
});

test('reference paths preserve role order and duplicate entries without retaining the input array', () => {
  const referencePaths = ['subject.png', 'style.png', 'subject.png', 'subject.png'];
  const result = buildImageRequest({ ...request, referencePaths });
  assert.deepEqual(result.referencePaths, referencePaths);
  assert.notEqual(result.referencePaths, referencePaths);
  referencePaths.reverse();
  referencePaths.push('later.png');
  assert.deepEqual(result.referencePaths, ['subject.png', 'style.png', 'subject.png', 'subject.png']);
});

test('size defaults to 1024x1024 without overriding explicit selections', () => {
  assert.equal(buildImageRequest(request).size, '1024x1024');
  assert.equal(buildImageRequest({ ...request, size: undefined }).size, '1024x1024');
  for (const size of ['auto', '1536x1024', '1024x1536']) {
    assert.equal(buildImageRequest({ ...request, size }).size, size);
  }
});

test('retry count includes failed and cancelled slots without counting successes twice', () => {
  assert.equal(retryableImageCount(null), 0);
  assert.equal(retryableImageCount(makeJob('completed', [{ index: 1, status: 'succeeded' }])), 0);
  const job = makeJob('partial', [
    { index: 1, status: 'succeeded' }, { index: 2, status: 'failed' }, { index: 3, status: 'cancelled' },
  ]);
  assert.equal(retryableImageCount(job), 2);
  assert.equal(retryableImageCount({ ...job, failed: 0, cancelled: 0 }), 2);
  assert.equal(retryableImageCount({ ...job, items: [] }), 2);
});

test('saved config gate rejects dirty, missing config, maintenance and browser preview', () => {
  const state = { native: true, configured: true, dirty: false, busy: false };
  assert.equal(imageTestBlockReason(state), '');
  assert.match(imageTestBlockReason({ ...state, dirty: true }), /未保存/);
  assert.match(imageTestBlockReason({ ...state, native: false }), /不能执行真实/);
  assert.match(imageTestBlockReason({ ...state, configured: false }), /先保存/);
  assert.match(imageTestBlockReason({ ...state, busy: true }), /维护/);
});

test('evidence separates model listing, model identity, generate and edit and expires on credential changes', () => {
  const evidence = createImageEvidence();
  evidence.record('gpt-image-2', 'models', 'checked');
  assert.equal(evidence.status('gpt-image-2', 'generate'), 'untested');
  evidence.record('gpt-image-2', 'generate', 'verified');
  evidence.record('gpt-image-2', 'edit', 'failed');
  assert.equal(evidence.status('other-model', 'generate'), 'untested');
  assert.equal(evidence.status('gpt-image-2', 'edit'), 'failed');
  const oldRevision = evidence.revision;
  evidence.invalidate();
  for (const mode of ['models', 'generate', 'edit']) assert.equal(evidence.status('gpt-image-2', mode), 'outdated');
  evidence.record('gpt-image-2', 'edit', 'verified', oldRevision);
  assert.equal(evidence.status('gpt-image-2', 'edit'), 'outdated');
  assert.equal(createImageEvidence().status('gpt-image-2', 'generate'), 'untested');
});

test('only raster data URLs can become thumbnail sources', () => {
  const good = 'data:image/png;base64,iVBORw0KGgo=';
  assert.equal(safeImagePreview(good), good);
  for (const bad of ['https://example.com/a.png', 'file:///secret.png', 'data:image/svg+xml;base64,AA==', 'javascript:alert(1)', undefined]) assert.equal(safeImagePreview(bad), '');
});

test('controller is inert until explicit start and invokes exact backend payload without credentials', async () => {
  const calls = [];
  const { controller, scheduled } = harness(async (command, args) => { calls.push([command, args]); return makeJob(); });
  assert.equal(calls.length, 0);
  await controller.start(buildImageRequest(request));
  assert.deepEqual(calls[0], ['test_image_api', { request: buildImageRequest(request) }]);
  assert.equal(controller.locked, true);
  assert.equal(scheduled.size, 1);
  await controller.start(request);
  await controller.retry();
  assert.equal(calls.length, 1);
  controller.dispose();
  assert.equal(scheduled.size, 0);
});

test('poll errors preserve running state and schedule recovery; cancellation remains available', async () => {
  const calls = [];
  const { controller, errors, scheduled } = harness(async (command, args) => {
    calls.push([command, args]);
    if (command === 'get_image_test_status') throw new Error('offline');
    return command === 'cancel_image_test' ? makeJob('cancelled', [{ index: 1, status: 'cancelled' }]) : makeJob();
  });
  await controller.start(request);
  await controller.poll();
  assert.equal(controller.locked, true);
  assert.match(errors[0], /offline/);
  assert.equal(scheduled.size, 1);
  await controller.cancel();
  assert.deepEqual(calls.at(-1), ['cancel_image_test', { jobId: 'job-1' }]);
  assert.equal(controller.job.status, 'cancelled');
  assert.equal(controller.locked, false);
  assert.equal(scheduled.size, 0);
});

test('retry requires explicit action, preserves successes and never resends original request', async () => {
  const success = { index: 1, status: 'succeeded', path: 'saved.png', previewDataUrl: 'data:image/png;base64,AA==' };
  const failed = { index: 2, status: 'failed', error: 'simulation failure' };
  const calls = [];
  const { controller } = harness(async (command, args) => {
    calls.push([command, args]);
    return command === 'retry_image_test' ? makeJob('running', [{ index: 1, status: 'queued' }, { index: 2, status: 'running' }]) : makeJob('partial', [success, failed]);
  });
  await controller.start(request);
  assert.equal(calls.length, 1);
  assert.equal(controller.locked, false);
  await controller.retry();
  assert.deepEqual(calls[1], ['retry_image_test', { jobId: 'job-1' }]);
  assert.deepEqual(controller.job.items[0], success);
  assert.equal(controller.job.items[1].status, 'running');
  assert.deepEqual(controller.job.items.map(item => item.index), [1, 2]);
  controller.dispose();
});

test('cancel wins against a stale in-flight polling response', async () => {
  let finishPoll;
  const { controller } = harness(async command => {
    if (command === 'get_image_test_status') return new Promise(resolve => { finishPoll = resolve; });
    if (command === 'cancel_image_test') return makeJob('cancelled', [{ index: 1, status: 'cancelled' }]);
    return makeJob();
  });
  await controller.start(request);
  const poll = controller.poll();
  await controller.cancel();
  finishPoll(makeJob());
  await poll;
  assert.equal(controller.job.status, 'cancelled');
  assert.equal(controller.locked, false);
});

test('explicit retry resumes purely cancelled jobs from aggregate or slot status and preserves successes', async () => {
  const success = { index: 1, status: 'succeeded', path: 'saved.png', previewDataUrl: 'data:image/png;base64,AA==' };
  for (const cancelledJob of [
    makeJob('cancelled', [success, { index: 2, status: 'cancelled' }]),
    { ...makeJob('cancelled', [success]), total: 2, cancelled: 1 },
    { ...makeJob('cancelled', [success, { index: 2, status: 'cancelled' }]), cancelled: 0 },
    makeJob('cancelled', [{ index: 1, status: 'cancelled' }]),
  ]) {
    const calls = [];
    const { controller } = harness(async (command, args) => {
      calls.push([command, args]);
      return command === 'retry_image_test'
        ? makeJob('running', [{ index: 1, status: 'queued' }, { index: 2, status: 'running' }])
        : cancelledJob;
    });
    await controller.start(request);
    assert.equal(calls.length, 1);
    assert.equal(controller.locked, false);
    await controller.retry();
    assert.deepEqual(calls[1], ['retry_image_test', { jobId: 'job-1' }]);
    assert.equal(controller.locked, true);
    if (cancelledJob.completed) assert.deepEqual(controller.job.items[0], success);
    await controller.retry();
    assert.equal(calls.length, 2);
    controller.dispose();
  }
});

test('completed jobs with no missing slots cannot be retried', async () => {
  const calls = [];
  const { controller } = harness(async command => {
    calls.push(command);
    return makeJob('completed', [{ index: 1, status: 'succeeded', path: 'saved.png' }]);
  });
  await controller.start(request);
  await controller.retry();
  assert.deepEqual(calls, ['test_image_api']);
});

test('failed cancel retains lock and failed submission cannot pretend completion', async () => {
  const { controller, errors } = harness(async command => {
    if (command === 'cancel_image_test') throw new Error('cancel failed');
    return makeJob();
  });
  await controller.start(request);
  await controller.cancel();
  assert.equal(controller.locked, true);
  assert.equal(controller.job.status, 'running');
  assert.match(errors[0], /cancel failed/);
  controller.dispose();
  const rejected = harness(async () => { throw new Error('submission failed'); });
  await rejected.controller.start(request);
  assert.equal(rejected.controller.job, null);
  assert.equal(rejected.controller.locked, false);
  assert.match(rejected.errors[0], /submission failed/);
});

test('invalid backend responses cannot fabricate completed jobs', async () => {
  const { controller, errors } = harness(async () => ({}));
  await controller.start(request);
  assert.equal(controller.job, null);
  assert.match(errors[0], /格式无效/);
});
