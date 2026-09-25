// Acceptance prototype only. The production desktop packaging is not changed.
import fs from 'node:fs/promises';
import path from 'node:path';
import dns from 'node:dns/promises';
import https from 'node:https';
import { randomUUID, createHash } from 'node:crypto';
import { parse } from 'smol-toml';
import ipaddr from 'ipaddr.js';
import sharp from 'sharp';

const MAX_IMAGE_BYTES = 50 * 1024 * 1024;
const MAX_RESPONSE_BYTES = 72 * 1024 * 1024;
const formats = { png: 'image/png', jpeg: 'image/jpeg', webp: 'image/webp' };
export const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

export function inside(root, candidate) {
  const relative = path.relative(path.resolve(root), path.resolve(candidate));
  return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative));
}

export async function isolatedPaths(env = process.env) {
  if (!env.OCEANWAY_MCP_TEST_ROOT || !env.CODEX_HOME) throw Error('Explicit isolated test paths are required.');
  const root = await fs.realpath(env.OCEANWAY_MCP_TEST_ROOT);
  const home = await fs.realpath(env.CODEX_HOME);
  if (home !== path.join(root, 'codex-home')) throw Error('Refusing a non-isolated CODEX_HOME.');
  const marker = JSON.parse(await fs.readFile(path.join(root, '.oceanway-isolation.json'), 'utf8'));
  if (marker.purpose !== 'oceanway-codex-isolated-acceptance') throw Error('Invalid isolation marker.');
  return { root, home };
}

export function apiRoot(base) {
  const url = new URL(base);
  if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password || url.search || url.hash) {
    throw Error('Invalid provider URL.');
  }
  url.pathname = url.pathname.replace(/\/+$/, '') || '/v1';
  return url.toString().replace(/\/$/, '');
}

export async function readHomeFile(home, name) {
  const file = path.join(home, name);
  const stat = await fs.lstat(file);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.nlink !== 1 || !inside(home, await fs.realpath(file))) {
    throw Error('Refusing a linked or non-regular configuration file.');
  }
  return fs.readFile(file, 'utf8');
}

export function providerReader(home, env = process.env) {
  return async () => {
    let config, auth;
    try {
      config = parse(await readHomeFile(home, 'config.toml'));
      auth = JSON.parse(await readHomeFile(home, 'auth.json'));
    } catch {
      throw Error('Cannot read or parse the saved configuration; no request was sent.');
    }
    if (config.profile) throw Error('This prototype does not resolve active profiles; no request was sent.');
    if (config.model_provider !== 'OceanWay') throw Error('OceanWay is not the active provider; no request was sent.');
    const provider = config.model_providers?.OceanWay;
    const key = provider?.experimental_bearer_token
      || (provider?.env_key && env[provider.env_key]) || auth.OPENAI_API_KEY;
    if (typeof key !== 'string' || !key.trim()) throw Error('No provider API key is configured.');
    const base = apiRoot(provider.base_url);
    return { base, key, identity: digest(`${base}\0${key}`) };
  };
}

async function imageInfo(bytes) {
  if (!bytes.length || bytes.length > MAX_IMAGE_BYTES) throw Error('Invalid image byte length.');
  const input = sharp(bytes, { limitInputPixels: 64 * 1024 * 1024, failOn: 'warning' });
  const meta = await input.metadata();
  if (!formats[meta.format] || !meta.width || !meta.height) throw Error('Unsupported or invalid raster image.');
  // Force decoding so truncated or forged image headers cannot count as success.
  await input.clone().resize({ width: 256, height: 256, fit: 'inside' }).toBuffer();
  return { mime: formats[meta.format], extension: meta.format === 'jpeg' ? 'jpg' : meta.format,
    width: meta.width, height: meta.height, sha256: digest(bytes), bytes: bytes.length };
}

