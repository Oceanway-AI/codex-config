import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import sharp from 'sharp';
import { ImageService, apiRoot, digest, inside, isolatedPaths, readHomeFile } from './engine.mjs';
import { stripManagedRules } from './prepare.mjs';

const png = await sharp({ create: { width: 32, height: 32, channels: 3, background: '#19a077' } }).png().toBuffer();
const provider = { base: 'https://example.invalid/v1', key: 'sk-fake-test-only', identity: 'fake' };
async function fixture(t, request) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'oceanway-mcp-test-'));
  const service = new ImageService({ root, readProvider: async () => provider, request });
  t.after(async () => {
    await Promise.all([...service.jobs.values()].map(job => job.done));
    const real = await fs.realpath(root);
    assert.ok(inside(await fs.realpath(os.tmpdir()), real));
    assert.ok(path.basename(real).startsWith('oceanway-mcp-test-'));
    await fs.rm(real, { recursive: true, force: true });
  });
  return { root, service };
}

test('quantity and distinct prompts are preserved with at most two in flight', async t => {
  let active = 0, peak = 0;
  const prompts = [];
  const { root, service } = await fixture(t, async (_provider, request) => {
    peak = Math.max(peak, ++active);
    prompts.push(request.prompt);
    assert.equal(request.model, 'gpt-image-2');
    assert.equal(request.n, 1);
    await new Promise(resolve => setTimeout(resolve, 5));
    active--;
    return { images: [png], requestId: `fake-${prompts.length}` };
  });
  const expected = Array.from({ length: 6 }, (_, i) => `style ${i}`);
  const result = await service.start({ count: 6, prompts: expected, workspace_directory: root });
  assert.equal(result.completed, 6);
  assert.equal(peak, 2);
  assert.deepEqual(prompts, expected);
  assert.equal(new Set(result.items.flatMap(item => item.outputs.map(output => output.path))).size, 6);
  const audit = await fs.readFile(service.auditPath, 'utf8');
  assert.ok(!audit.includes(provider.key));
});

test('reference order and exact bytes reach edit requests; explicit model is retained', async t => {
  const { root, service } = await fixture(t, async (_provider, request, references) => {
    assert.equal(request.model, 'custom-test-image');
    assert.equal(references.length, 2);
    assert.equal(references[0].path, path.join(root, 'first.png'));
    assert.equal(references[1].path, path.join(root, 'second.png'));
    assert.equal(digest(references[0].bytes), digest(png));
    return { images: [png] };
  });
  await fs.writeFile(path.join(root, 'first.png'), png);
  await fs.writeFile(path.join(root, 'second.png'), png);
  const result = await service.start({ prompt: 'combine', model: 'custom-test-image', count: 3,
    reference_paths: [path.join(root, 'first.png'), path.join(root, 'second.png')], workspace_directory: root });
  assert.equal(result.mode, 'edit');
  assert.equal(result.completed, 3);
});

test('partial failures retain successful output and never auto retry', async t => {
  let requests = 0;
  const { root, service } = await fixture(t, async () => {
    if (++requests === 2) throw Error(`TimeoutError ${provider.key}`);
    return { images: [png] };
  });
  const result = await service.start({ prompt: 'test', count: 3, workspace_directory: root });
  assert.equal(requests, 3);
  assert.equal(result.completed, 2);
  assert.equal(result.failed, 1);
  assert.equal(result.status, 'partial');
  assert.ok(!JSON.stringify(result).includes(provider.key));
});

test('cancel stops queued slots and retains in-flight success', async t => {
  let requests = 0;
  let resolve;
  const pending = new Promise(yes => { resolve = yes; });
  const { root, service } = await fixture(t, async () => {
    requests++;
    await pending;
    return { images: [png] };
  });
  const result = await service.start({ prompt: 'test', count: 6, workspace_directory: root }, 0);
  while (requests < 2) await new Promise(resolve => setTimeout(resolve, 2));
  await service.cancel(result.id);
  resolve();
  const final = await service.wait(result.id);
  assert.equal(requests, 2);
  assert.equal(final.completed, 2);
  assert.equal(final.cancelled, 4);
});

test('cancellation during provider reload prevents claimed but unsent POSTs', async t => {
  let requests = 0, reads = 0, release;
  const wait = new Promise(resolve => { release = resolve; });
  const { root, service } = await fixture(t, async () => { requests++; return { images: [png] }; });
  service.readProvider = async () => {
    if (++reads > 1) await wait;
    return provider;
  };
  const job = await service.start({ prompt: 'test', count: 6, workspace_directory: root }, 0);
  await service.cancel(job.id);
  release();
  const result = await service.wait(job.id);
  assert.equal(requests, 0);
  assert.equal(result.cancelled, 6);
  assert.equal(result.pending, 0);
});

