// Optional local runtime probe: no real provider, credentials or image charges.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import http from 'node:http';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { crc32 } from 'node:zlib';

const executable = process.argv[2];
if (!executable) throw new Error('Pass the absolute path to a Codex executable.');
const home = await fs.mkdtemp(path.join(os.tmpdir(), 'oceanway-codex-runtime-'));
const workspace = path.join(home, 'workspace');
await fs.mkdir(workspace);
const rules = await fs.readFile(new URL('../src-tauri/src/direct-image-instructions.md', import.meta.url), 'utf8');
const requests = [];
const server = http.createServer(async (req, res) => {
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  if (req.method !== 'POST') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ data: [] }));
    return;
  }
  const bytes = Buffer.concat(chunks);
  // Codex can compress request bodies even for a local provider.
  const encoding = req.headers['content-encoding'];
  const zlib = await import('node:zlib');
  const decoded = encoding === 'zstd' ? zlib.zstdDecompressSync(bytes)
    : encoding === 'gzip' ? zlib.gunzipSync(bytes) : bytes;
  requests.push(JSON.parse(decoded.toString()));
  res.writeHead(200, { 'Content-Type': 'text/event-stream' });
  const item = { type: 'message', id: 'msg_probe', role: 'assistant', status: 'completed',
    content: [{ type: 'output_text', text: 'Local transport probe complete.', annotations: [] }] };
  for (const event of [
    { type: 'response.created', response: { id: 'resp_probe', status: 'in_progress', output: [] } },
    { type: 'response.output_item.added', output_index: 0, item },
    { type: 'response.output_item.done', output_index: 0, item },
    { type: 'response.completed', response: { id: 'resp_probe', status: 'completed', output: [item],
      usage: { input_tokens: 1, output_tokens: 1, total_tokens: 2 } } },
  ]) res.write(`event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`);
  res.end();
});
server.listen(0, '127.0.0.1');
await once(server, 'listening');
const port = server.address().port;
const config = [
  'model = "gpt-5.4"',
  'model_provider = "OceanWay"',
  'web_search = "disabled"',
  'check_for_update_on_startup = false',
  `developer_instructions = ${JSON.stringify(rules)}`,
  '[model_providers.OceanWay]',
  'name = "Local test only"',
  `base_url = "http://127.0.0.1:${port}/v1"`,
  'wire_api = "responses"',
  'requires_openai_auth = false',
  'experimental_bearer_token = "fake-local-probe"',
].join('\n');
await fs.writeFile(path.join(home, 'config.toml'), config);
const imagePath = path.join(workspace, 'reference.png');
const imageBytes = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a8LsAAAAASUVORK5CYII=', 'base64');
for (let offset = 8; offset < imageBytes.length;) {
  const length = imageBytes.readUInt32BE(offset);
  imageBytes.writeUInt32BE(crc32(imageBytes.subarray(offset + 4, offset + 8 + length)), offset + 8 + length);
  offset += 12 + length;
}
await fs.writeFile(imagePath, imageBytes);
const childEnv = Object.fromEntries(Object.entries(process.env)
  .filter(([key]) => !/^(OPENAI|CODEX|AZURE_OPENAI)/i.test(key)));
const child = spawn(executable, [
  'exec', '--ephemeral', '--skip-git-repo-check', '--json',
  '-C', workspace, '-i', imagePath, '--', 'Inspect this image only; do not generate anything.',
], { env: { ...childEnv, CODEX_HOME: home }, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
let stdout = '';
let stderr = '';
child.stdout.on('data', chunk => { stdout += chunk; });
child.stderr.on('data', chunk => { stderr += chunk; });
const timeout = setTimeout(() => child.kill(), 60_000);
try {
  const [code] = await once(child, 'exit');
  assert.equal(code, 0, stderr.slice(-3000));
  assert.ok(requests.length > 0, 'No local model request captured.');
  const serialized = JSON.stringify(requests[0]);
  assert.ok(serialized.includes('OCEANWAY:DIRECT-IMAGE-API:BEGIN'), 'User-level developer instructions missing.');
  assert.ok(requests[0].input.some(item => item.role === 'developer'
    && JSON.stringify(item).includes('OCEANWAY:DIRECT-IMAGE-API:BEGIN')), 'Rules not in developer message.');
  assert.ok(serialized.includes('input_image'), `Image was not included in model input: ${JSON.stringify(
    requests[0].input.filter(item => item.role === 'user')
  ).slice(-1500)} ${stderr.slice(-1000)}`);
  const report = {
    scope: 'Local Codex executable transport only, not desktop natural-language acceptance',
    instructionInjection: true, imageInModelInput: true,
    originalImagePathInPrompt: serialized.includes(imagePath.replaceAll('\\', '\\\\')),
    modelRequests: requests.length, paidRequests: 0,
    codexHome: home,
  };
  await fs.writeFile(path.join(home, 'report.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
} finally {
  clearTimeout(timeout);
  server.closeAllConnections();
  server.close();
}