export async function readBounded(response, max) {
  if (Number(response.headers.get('content-length')) > max) throw Error('Response exceeds byte limit.');
  const chunks = [];
  let size = 0;
  for await (const chunk of response.body) {
    size += chunk.length;
    if (size > max) throw Error('Response exceeds byte limit.');
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

export async function downloadPublicImage(address, redirects = 0) {
  if (redirects > 3) throw Error('Too many image redirects.');
  const url = new URL(address);
  if (url.protocol !== 'https:' || url.username || url.password) throw Error('Unsafe image download URL.');
  const records = await dns.lookup(url.hostname.replace(/^\[|\]$/g, ''), { all: true });
  if (!records.length || records.some(({ address }) => ipaddr.process(address).range() !== 'unicast')) {
    throw Error('Non-public image download destination.');
  }
  // Pin the validated address; use a fresh client without the provider auth header.
  const record = records.find(record => record.family === 4) || records[0];
  const response = await new Promise((resolve, reject) => {
    const request = https.get(url, {
      agent: false, signal: AbortSignal.timeout(60_000),
      lookup: (_host, options, callback) => options.all
        ? callback(null, [record]) : callback(null, record.address, record.family),
    }, resolve);
    request.on('error', reject);
  });
  if ([301, 302, 303, 307, 308].includes(response.statusCode)) {
    response.resume();
    return downloadPublicImage(new URL(response.headers.location, url).href, redirects + 1);
  }
  if (response.statusCode !== 200) { response.resume(); throw Error(`Image download HTTP ${response.statusCode}.`); }
  const chunks = [];
  let size = 0;
  for await (const chunk of response) {
    size += chunk.length;
    if (size > MAX_IMAGE_BYTES) { response.destroy(); throw Error('Image download exceeds byte limit.'); }
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

export async function requestImage(provider, request, references) {
  const endpoint = `${provider.base}/images/${references.length ? 'edits' : 'generations'}`;
  const headers = { Authorization: `Bearer ${provider.key}` };
  let body;
  if (references.length) {
    body = new FormData();
    for (const [name, value] of Object.entries(request)) body.set(name, String(value));
    for (const reference of references) {
      body.append('image[]', new Blob([reference.bytes], { type: reference.mime }), path.basename(reference.path));
    }
  } else {
    headers['Content-Type'] = 'application/json';
    body = JSON.stringify(request);
  }
  const response = await fetch(endpoint, {
    method: 'POST', headers, body, redirect: 'error', signal: AbortSignal.timeout(240_000),
  });
  const requestId = response.headers.get('x-request-id') || response.headers.get('x-codex-imagegen-request-id');
  const data = JSON.parse((await readBounded(response, MAX_RESPONSE_BYTES)).toString('utf8'));
  if (!response.ok) {
    const message = String(data.error?.message || 'Provider rejected the request.')
      .replaceAll(provider.key, '[REDACTED]').replace(/sk-[\w-]+/g, '[REDACTED]').slice(0, 400);
    throw Error(`HTTP ${response.status}: ${message}${requestId ? ` (request ${requestId})` : ''}`);
  }
  if (!Array.isArray(data.data) || data.data.length === 0) throw Error('Provider returned no images.');
  const images = [];
  for (const item of data.data) {
    if (typeof item.b64_json === 'string') {
      if (!/^[A-Za-z0-9+/]*={0,2}$/.test(item.b64_json)) throw Error('Invalid image Base64.');
      images.push(Buffer.from(item.b64_json, 'base64'));
    } else if (typeof item.url === 'string') images.push(await downloadPublicImage(item.url));
    else throw Error('Provider image has no usable content.');
  }
  return { images, requestId, endpoint };
}

export class ImageService {
  constructor({ root, readProvider, request = requestImage }) {
    this.root = root;
    this.readProvider = readProvider;
    this.request = request;
    this.jobs = new Map();
    this.auditPath = path.join(root, 'mcp-audit.jsonl');
  }

  async audit(event) {
    await fs.appendFile(this.auditPath, JSON.stringify({ at: new Date().toISOString(), ...event }) + '\n');
  }

  async start(args, waitSeconds = 20) {
    const count = args.count ?? 1;
    if (!Number.isSafeInteger(count) || count < 1) throw Error('Count must be a positive safe integer.');
    if (!args.prompt?.trim() && !args.prompts?.length) throw Error('An image prompt is required.');
    if (args.prompts && args.prompts.length !== count) throw Error('prompts length must match the requested total.');
    const workspace = await fs.realpath(args.workspace_directory);
    if (!inside(this.root, workspace) || inside(path.join(this.root, 'codex-home'), workspace)) {
      throw Error('The prototype only writes to an isolated task workspace.');
    }
    const provider = await this.readProvider();
    const references = [];
    for (const file of args.reference_paths ?? []) {
      const real = await fs.realpath(file);
      if (!inside(this.root, real) || inside(path.join(this.root, 'codex-home'), real)) {
        throw Error('Reference must be an original image inside the isolated test workspace.');
      }
      const stat = await fs.stat(real);
      if (!stat.isFile() || stat.size > MAX_IMAGE_BYTES) throw Error('Invalid reference file.');
      const bytes = await fs.readFile(real);
      references.push({ path: real, ...await imageInfo(bytes), bytes });
    }
    const id = randomUUID();
    const directory = path.join(workspace, 'output', 'images', id);
    await fs.mkdir(directory, { recursive: true });
    if (!inside(workspace, await fs.realpath(directory))) throw Error('Output directory escapes the workspace.');
    const job = {
      id, directory, total: count, completed: 0, failed: 0, cancelled: 0,
      model: args.model || 'gpt-image-2', size: args.size || '1024x1024',
      mode: references.length ? 'edit' : 'generate', next: 1, stopped: false,
      items: [], reference_paths: references.map(r => r.path), persistence: Promise.resolve(),
      finalizing: false, finalized: false,
    };
    this.jobs.set(id, job);
    await this.audit({ event: 'start', jobId: id, model: job.model, total: count, mode: job.mode,
      endpoint: `${provider.base}/images/${references.length ? 'edits' : 'generations'}`,
      references: references.map(r => ({ path: r.path, sha256: r.sha256, bytes: r.bytes.length, mime: r.mime })) });
    await this.persist(job);
    const worker = async () => {
      while (!job.stopped && job.next <= job.total) {
        const index = job.next++;
        const item = { index, status: 'running', prompt: args.prompts?.[index - 1] || args.prompt };
        job.items.push(item);
        const started = Date.now();
        try {
          const current = await this.readProvider();
          if (provider.identity !== current.identity) throw Error('Provider changed; request was not sent.');
          await this.audit({ event: 'request', jobId: id, index, model: job.model });
          if (job.stopped) {
            item.status = 'cancelled';
            job.cancelled++;
            await this.audit({ event: 'cancel-before-submit', jobId: id, index });
            break;
          }
          item.dispatched = true;
          const result = await this.request(current, {
            model: job.model, prompt: item.prompt, n: 1, size: job.size,
          }, references);
          item.request_id = result.requestId;
          item.outputs = [];
          for (const [offset, bytes] of result.images.entries()) {
            const info = await imageInfo(bytes);
            const output = path.join(directory, `${String(index).padStart(3, '0')}-${offset + 1}.${info.extension}`);
            await fs.writeFile(output, bytes, { flag: 'wx' });
            if (digest(await fs.readFile(output)) !== info.sha256) throw Error('Saved image readback did not match.');
            item.outputs.push({ path: output, ...info });
          }
          if (result.images.length !== 1) item.warning = `Requested 1, received ${result.images.length}; no automatic retry.`;
          item.status = 'succeeded';
          job.completed++;
        } catch (error) {
          item.status = 'failed';
          item.error = String(error.message).replaceAll(provider.key, '[REDACTED]');
          job.failed++;
        }
        item.elapsed_ms = Date.now() - started;
        await this.audit({ event: 'result', jobId: id, ...item });
        await this.persist(job);
      }
    };
    const guardedWorker = () => worker().catch(error => { job.stopped = true; throw error; });
    job.done = Promise.allSettled([guardedWorker(), guardedWorker()]).then(async results => {
      job.finalizing = true;
      const rejected = results.find(result => result.status === 'rejected');
      if (rejected) throw rejected.reason;
      await this.persist(job, true);
      job.finalized = true;
    }).catch(error => {
      job.stopped = true;
      job.cancelled = Math.max(0, job.total - job.next + 1) + job.items.filter(item => item.status === 'cancelled').length;
      job.persistence_error = String(error.message).replaceAll(provider.key, '[REDACTED]');
      job.finalized = true;
    });
    return this.wait(id, waitSeconds);
  }

  snapshot(job, finalized = job.finalized) {
    const { id, directory, total, completed, failed, cancelled, model, size, mode, items, reference_paths } = job;
    const savedImages = items.reduce((total, item) => total + (item.outputs?.length || 0), 0);
    const mismatched = items.some(item => item.warning);
    return { id, directory, total, completed, failed, cancelled, model, size, mode, reference_paths,
      saved_images: savedImages,
      status: !finalized ? 'running' : job.persistence_error ? completed ? 'partial' : 'failed'
        : cancelled ? 'cancelled' : failed || mismatched ? completed ? 'partial' : 'failed'
          : completed === total ? 'completed' : 'failed',
      count_mismatch: mismatched,
      pending: total - completed - failed - cancelled,
      items: structuredClone(items), persistence_error: job.persistence_error,
      notice: 'Paid requests are never retried automatically. Keep polling while running. Preserve successful originals and report failed slot indices. Do not replace failed images with local drawing/recoloring, a different model or a different tool. Retry or cancel only on explicit user request.' };
  }

  persist(job, finalized) {
    const snapshot = JSON.stringify(this.snapshot(job, finalized), null, 2);
    job.persistence = job.persistence.then(async () => {
      const temp = path.join(job.directory, 'manifest.tmp');
      await fs.writeFile(temp, snapshot);
      await fs.rename(temp, path.join(job.directory, 'manifest.json'));
    });
    return job.persistence;
  }

  async wait(id, seconds = 20) {
    const job = this.jobs.get(id);
    if (!job) throw Error('Unknown image job.');
    const deadline = Date.now() + Math.min(30, Math.max(0, seconds)) * 1000;
    while (this.snapshot(job).status === 'running' && Date.now() < deadline) await sleep(200);
    return this.snapshot(job);
  }

  async cancel(id) {
    const job = this.jobs.get(id);
    if (!job) throw Error('Unknown image job.');
    // A late cancellation must not queue a running snapshot after the final one.
    if (job.finalizing || job.finalized) {
      await job.done;
      return this.snapshot(job);
    }
    if (job.stopped) return this.snapshot(job);
    job.stopped = true;
    job.cancelled = Math.max(0, job.total - job.next + 1);
    await this.persist(job);
    return this.snapshot(job);
  }
}

export async function toolResult(snapshot) {
  const content = [{ type: 'text', text: JSON.stringify(snapshot) }];
  // Tool images are previews. Original file paths remain available for exact edits.
  for (const item of snapshot.items) {
    for (const output of item.outputs || []) {
      const preview = await sharp(output.path).resize({ width: 640, height: 640, fit: 'inside', withoutEnlargement: true }).jpeg({ quality: 82 }).toBuffer();
      content.push({ type: 'image', mimeType: 'image/jpeg', data: preview.toString('base64') });
    }
  }
  return { content, structuredContent: snapshot, isError: snapshot.status === 'failed' };
}