test('extra provider images are retained but never count as exact success', async t => {
  const { root, service } = await fixture(t, async () => ({ images: [png, png] }));
  const result = await service.start({ prompt: 'test', workspace_directory: root });
  assert.equal(result.status, 'partial');
  assert.equal(result.completed, 1);
  assert.equal(result.saved_images, 2);
  assert.equal(result.count_mismatch, true);
  const manifest = JSON.parse(await fs.readFile(path.join(result.directory, 'manifest.json'), 'utf8'));
  assert.equal(manifest.status, 'partial');
});

test('persistence failure remains explicit instead of claiming completion', async t => {
  const { root, service } = await fixture(t, async () => ({ images: [png] }));
  const persist = service.persist.bind(service);
  service.persist = (job, final) => final ? Promise.reject(Error('test disk failure')) : persist(job, final);
  const result = await service.start({ prompt: 'test', workspace_directory: root });
  assert.equal(result.status, 'partial');
  assert.equal(result.persistence_error, 'test disk failure');
});

test('late cancellation cannot overwrite the final manifest with running state', async t => {
  const { root, service } = await fixture(t, async () => ({ images: [png] }));
  const persist = service.persist.bind(service);
  let releaseFinal, enteredFinal;
  const finalGate = new Promise(resolve => { releaseFinal = resolve; });
  const finalEntered = new Promise(resolve => { enteredFinal = resolve; });
  let lateWrites = 0, finalStarted = false;
  service.persist = async (job, final) => {
    if (final) {
      finalStarted = true;
      await persist(job, final);
      enteredFinal();
      await finalGate;
    } else {
      if (finalStarted) lateWrites++;
      await persist(job, final);
    }
  };
  const started = await service.start({ prompt: 'test', workspace_directory: root }, 0);
  await finalEntered;
  let cancellationSettled = false;
  const cancellation = service.cancel(started.id).then(result => {
    cancellationSettled = true;
    return result;
  });
  try {
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(cancellationSettled, false);
  } finally {
    releaseFinal();
  }
  const result = await cancellation;
  assert.equal(result.status, 'completed');
  assert.equal(lateWrites, 0);
  const manifest = JSON.parse(await fs.readFile(path.join(result.directory, 'manifest.json'), 'utf8'));
  assert.equal(manifest.status, 'completed');
  assert.equal(manifest.cancelled, 0);
  assert.equal((await service.cancel(started.id)).status, 'completed');
});

test('linked configuration files are rejected', async t => {
  const { root } = await fixture(t, async () => ({ images: [png] }));
  await fs.writeFile(path.join(root, 'original.toml'), 'model = "fake"');
  await fs.link(path.join(root, 'original.toml'), path.join(root, 'config.toml'));
  await assert.rejects(readHomeFile(root, 'config.toml'), /linked/);
});

test('provider change stops requests and image impostors cannot succeed', async t => {
  let requests = 0;
  const { root, service } = await fixture(t, async () => { requests++; return { images: [Buffer.from('<html>error</html>')] }; });
  const invalid = await service.start({ prompt: 'test', workspace_directory: root });
  assert.equal(invalid.completed, 0);
  assert.equal(invalid.failed, 1);
  let reads = 0;
  service.readProvider = async () => ({ ...provider, identity: ++reads === 1 ? 'old' : 'new' });
  const changed = await service.start({ prompt: 'test', workspace_directory: root });
  assert.equal(changed.failed, 1);
  assert.equal(requests, 1);
});

test('address normalization, isolation guard and owned-rule removal', async () => {
  assert.equal(apiRoot('https://example.invalid/'), 'https://example.invalid/v1');
  assert.equal(apiRoot('https://example.invalid/v1/'), 'https://example.invalid/v1');
  assert.equal(apiRoot('https://example.invalid/api/v2'), 'https://example.invalid/api/v2');
  assert.throws(() => apiRoot('https://user:secret@example.invalid'));
  assert.equal(inside('C:\\test', 'C:\\test-other'), false);
  await assert.rejects(isolatedPaths({}));
  const rule = '<!-- OCEANWAY:DIRECT-IMAGE-API:BEGIN -->\nowned\n<!-- OCEANWAY:DIRECT-IMAGE-API:END -->\n\n';
  assert.equal(stripManagedRules(rule + 'Keep user rules.'), 'Keep user rules.');
  assert.throws(() => stripManagedRules(rule + rule));
});
