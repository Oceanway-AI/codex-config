// Explicit packaged-binary acceptance. No desktop UI or real provider is exercised.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { access, mkdir, mkdtemp, readFile, realpath, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { createInterface } from 'node:readline';
import { setTimeout as sleep } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { deflateSync } from 'node:zlib';
import { FIXTURE_KEY, startImageDesktopFixture } from './image-desktop-fixture.mjs';

const options = new Map();
for (let index = 2; index < process.argv.length; index += 2) {
  const name = process.argv[index], value = process.argv[index + 1];
  assert.ok(['--executable', '--report'].includes(name) && value && !options.has(name),
    'Usage: node tests/mcp-bundle-images.mjs --executable PATH [--report tests/results/NAME.json]');
  options.set(name, value);
}
assert.ok(options.has('--executable'), 'Pass the packaged executable explicitly.');
const executable = resolve(options.get('--executable'));
assert.ok((await stat(executable)).isFile());
const repo = fileURLToPath(new URL('../', import.meta.url));
const reportPath = options.has('--report') ? resolve(options.get('--report')) : null;
if (reportPath) assertWithin(join(repo, 'tests', 'results'), reportPath);
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const imageBytes = await readFile(new URL('../src/assets/oceanway-ai-icon.png', import.meta.url));
const report = {
  executable, executableSha256: sha256(await readFile(executable)),
  fixtureSha256: sha256(await readFile(new URL('./image-desktop-fixture.mjs', import.meta.url))),
  fixtureImageSha256: sha256(imageBytes), node: process.version,
  startedAt: new Date().toISOString(), network: 'numeric loopback only; text proxy disabled',
  acceptance: 'packaged stdio MCP; CLI setup only, not desktop UI acceptance',
  tests: [], processes: [], jobs: [], cleanup: {},
};
const root = await mkdtemp(join(tmpdir(), 'oceanway-packaged-images-'));
const home = join(root, 'codex-home'), workspace = join(root, 'workspace');
const children = [];
let fixture, peer, failure, unexpectedFetchCalls = 0;
const originalFetch = globalThis.fetch;
// The fixture's optional upstream branch must never execute in this harness.
globalThis.fetch = async () => {
  unexpectedFetchCalls += 1;
  throw new Error('Outbound fetch is forbidden in packaged image acceptance.');
};

function assertWithin(parent, child) {
  const path = relative(resolve(parent), resolve(child));
  assert.ok(path && !isAbsolute(path) && path !== '..' && !path.startsWith(`..${sep}`),
    'Path must remain strictly inside the harness-owned directory.');
}

async function waitFor(predicate, label, milliseconds = 20000) {
  const deadline = Date.now() + milliseconds;
  while (Date.now() < deadline) {
    const value = await predicate();
    if (value) return value;
    await sleep(50);
  }
  throw new Error(`Timed out: ${label}`);
}

async function bounded(promise, label, milliseconds = 15000) {
  let timer;
  try {
    return await Promise.race([promise, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`Timed out: ${label}`)), milliseconds);
    })]);
  } finally { clearTimeout(timer); }
}

function isolatedEnvironment() {
  const env = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (/^(systemroot|windir|systemdrive|comspec|path|pathext|number_of_processors|processor_architecture)$/i.test(key)) {
      env[key] = value;
    }
  }
  return { ...env, CODEX_HOME: home, HOME: join(root, 'profile'), USERPROFILE: join(root, 'profile'),
    APPDATA: join(root, 'appdata'), LOCALAPPDATA: join(root, 'localappdata'),
    XDG_CONFIG_HOME: join(root, 'xdg'), TEMP: join(root, 'temp'), TMP: join(root, 'temp') };
}

function childProcess(role, args) {
  const record = { role, pid: null, exited: false, code: null, signal: null, forced: false };
  report.processes.push(record);
  const child = spawn(executable, args, {
    cwd: workspace, windowsHide: true, env: isolatedEnvironment(), stdio: ['pipe', 'pipe', 'pipe'],
  });
  record.pid = child.pid ?? null;
  let stderr = '';
  child.stderr.on('data', bytes => { stderr = (stderr + bytes.toString()).slice(-4000); });
  const exit = new Promise(resolveExit => {
    child.once('error', error => { record.spawnError = error.message; });
    child.once('close', (code, signal) => {
      Object.assign(record, { exited: true, code, signal });
      resolveExit({ code, signal, stderr, spawnError: record.spawnError });
    });
  });
  const handle = { child, exit, record, force() {
    if (!record.exited) {
      record.forced = true;
      child.kill('SIGKILL');
    }
  } };
  children.push(handle);
  return handle;
}

