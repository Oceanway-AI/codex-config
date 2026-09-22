// Optional real Codex app-server verification. Dedicated home/workspace only;
// stdin/stdout forward JSON-RPC and never connect to the desktop's server.
import fs from 'node:fs/promises';
import path from 'node:path';
import { spawn } from 'node:child_process';
import readline from 'node:readline';
import { isolatedEnvironment } from './isolated-live-harness.mjs';

const [root, executable] = process.argv.slice(2);
if (!root || !executable || !path.isAbsolute(root) || !path.isAbsolute(executable)) {
  throw new Error('Pass the absolute private test root and Codex executable.');
}
const env = isolatedEnvironment(root);
const auth = JSON.parse(await fs.readFile(path.join(env.CODEX_HOME, 'auth.json'), 'utf8'));
const key = auth.OPENAI_API_KEY;
if (!key) throw new Error('Private test auth is missing.');
const redact = text => text.replaceAll(key, '[REDACTED]');
const logs = await fs.open(path.join(root, 'agent-events.jsonl'), 'a');
const errors = await fs.open(path.join(root, 'agent-stderr.log'), 'a');
const agent = spawn(executable, [
  'app-server', '--stdio',
  '-c', 'project_root_markers=[]',
  '-c', 'web_search="disabled"',
  '-c', 'model_reasoning_effort="medium"',
  '-c', 'check_for_update_on_startup=false',
  '-c', 'model_providers.OceanWay.request_max_retries=0',
  '-c', 'model_providers.OceanWay.stream_max_retries=0',
  '-c', `shell_environment_policy.set.CODEX_HOME=${JSON.stringify(env.CODEX_HOME)}`,
], {
  cwd: path.join(root, 'workspace'), env, windowsHide: true,
  stdio: ['pipe', 'pipe', 'pipe'],
});
const output = readline.createInterface({ input: agent.stdout, crlfDelay: Infinity });
output.on('line', line => {
  const clean = redact(line);
  logs.appendFile(clean + '\n');
  console.log(clean);
});
agent.stderr.on('data', chunk => errors.appendFile(redact(chunk.toString())));
agent.on('error', error => console.log(JSON.stringify({ harnessError: redact(error.message) })));
agent.on('exit', async (code, signal) => {
  console.log(JSON.stringify({ agentExit: code, signal }));
  await logs.close();
  await errors.close();
  process.exit(code || 0);
});
const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
input.on('line', line => {
  const message = JSON.parse(line);
  if (message.harnessStop) agent.stdin.end();
  else agent.stdin.write(JSON.stringify(message) + '\n');
});
input.on('close', () => agent.stdin.end());
console.log(JSON.stringify({ agentPid: agent.pid, privateHome: env.CODEX_HOME }));
