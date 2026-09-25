// Explicit local acceptance helper. Never bundled or started by the application.
import http from 'node:http';
import { once } from 'node:events';
import { createHash } from 'node:crypto';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';

export const FIXTURE_KEY = 'oceanway-local-fixture';
const BODY_LIMIT = 64 * 1024 * 1024;

async function bodyBytes(request) {
  const chunks = [];
  let length = 0;
  for await (const chunk of request) {
    length += chunk.length;
    if (length > BODY_LIMIT) throw new Error('Fixture request body exceeds limit.');
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

function json(response, status, value, requestId) {
  if (response.destroyed) return;
  response.writeHead(status, {
    'content-type': 'application/json',
    ...(requestId ? { 'x-request-id': requestId } : {}),
  });
  response.end(JSON.stringify(value));
}

export async function startImageDesktopFixture({
  imageBytes, textBaseUrl, textKey, textModels = [], allowTextProxy = false,
}) {
  if (!Buffer.isBuffer(imageBytes) || !imageBytes.length) {
    throw new Error('Supply a known fixture PNG; fixture outputs are not real generation.');
  }
  const upstream = allowTextProxy ? new URL(textBaseUrl) : null;
  if (upstream && (upstream.protocol !== 'https:' || upstream.username || upstream.password
      || upstream.search || upstream.hash || !textKey || !textModels.length
      || textModels.some(model => typeof model !== 'string' || !model || /image|banana/i.test(model)))) {
    throw new Error('The text-only upstream requires HTTPS, a separate key and explicit text models.');
  }
  const audit = [];
  const waiting = new Set();
  const textControllers = new Set();
  let mode = 'success', serial = 0, active = 0, peak = 0;
  const server = http.createServer(async (request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    if (request.headers.authorization !== `Bearer ${FIXTURE_KEY}`) {
      json(response, 401, { error: { message: 'Local fixture credential required.' } });
      return;
    }
    try {
      const bytes = await bodyBytes(request);
      if (/^\/(?:v1\/)?images\/(?:generations|edits)$/.test(url.pathname)) {
        if (request.method !== 'POST') {
          json(response, 405, { error: { message: 'Use POST for fixture images.' } });
          return;
        }
        const editing = url.pathname.endsWith('/edits');
        let values, references = [];
        if (editing) {
          const form = await new Request('http://127.0.0.1/fixture', {
            method: 'POST', body: bytes,
            headers: { 'content-type': request.headers['content-type'] || '' },
          }).formData();
          values = Object.fromEntries([...form].filter(([, value]) => typeof value === 'string'));
          for (const [field, file] of form) {
            if (typeof file === 'string') continue;
            const original = Buffer.from(await file.arrayBuffer());
            references.push({ field, name: file.name, type: file.type, bytes: original.length,
              sha256: createHash('sha256').update(original).digest('hex') });
          }
        } else {
          values = JSON.parse(bytes.toString('utf8'));
        }
        const id = `ow-local-fixture-${++serial}`;
        const entry = { id, path: url.pathname, model: values.model, n: Number(values.n),
          prompt: values.prompt, size: values.size, references, mode, outcome: 'pending' };
        audit.push(entry);
        active += 1;
        peak = Math.max(peak, active);
        try {
          if (mode === 'stall') {
            await new Promise(resolve => {
              const release = () => { waiting.delete(release); resolve(); };
              waiting.add(release);
              response.once('close', release);
            });
          }
          const fail = mode === 'fail' || (mode === 'partial' && /\bOW_FAIL\b/i.test(values.prompt));
          entry.outcome = fail ? 'http-502' : 'mock-image';
          if (fail) {
            json(response, 502, { error: { message: 'Deliberate local image fixture failure.' } }, id);
          } else {
            const image = { b64_json: imageBytes.toString('base64') };
            json(response, 200, { data: mode === 'extra-output' ? [image, image] : [image] }, id);
          }
        } finally {
          active -= 1;
        }
        return;
      }
      const route = /^\/(?:v1\/)?(responses|models)$/.exec(url.pathname)?.[1];
      if (!route || !allowTextProxy) {
        json(response, 404, { error: { message: 'No matching local fixture route.' } });
        return;
      }
      if (!((route === 'responses' && request.method === 'POST')
          || (route === 'models' && request.method === 'GET'))) {
        json(response, 405, { error: { message: 'Unsupported text proxy method.' } });
        return;
      }
      if (request.method === 'POST') {
        let payload;
        try { payload = JSON.parse(bytes.toString('utf8')); } catch {
          json(response, 400, { error: { message: 'Text proxy requires uncompressed JSON.' } });
          return;
        }
        const localTool = tool => tool && (
          ['function', 'custom'].includes(tool.type)
          || (tool.type === 'namespace' && Array.isArray(tool.tools) && tool.tools.every(localTool)));
        if (!textModels.includes(payload.model)
            || (payload.tools !== undefined && (!Array.isArray(payload.tools) || !payload.tools.every(localTool)))
            || (payload.modalities !== undefined && (!Array.isArray(payload.modalities)
              || payload.modalities.some(value => value !== 'text')))
            || (typeof payload.tool_choice === 'object' && payload.tool_choice !== null
              && !localTool(payload.tool_choice))) {
          json(response, 400, { error: { message: 'Only explicit text models and local function tools may be proxied.' } });
          return;
        }
      }
      // Never proxy hosted tools, /images or caller-supplied destinations. Keep the real key
      // private to this explicitly enabled test helper, not in Codex/MCP inputs.
      const destination = new URL(`${upstream.href.replace(/\/$/, '')}/${route}`);
      const controller = new AbortController();
      textControllers.add(controller);
      response.once('close', () => controller.abort());
      try {
        const remote = await fetch(destination, {
          method: request.method, redirect: 'error', signal: controller.signal,
          headers: { authorization: `Bearer ${textKey}`,
            'content-type': request.headers['content-type'] || 'application/json' },
          ...(request.method === 'POST' ? { body: bytes } : {}),
        });
        const headers = {};
        for (const name of ['content-type', 'x-request-id']) {
          if (remote.headers.has(name)) headers[name] = remote.headers.get(name);
        }
        response.writeHead(remote.status, headers);
        if (remote.body) await pipeline(Readable.fromWeb(remote.body), response);
        else response.end();
      } finally {
        textControllers.delete(controller);
      }
    } catch {
      if (!response.headersSent) json(response, 500, { error: { message: 'Local acceptance fixture failed.' } });
      else response.destroy();
    }
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  return {
    baseUrl: `http://127.0.0.1:${server.address().port}/v1`,
    audit,
    stats: () => ({ active, peak, imageSubmissions: audit.length }),
    setMode(value) {
      if (!['success', 'partial', 'fail', 'stall', 'extra-output'].includes(value)) {
        throw new Error('Unknown fixture mode.');
      }
      mode = value;
    },
    release: () => { for (const release of [...waiting]) release(); },
    async close() {
      for (const release of [...waiting]) release();
      for (const controller of textControllers) controller.abort();
      await new Promise(resolve => { server.close(resolve); server.closeAllConnections(); });
    },
  };
}