class McpPeer {
  constructor(role) {
    this.handle = childProcess(role, ['--image-mcp-stdio']);
    this.serial = 0;
    this.pending = new Map();
    this.lines = createInterface({ input: this.handle.child.stdout });
    this.lines.on('line', line => {
      try {
        assert.ok(Buffer.byteLength(line) <= 2 * 1024 * 1024, 'MCP response exceeds harness limit.');
        const message = JSON.parse(line);
        const waiter = this.pending.get(message.id);
        if (!waiter) return;
        this.pending.delete(message.id);
        clearTimeout(waiter.timer);
        if (message.error) waiter.reject(new Error(`MCP error: ${JSON.stringify(message.error)}`));
        else waiter.resolve(message.result);
      } catch (error) { this.fail(error); }
    });
    this.handle.child.stdin.on('error', error => this.fail(error));
    this.handle.child.on('error', error => this.fail(error));
    this.handle.exit.then(() => {
      this.fail(new Error('Packaged MCP process exited.'));
      this.lines.close();
    });
  }

  fail(error) {
    for (const waiter of this.pending.values()) {
      clearTimeout(waiter.timer);
      waiter.reject(error);
    }
    this.pending.clear();
  }

  request(method, params) {
    const id = ++this.serial;
    return new Promise((resolveRequest, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`MCP ${method} timed out.`));
      }, 30000);
      this.pending.set(id, { resolve: resolveRequest, reject, timer });
      this.handle.child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
    });
  }

  async initialize() {
    const initialized = await this.request('initialize', {
      protocolVersion: '2025-06-18', capabilities: {},
      clientInfo: { name: 'oceanway-packaged-image-acceptance', version: '1' },
    });
    assert.ok(initialized.capabilities?.tools);
    this.handle.child.stdin.write(`${JSON.stringify({
      jsonrpc: '2.0', method: 'notifications/initialized',
    })}\n`);
    const listed = await this.request('tools/list', {});
    assert.deepEqual(listed.tools.map(tool => tool.name).sort(),
      ['generate_images', 'get_image_job', 'cancel_image_job', 'retry_image_job'].sort());
    const schema = listed.tools.find(tool => tool.name === 'generate_images').inputSchema;
    assert.equal(schema.properties.count.maximum, undefined);
    assert.equal(schema.properties.model.default, 'gpt-image-2');
    assert.equal(schema.properties.count.default, 1);
  }

  async call(name, args, expectError = false) {
    const result = await this.request('tools/call', { name, arguments: args });
    assert.equal(result.isError === true, expectError, `Unexpected tool outcome: ${name}`);
    assert.ok(result.structuredContent, 'Missing structured MCP content.');
    const serialized = JSON.stringify(result.structuredContent);
    assert.ok(!serialized.includes('previewDataUrl') && !serialized.includes(FIXTURE_KEY),
      'Structured job must not contain duplicated previews or credentials.');
    assert.ok(Buffer.byteLength(JSON.stringify(result)) < 1024 * 1024, 'Oversized tool result.');
    for (const content of result.content ?? []) {
      if (content.type === 'image') {
        assert.equal(content.mimeType, 'image/png');
        assert.ok(Buffer.from(content.data, 'base64').length <= 256 * 1024);
      }
    }
    return result.structuredContent;
  }

  generate(args) {
    return this.call('generate_images', { workspace_directory: workspace, ...args });
  }

  status(id) { return this.call('get_image_job', { job_id: id, wait_seconds: 0 }); }
  retry(id) { return this.call('retry_image_job', { job_id: id }); }
  cancel(id) { return this.call('cancel_image_job', { job_id: id }); }

  async finished(id) {
    return waitFor(async () => {
      const job = await this.status(id);
      return job.status !== 'running' && job;
    }, `terminal image job ${id}`);
  }

  endInput() { if (!this.handle.child.stdin.destroyed) this.handle.child.stdin.end(); }

  async close() {
    this.endInput();
    const result = await bounded(this.handle.exit, 'clean MCP EOF');
    assert.equal(result.code, 0, `MCP EOF failed: ${result.stderr}`);
    return result;
  }
}

