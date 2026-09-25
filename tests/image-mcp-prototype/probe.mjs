import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';

const root = path.resolve(process.argv[2]);
const transport = new StdioClientTransport({
  command: process.execPath,
  args: [fileURLToPath(new URL('./server.mjs', import.meta.url))],
  env: { ...process.env, CODEX_HOME: path.join(root, 'codex-home'), OCEANWAY_MCP_TEST_ROOT: root },
  stderr: 'pipe',
});
const client = new Client({ name: 'oceanway-acceptance-probe', version: '0.0.0' });
try {
  await client.connect(transport);
  const tools = await client.listTools();
  console.log(JSON.stringify({
    ready: true,
    tools: tools.tools.map(tool => ({ name: tool.name, required: tool.inputSchema.required })),
    paidRequests: 0,
  }, null, 2));
} finally {
  await client.close();
}
