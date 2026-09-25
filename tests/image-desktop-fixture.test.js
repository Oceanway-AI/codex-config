import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { startImageDesktopFixture, FIXTURE_KEY } from './image-desktop-fixture.mjs';

const imageBytes = await readFile(new URL('../src/assets/oceanway-ai-icon.png', import.meta.url));
const headers = { authorization: `Bearer ${FIXTURE_KEY}`, 'content-type': 'application/json' };

test('desktop fixture never proxies image requests and identifies partial failures by prompt', async () => {
  const fixture = await startImageDesktopFixture({ imageBytes });
  try {
    fixture.setMode('partial');
    const send = prompt => fetch(`${fixture.baseUrl}/images/generations`, {
      method: 'POST', headers, body: JSON.stringify({ model: 'gpt-image-2', prompt, n: 1 }),
    });
    const failed = await send('Render the literal OW_FAIL label.');
    assert.equal(failed.status, 502);
    const good = await send('Render a successful fixture.');
    assert.equal(good.status, 200);
    assert.deepEqual(Buffer.from((await good.json()).data[0].b64_json, 'base64'), imageBytes);
    assert.equal(fixture.audit.length, 2);
    assert.equal((await fetch(`${fixture.baseUrl}/responses`, { method: 'POST', headers, body: '{}' })).status, 404);
    assert.equal((await fetch(`${fixture.baseUrl}/images/generations`, {
      method: 'POST', headers: { ...headers, authorization: 'Bearer unrelated-key' }, body: '{}',
    })).status, 401);
  } finally { await fixture.close(); }
});

test('desktop fixture preserves actual multipart original bytes and duplicate reference order', async () => {
  const fixture = await startImageDesktopFixture({ imageBytes });
  try {
    const form = new FormData();
    form.set('model', 'gpt-image-2');
    form.set('prompt', 'Use both original roles.');
    form.set('n', '1');
    for (const name of ['first.png', 'second.png', 'first.png']) {
      form.append('image[]', new Blob([imageBytes], { type: 'image/png' }), name);
    }
    const response = await fetch(`${fixture.baseUrl}/images/edits`, {
      method: 'POST', headers: { authorization: `Bearer ${FIXTURE_KEY}` }, body: form,
    });
    assert.equal(response.status, 200);
    assert.deepEqual(fixture.audit[0].references.map(ref => ref.name), ['first.png', 'second.png', 'first.png']);
    assert.ok(fixture.audit[0].references.every(ref =>
      ref.sha256 === createHash('sha256').update(imageBytes).digest('hex')));
  } finally { await fixture.close(); }
});

test('desktop fixture rejects unsafe text upstream configuration before listening', async () => {
  for (const textBaseUrl of ['http://example.invalid', 'https://u:p@example.invalid', 'https://example.invalid?key=x']) {
    await assert.rejects(startImageDesktopFixture({ imageBytes, textBaseUrl, textKey: 'fake', allowTextProxy: true }));
  }
});

test('text proxy rejects hosted image and remote tools without any upstream request', async t => {
  const originalFetch = globalThis.fetch;
  let upstreamCalls = 0;
  t.mock.method(globalThis, 'fetch', async (url, options) => {
    if (String(url).startsWith('https://example.invalid/')) {
      upstreamCalls += 1;
      return new Response('{"output":[]}', { headers: { 'content-type': 'application/json' } });
    }
    return originalFetch(url, options);
  });
  const fixture = await startImageDesktopFixture({ imageBytes, textBaseUrl: 'https://example.invalid/v1',
    textKey: 'fake-upstream', textModels: ['test-text'], allowTextProxy: true });
  try {
    for (const payload of [
      { tools: [{ type: 'image_generation' }] },
      { tools: [{ type: 'mcp', server_url: 'https://example.invalid' }] },
      { tools: [{ type: 'namespace', tools: [{ type: 'image_generation' }] }] },
      { tool_choice: { type: 'image_generation' } },
      { modalities: ['text', 'image'] },
      { model: 'gpt-image-2' },
      { tools: {} },
    ]) {
      const response = await fetch(`${fixture.baseUrl}/responses`, { method: 'POST', headers,
        body: JSON.stringify({ model: 'test-text', ...payload }) });
      assert.equal(response.status, 400);
    }
    assert.equal(upstreamCalls, 0);
    assert.equal(fixture.audit.length, 0);
    const response = await fetch(`${fixture.baseUrl}/responses`, { method: 'POST', headers,
      body: JSON.stringify({ model: 'test-text', input: 'Test',
        tools: [{ type: 'namespace', tools: [{ type: 'function', name: 'generate_images' }] }] }) });
    assert.equal(response.status, 200);
    assert.equal(upstreamCalls, 1);
  } finally { await fixture.close(); }
});