async function scenario(name, run) {
  const started = Date.now();
  try {
    const evidence = await run();
    report.tests.push({ name, status: 'pass', durationMs: Date.now() - started, evidence });
    console.log(`PASS ${name}`);
  } catch (error) {
    report.tests.push({ name, status: 'fail', durationMs: Date.now() - started, error: error.message.slice(0, 2000) });
    throw error;
  }
}

async function outputEvidence(job) {
  assert.equal(job.persistenceError, null);
  const directory = await realpath(job.outputDirectory);
  assertWithin(await realpath(workspace), directory);
  assert.equal(basename(directory), job.id);
  const outputs = [];
  for (const item of job.items.filter(item => item.status === 'succeeded')) {
    const path = await realpath(item.path);
    assertWithin(directory, path);
    const bytes = await readFile(path);
    assert.equal(sha256(bytes), item.sha256);
    assert.equal(item.sha256, report.fixtureImageSha256);
    assert.equal(item.width, imageBytes.readUInt32BE(16));
    assert.equal(item.height, imageBytes.readUInt32BE(20));
    assert.equal(item.outputs[0].sha256, item.sha256);
    assert.equal(item.outputs[0].bytes, bytes.length);
    outputs.push({ index: item.index, path, sha256: item.sha256, requestId: item.requestId });
  }
  assert.equal(outputs.length, job.completed);
  report.jobs.push({ id: job.id, status: job.status, total: job.total, completed: job.completed,
    failed: job.failed, cancelled: job.cancelled, outcomeUnknown: job.outcomeUnknown, outputs });
  return outputs;
}

async function manifest(job) {
  return JSON.parse(await readFile(join(job.outputDirectory, 'manifest.json'), 'utf8'));
}

// Small deterministic, valid PNG originals with distinct pixels and dimensions.
function originalPng(width, height, rgb) {
  const crc32 = bytes => {
    let crc = 0xffffffff;
    for (const byte of bytes) {
      crc ^= byte;
      for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
    }
    return (crc ^ 0xffffffff) >>> 0;
  };
  const chunk = (type, body) => {
    const data = Buffer.concat([Buffer.from(type), body]);
    const header = Buffer.alloc(4), footer = Buffer.alloc(4);
    header.writeUInt32BE(body.length);
    footer.writeUInt32BE(crc32(data));
    return Buffer.concat([header, data, footer]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8;
  header[9] = 2;
  const rows = Buffer.alloc(height * (1 + width * 3));
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) Buffer.from(rgb).copy(rows, y * (1 + width * 3) + 1 + x * 3);
  }
  return Buffer.concat([Buffer.from('89504e470d0a1a0a', 'hex'), chunk('IHDR', header),
    chunk('IDAT', deflateSync(rows)), chunk('IEND', Buffer.alloc(0))]);
}

