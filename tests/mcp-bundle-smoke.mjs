// Non-billing acceptance of the packaged executable, not a production MCP client.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, readFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join, dirname, basename } from 'node:path';
import { createInterface } from 'node:readline';
import { once } from 'node:events';

const executable = resolve(process.argv[2] || '');
assert.ok(process.argv[2], 'Pass the built executable path.');
const home = await mkdtemp(join(tmpdir(), 'oceanway-mcp-smoke-'));
const child = spawn(executable, ['--image-mcp-stdio'], {
  windowsHide: true,
  env: { ...process.env, CODEX_HOME: home },
  stdio: ['pipe', 'pipe', 'pipe'],
});
const exit = once(child, 'exit');
const pending = new Map();
let serial = 0, stderr = '';
child.stderr.on('data', data => { stderr += data.toString().slice(0, 2000); });
child.on('error', error => {
  for (const waiter of pending.values()) waiter.reject(error);
});
const lines = createInterface({ input: child.stdout });
lines.on('line', line => {
  const message = JSON.parse(line);
  const waiter = pending.get(message.id);
  if (!waiter) return;
  pending.delete(message.id);
  if (message.error) waiter.reject(new Error(JSON.stringify(message.error)));
  else waiter.resolve(message.result);
});
function request(method, params) {
  const id = ++serial;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
  });
}
const deadline = setTimeout(() => {
  for (const waiter of pending.values()) waiter.reject(new Error('MCP bundle handshake timed out.'));
  child.kill();
}, 30000);
try {
  const initialized = await request('initialize', {
    protocolVersion: '2025-06-18',
    capabilities: {},
    clientInfo: { name: 'oceanway-bundle-smoke', version: '1' },
  });
  assert.ok(initialized.capabilities?.tools);
  child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`);
  const listed = await request('tools/list', {});
  assert.deepEqual(listed.tools.map(tool => tool.name).sort(),
    ['generate_images', 'get_image_job', 'cancel_image_job', 'retry_image_job'].sort());
  const generate = listed.tools.find(tool => tool.name === 'generate_images');
  assert.equal(generate.inputSchema.properties.model.default, 'gpt-image-2');
  assert.equal(generate.inputSchema.properties.count.default, 1);
  assert.equal(generate.inputSchema.properties.count.maximum, undefined);
  child.stdin.end();
  const [code] = await exit;
  assert.equal(code, 0, `MCP did not exit cleanly: ${stderr}`);
  const files = await readdir(home);
  assert.ok(!files.includes('auth.json') && !files.includes('config.toml'));
  for (const name of files.filter(name => name.endsWith('.json'))) {
    assert.ok(!(await readFile(join(home, name), 'utf8')).includes('api_key'));
  }
  console.log('Packaged MCP initialize/list/EOF passed; no credentials or paid calls.');
} finally {
  clearTimeout(deadline);
  child.kill();
  lines.close();
  // This directory was exclusively created by mkdtemp in this process.
  assert.equal(dirname(resolve(home)), resolve(tmpdir()));
  assert.ok(basename(home).startsWith('oceanway-mcp-smoke-'));
  await rm(home, { recursive: true, force: true });
}
