import fs from 'node:fs/promises';
import path from 'node:path';
import { parse, stringify } from 'smol-toml';
import { isolatedPaths, readHomeFile } from './engine.mjs';

export function stripManagedRules(text) {
  const begin = '<!-- OCEANWAY:DIRECT-IMAGE-API:BEGIN -->';
  const end = '<!-- OCEANWAY:DIRECT-IMAGE-API:END -->';
  const a = text.indexOf(begin), b = text.indexOf(end);
  if (a === -1 && b === -1) return text;
  if (a < 0 || b < a || text.indexOf(begin, a + begin.length) !== -1 || text.indexOf(end, b + end.length) !== -1) {
    throw Error('Malformed managed rules; no configuration was changed.');
  }
  return text.slice(0, a) + text.slice(b + end.length).replace(/^\r?\n\r?\n/, '');
}

export async function prepareIsolatedMcp({ root, nodeExecutable, serverPath }) {
  const paths = await isolatedPaths({ OCEANWAY_MCP_TEST_ROOT: root, CODEX_HOME: path.join(root, 'codex-home') });
  const configPath = path.join(paths.home, 'config.toml');
  const original = await readHomeFile(paths.home, 'config.toml');
  const config = parse(original);
  if (config.model_provider !== 'OceanWay') throw Error('The isolated provider is not configured.');
  if (typeof config.developer_instructions === 'string') {
    config.developer_instructions = stripManagedRules(config.developer_instructions);
    if (!config.developer_instructions.trim()) delete config.developer_instructions;
  }
  const changes = [];
  for (const name of ['AGENTS.md', 'AGENTS.override.md']) {
    const file = path.join(paths.home, name);
    const text = await readHomeFile(paths.home, name).catch(error => {
      if (error.code === 'ENOENT') return null;
      throw error;
    });
    if (text !== null) changes.push({ file, before: text, after: stripManagedRules(text) });
  }
  config.mcp_servers ??= {};
  config.mcp_servers.oceanway_images = {
    command: nodeExecutable, args: [serverPath],
    env: { CODEX_HOME: paths.home, OCEANWAY_MCP_TEST_ROOT: paths.root },
    startup_timeout_sec: 30, tool_timeout_sec: 90, enabled: true,
  };
  const rendered = stringify(config);
  parse(rendered);
  const backup = path.join(paths.root, `before-mcp-${Date.now()}`);
  await fs.mkdir(backup);
  for (const { file } of changes) await fs.copyFile(file, path.join(backup, path.basename(file)), fs.constants.COPYFILE_EXCL);
  await fs.copyFile(configPath, path.join(backup, 'config.toml'), fs.constants.COPYFILE_EXCL);
  try {
    for (const { file, after } of changes) await fs.writeFile(file, after);
    await fs.writeFile(configPath, rendered);
  } catch (error) {
    for (const { file, before } of changes) await fs.writeFile(file, before);
    await fs.writeFile(configPath, original);
    throw error;
  }
  return { codexHome: paths.home, backup, mcpRegistered: true,
    agentsHaveManagedImageRules: changes.some(change => change.after.includes('OCEANWAY:DIRECT-IMAGE-API')),
    developerHasManagedImageRules: Boolean(config.developer_instructions?.includes('OCEANWAY:DIRECT-IMAGE-API')) };
}

export const shortRoutingRules = [
  '<!-- OCEANWAY:IMAGE-MCP-ROUTING:BEGIN -->',
  'For image generation or edits with the active OceanWay provider, use oceanway_images MCP tools; do not substitute imagegen, image CLIs, or local drawing/recoloring.',
  'Honor the user-requested model, total count and original reference files. Analysis or prompt-only requests must not generate images.',
  'On unavailable tools/models or partial failure, retain successes and report missing outputs; do not change route/model or resend paid requests without explicit user approval.',
  '<!-- OCEANWAY:IMAGE-MCP-ROUTING:END -->',
].join('\n');

export async function addIsolatedRouting(root) {
  const { home } = await isolatedPaths({ OCEANWAY_MCP_TEST_ROOT: root, CODEX_HOME: path.join(root, 'codex-home') });
  const override = await readHomeFile(home, 'AGENTS.override.md').catch(error => {
    if (error.code === 'ENOENT') return '';
    throw error;
  });
  const name = override.trim() ? 'AGENTS.override.md' : 'AGENTS.md';
  const previous = await readHomeFile(home, name).catch(error => {
    if (error.code === 'ENOENT') return '';
    throw error;
  });
  if (previous.includes('OCEANWAY:IMAGE-MCP-ROUTING')) throw Error('Routing already exists; do not append duplicates.');
  const backup = path.join(path.dirname(home), `before-short-routing-${Date.now()}.txt`);
  await fs.writeFile(backup, previous, { flag: 'wx' });
  await fs.writeFile(path.join(home, name), shortRoutingRules + '\n\n' + previous);
  return { path: path.join(home, name), backup, ruleLines: 3, containsKey: false };
}