try {
  for (const directory of [home, workspace, 'profile', 'appdata', 'localappdata', 'xdg', 'temp']) {
    await mkdir(isAbsolute(directory) ? directory : join(root, directory));
  }
  fixture = await startImageDesktopFixture({ imageBytes, allowTextProxy: false });
  const fixtureUrl = new URL(fixture.baseUrl);
  assert.equal(fixtureUrl.protocol, 'http:');
  assert.equal(fixtureUrl.hostname, '127.0.0.1');
  report.sandbox = root;

  await scenario('CLI mock setup and packaged MCP discovery send no image POST', async () => {
    const configured = childProcess('mock-only CLI setup', [
      '--provider-id', 'OceanWay', '--base-url', fixture.baseUrl,
      '--model', 'gpt-image-2', '--api-key', FIXTURE_KEY,
    ]);
    configured.child.stdout.resume();
    configured.child.stdin.end();
    const result = await bounded(configured.exit, 'mock-only CLI setup');
    assert.equal(result.code, 0, result.stderr);
    const auth = JSON.parse(await readFile(join(home, 'auth.json'), 'utf8'));
    assert.equal(auth.OPENAI_API_KEY, FIXTURE_KEY);
    const config = await readFile(join(home, 'config.toml'), 'utf8');
    assert.ok(config.includes(fixture.baseUrl));
    assert.ok(!config.includes(FIXTURE_KEY));
    peer = new McpPeer('initial MCP session');
    await peer.initialize();
    assert.equal(fixture.audit.length, 0);
    return { imagePosts: 0, toolCount: 4 };
  });

  await scenario('seven distinct prompts retain exact count and slot-to-request mapping', async () => {
    const before = fixture.audit.length;
    const prompts = Array.from({ length: 7 }, (_, index) => `batch slot ${index + 1}`);
    const job = await peer.generate({ prompts, count: prompts.length, size: '320x160' });
    const done = await peer.finished(job.id);
    assert.equal(done.status, 'completed');
    assert.equal(done.completed, 7);
    assert.equal(done.failed, 0);
    assert.equal(fixture.audit.length - before, 7);
    const audit = new Map(fixture.audit.slice(before).map(entry => [entry.id, entry]));
    for (const item of done.items) {
      assert.equal(audit.get(item.requestId).prompt, prompts[item.index - 1]);
      assert.equal(audit.get(item.requestId).n, 1);
      assert.equal(audit.get(item.requestId).size, '320x160');
      assert.equal(audit.get(item.requestId).model, 'gpt-image-2');
    }
    await outputEvidence(done);
    return { jobId: job.id, completed: done.completed, imagePosts: 7 };
  });

  await scenario('ordered duplicate attachments preserve original bytes and hashes', async () => {
    const originals = [originalPng(3, 5, [210, 30, 70]), originalPng(7, 2, [20, 160, 190])];
    const paths = [join(workspace, 'original-first.png'), join(workspace, 'original-second.png')];
    await Promise.all(paths.map((path, index) => writeFile(path, originals[index])));
    const references = [paths[0], paths[1], paths[0]];
    const hashes = [sha256(originals[0]), sha256(originals[1]), sha256(originals[0])];
    assert.notEqual(hashes[0], hashes[1]);
    const before = fixture.audit.length;
    const job = await peer.generate({ prompts: ['edit first view', 'edit second view'],
      count: 2, reference_paths: references });
    const done = await peer.finished(job.id);
    assert.equal(done.status, 'completed');
    assert.deepEqual(done.referenceHashes, hashes);
    assert.equal(fixture.audit.length - before, 2);
    for (const entry of fixture.audit.slice(before)) {
      assert.equal(entry.path, '/v1/images/edits');
      assert.deepEqual(entry.references.map(reference => reference.sha256), hashes);
      assert.deepEqual(entry.references.map(reference => reference.bytes),
        [originals[0].length, originals[1].length, originals[0].length]);
      assert.deepEqual(entry.references.map(reference => reference.field), ['image[]', 'image[]', 'image[]']);
      assert.deepEqual(entry.references.map(reference => reference.name),
        ['reference-1.png', 'reference-2.png', 'reference-3.png']);
      assert.ok(entry.references.every(reference => reference.type === 'image/png'));
    }
    for (let index = 0; index < paths.length; index++) {
      assert.deepEqual(await readFile(paths[index]), originals[index]);
    }
    assert.deepEqual((await manifest(done)).job.referenceHashes, hashes);
    await outputEvidence(done);
    return { jobId: job.id, imagePosts: 2, orderedReferenceHashes: hashes };
  });

  await scenario('partial failure retries only failed slots and preserves successful files', async () => {
    fixture.setMode('partial');
    const before = fixture.audit.length;
    const prompts = ['partial first', 'OW_FAIL partial second', 'partial third', 'OW_FAIL partial fourth', 'partial fifth'];
    const job = await peer.generate({ prompts, count: 5 });
    const partial = await peer.finished(job.id);
    assert.equal(partial.status, 'partial');
    assert.deepEqual([partial.completed, partial.failed], [3, 2]);
    const saved = await outputEvidence(partial);
    assert.equal(fixture.audit.length - before, 5);
    await sleep(250);
    assert.equal(fixture.audit.length - before, 5, 'No automatic retry.');
    fixture.setMode('success');
    await peer.retry(job.id);
    const done = await peer.finished(job.id);
    assert.equal(done.status, 'completed');
    assert.equal(done.completed, 5);
    for (const output of saved) {
      const item = done.items.find(item => item.index === output.index);
      assert.equal(item.requestId, output.requestId);
      assert.equal(sha256(await readFile(output.path)), output.sha256);
    }
    assert.deepEqual(fixture.audit.slice(before + 5).map(entry => entry.prompt).sort(),
      [prompts[1], prompts[3]].sort());
    await peer.call('retry_image_job', { job_id: job.id }, true);
    assert.equal(fixture.audit.length - before, 7, 'Completed retry must not send another POST.');
    await outputEvidence(done);
    return { jobId: job.id, initialCompleted: 3, initialFailed: 2, explicitRetryPosts: 2 };
  });

  await scenario('live ownership lock prevents another MCP process from mutating the manifest', async () => {
    fixture.setMode('stall');
    const before = fixture.audit.length;
    const job = await peer.generate({ prompt: 'live owner fixture', count: 9 });
    await waitFor(() => fixture.audit.length === before + 2 && fixture.stats().active === 2, 'two active owner requests');
    const path = join(job.outputDirectory, 'manifest.json');
    const original = await readFile(path);
    const observer = new McpPeer('concurrent readback session');
    await observer.initialize();
    await observer.call('get_image_job', { job_id: job.id, wait_seconds: 0 }, true);
    await observer.close();
    assert.deepEqual(await readFile(path), original);
    assert.equal(fixture.audit.length - before, 2);
    const cancelled = await peer.cancel(job.id);
    assert.equal(cancelled.cancelled, 7);
    fixture.setMode('success');
    fixture.release();
    const done = await peer.finished(job.id);
    assert.equal(done.status, 'cancelled');
    assert.deepEqual([done.completed, done.failed, done.cancelled], [2, 0, 7]);
    assert.equal(fixture.audit.length - before, 2);
    await outputEvidence(done);
    return { jobId: job.id, manifestUnchangedByObserver: true, admittedSaved: 2, queuedCancelled: 7 };
  });

  await scenario('stdin EOF cancels queued work and waits for admitted outputs to save', async () => {
    fixture.setMode('stall');
    const before = fixture.audit.length;
    const job = await peer.generate({ prompt: 'EOF drain fixture', count: 6 });
    await waitFor(() => fixture.audit.length === before + 2 && fixture.stats().active === 2, 'two active EOF requests');
    peer.endInput();
    await waitFor(async () => (await manifest(job)).job.cancelled === 4, 'EOF durable queue cancellation');
    assert.equal(peer.handle.record.exited, false, 'Server must drain admitted requests before exiting.');
    fixture.setMode('success');
    fixture.release();
    await peer.close();
    assert.equal(fixture.audit.length - before, 2);
    const saved = (await manifest(job)).job;
    assert.deepEqual([saved.completed, saved.failed, saved.cancelled], [2, 0, 4]);
    await outputEvidence(saved);
    peer = new McpPeer('after clean EOF recovery');
    await peer.initialize();
    const recovered = await peer.status(job.id);
    assert.equal(recovered.status, 'interrupted');
    assert.equal(recovered.completed, 2);
    await sleep(250);
    assert.equal(fixture.audit.length - before, 2, 'Recovery/poll must never resume POSTs.');
    return { jobId: job.id, admittedSaved: 2, queuedCancelled: 4, recoveryPosts: 0 };
  });

  await scenario('killed-process recovery records uncertain slots without automatic POST', async () => {
    fixture.setMode('partial');
    const before = fixture.audit.length;
    const job = await peer.generate({ count: 5,
      prompts: ['crash first', 'OW_FAIL crash second', 'crash third', 'OW_FAIL crash fourth', 'crash fifth'] });
    const partial = await peer.finished(job.id);
    assert.deepEqual([partial.completed, partial.failed], [3, 2]);
    const saved = await outputEvidence(partial);
    fixture.setMode('stall');
    await peer.retry(job.id);
    await waitFor(() => fixture.audit.length === before + 7 && fixture.stats().active === 2, 'two retried requests before crash');
    peer.handle.force();
    await bounded(peer.handle.exit, 'terminate only the owned MCP child');
    fixture.release();
    await waitFor(() => fixture.stats().active === 0, 'fixture drops crashed connections');
    fixture.setMode('success');
    peer = new McpPeer('after forced process recovery');
    await peer.initialize();
    const recovered = await peer.status(job.id);
    assert.equal(recovered.status, 'interrupted');
    assert.equal(recovered.paused, true);
    assert.equal(recovered.completed, 3);
    assert.equal(recovered.outcomeUnknown, 2);
    assert.equal(recovered.items.filter(item => item.status === 'uncertain').length, 2);
    await sleep(250);
    assert.equal(fixture.audit.length - before, 7, 'Crash recovery must never automatically POST.');
    await peer.retry(job.id);
    const done = await peer.finished(job.id);
    assert.equal(done.status, 'completed');
    assert.equal(done.completed, 5);
    assert.equal(fixture.audit.length - before, 9);
    for (const output of saved) {
      assert.equal(done.items.find(item => item.index === output.index).requestId, output.requestId);
      assert.equal(sha256(await readFile(output.path)), output.sha256);
    }
    await outputEvidence(done);
    return { jobId: job.id, retainedSuccesses: 3, uncertainSlots: 2, recoveryPosts: 0, explicitRetryPosts: 2 };
  });

  await scenario('count mismatch returns warnings and retains additional outputs', async () => {
    fixture.setMode('extra-output');
    const before = fixture.audit.length;
    const job = await peer.generate({ prompt: 'extra output fixture', count: 1 });
    const done = await peer.finished(job.id);
    assert.equal(done.status, 'completed_with_warnings');
    assert.equal(done.countMismatch, true);
    assert.ok(done.warnings.length > 0);
    assert.equal(done.items[0].outputs.length, 2);
    for (const output of done.items[0].outputs) {
      assert.equal(sha256(await readFile(output.path)), report.fixtureImageSha256);
    }
    await peer.call('retry_image_job', { job_id: job.id }, true);
    assert.equal(fixture.audit.length - before, 1);
    await outputEvidence(done);
    return { jobId: job.id, imagePosts: 1, savedOutputs: 2, status: done.status };
  });
  await peer.close();
  assert.ok(fixture.stats().peak <= 2, 'At most two engine POSTs may be active within one process.');
  assert.equal(unexpectedFetchCalls, 0);
} catch (error) {
  failure = error;
  console.error(`FAIL ${error.message}`);
} finally {
  const cleanupErrors = [];
  for (const handle of children) {
    if (!handle.record.exited && !handle.child.stdin.destroyed) handle.child.stdin.end();
  }
  fixture?.setMode('success');
  fixture?.release();
  for (const handle of children) {
    try {
      await bounded(handle.exit, 'owned subprocess cleanup');
    } catch (error) {
      handle.force();
      try { await bounded(handle.exit, 'owned subprocess forced cleanup'); }
      catch (forcedError) { cleanupErrors.push(forcedError.message); }
      cleanupErrors.push(error.message);
    }
  }
  if (fixture) {
    report.fixtureStats = fixture.stats();
    report.audit = fixture.audit;
    try { await bounded(fixture.close(), 'fixture close'); report.cleanup.fixtureClosed = true; }
    catch (error) { cleanupErrors.push(error.message); }
  }
  report.unexpectedFetchCalls = unexpectedFetchCalls;
  globalThis.fetch = originalFetch;
  try {
    const actual = await realpath(root), parent = await realpath(tmpdir());
    assert.equal(dirname(actual), parent);
    assert.ok(basename(actual).startsWith('oceanway-packaged-images-'));
    assert.ok(children.every(handle => handle.record.exited), 'Refuse deletion while owned processes remain.');
    await rm(actual, { recursive: true, force: true });
    await assert.rejects(access(actual));
    report.cleanup.temporaryDirectoryRemoved = true;
  } catch (error) { cleanupErrors.push(error.message); }
  report.cleanup.allOwnedProcessesExited = children.every(handle => handle.record.exited);
  report.cleanup.errors = cleanupErrors;
  report.finishedAt = new Date().toISOString();
  report.status = !failure && cleanupErrors.length === 0 ? 'pass' : 'fail';
  if (failure) report.error = failure.message.slice(0, 2000);
  if (reportPath) {
    await mkdir(dirname(reportPath), { recursive: true });
    await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
  }
  console.log(`${report.status.toUpperCase()}: ${report.tests.filter(test => test.status === 'pass').length} scenarios; `
    + `${report.fixtureStats?.imageSubmissions ?? 0} loopback image POSTs; `
    + `cleanup ${cleanupErrors.length ? 'FAILED' : 'complete'}.`);
  if (report.status !== 'pass') process.exitCode = 1;
}
