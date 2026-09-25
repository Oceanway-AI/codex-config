// Explicitly invoked test harness, never a production image-generation helper.
import fs from 'node:fs/promises';
import path from 'node:path';
import net from 'node:net';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';
import { createRequire } from 'node:module';

export const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

export async function fingerprint(files) {
  return Object.fromEntries(await Promise.all(files.map(async file => {
    const bytes = await fs.readFile(file).catch(error => {
      if (error.code === 'ENOENT') return null;
      throw error;
    });
    return [file, bytes && createHash('sha256').update(bytes).digest('hex')];
  })));
}

export function privateEnvironment(root, parentEnvironment = globalThis.process?.env ?? {}) {
  if (!path.isAbsolute(root)) throw new Error('An absolute private root is required.');
  const env = Object.fromEntries(Object.entries(parentEnvironment).filter(([name]) =>
    !/^(CODEX|OPENAI|CHATGPT|ELECTRON|WEBVIEW2)/i.test(name)));
  return {
    ...env,
    CODEX_HOME: path.join(root, 'codex-home'),
    OCEANWAY_RESTART_TARGET: path.join(root, 'codex-home', 'oceanway-test-desktop.json'),
    CODEX_ELECTRON_USER_DATA_PATH: path.join(root, 'desktop-profile'),
    HOME: path.join(root, 'profile'),
    USERPROFILE: path.join(root, 'profile'),
    APPDATA: path.join(root, 'profile', 'AppData', 'Roaming'),
    LOCALAPPDATA: path.join(root, 'profile', 'AppData', 'Local'),
    TEMP: path.join(root, 'temp'),
    TMP: path.join(root, 'temp'),
  };
}

export async function freePort() {
  const server = net.createServer();
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const port = server.address().port;
  await new Promise(resolve => server.close(resolve));
  return port;
}

export async function launchDesktop({ root, executable, engine, playwrightModule, parentEnvironment }) {
  const { chromium } = createRequire(pathToFileURL(playwrightModule))('./index.js');
  const env = privateEnvironment(root, parentEnvironment);
  for (const name of ['CODEX_HOME', 'CODEX_ELECTRON_USER_DATA_PATH',
    'HOME', 'APPDATA', 'LOCALAPPDATA', 'TEMP']) {
    await fs.mkdir(env[name], { recursive: true });
  }
  const workspace = path.join(root, 'workspace');
  await fs.mkdir(workspace, { recursive: true });
  const port = await freePort();
  const child = spawn(executable, [
    `--user-data-dir=${env.CODEX_ELECTRON_USER_DATA_PATH}`,
    '--remote-debugging-address=127.0.0.1', `--remote-debugging-port=${port}`,
  ], { cwd: workspace, env: { ...env, CODEX_CLI_PATH: engine },
    windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'] });
  let launchError;
  child.on('error', error => { launchError = error; });
  // Drain without exposing app logs, which can contain request data.
  child.stderr.on('data', () => {});
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline && child.exitCode === null && !launchError) {
    try {
      if ((await fetch(`http://127.0.0.1:${port}/json/version`)).ok) break;
    } catch { /* The private desktop is starting. */ }
    await sleep(300);
  }
  if (child.exitCode !== null || launchError) {
    throw new Error('The isolated desktop did not start.');
  }
  let browser;
  try {
    browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  } catch (error) {
    child.kill();
    throw error;
  }
  const page = browser.contexts()[0].pages()[0];
  return { root, workspace, env, child, browser, page, port };
}

export async function bindRestartFixture({ root, pid, engine, port, protectedPids }) {
  if (!protectedPids?.length || protectedPids.includes(pid)) {
    throw new Error('Explicit protected desktop identities are required.');
  }
  const env = privateEnvironment(root);
  // The live process supplies the executable and exact creation timestamp.
  const script = `$p=Get-Process -Id ${Number(pid)}; $c=Get-CimInstance Win32_Process -Filter 'ProcessId=${Number(pid)}'; ` +
    `[pscustomobject]@{pid=$p.Id;started=$p.StartTime.ToUniversalTime().Ticks.ToString();path=$p.Path;commandLine=$c.CommandLine} | ConvertTo-Json -Compress`;
  const child = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script],
    { windowsHide: true, stdio: ['ignore', 'pipe', 'ignore'] });
  let output = '';
  child.stdout.on('data', chunk => { output += chunk; });
  const [code] = await once(child, 'exit');
  if (code !== 0) throw new Error('Cannot inspect isolated process.');
  const identity = JSON.parse(output);
  if (!identity.commandLine.includes(`--user-data-dir=${env.CODEX_ELECTRON_USER_DATA_PATH}`)
      && !identity.commandLine.includes(`--user-data-dir="${env.CODEX_ELECTRON_USER_DATA_PATH}"`)) {
    throw new Error('The selected process does not own the private profile.');
  }
  delete identity.commandLine;
  const manifest = { ...identity, isolated: true, codexHome: env.CODEX_HOME,
    userData: env.CODEX_ELECTRON_USER_DATA_PATH, engine, port };
  await fs.writeFile(path.join(env.CODEX_HOME, 'oceanway-test-desktop.json'),
    JSON.stringify(manifest), { flag: 'wx' });
  await fs.writeFile(path.join(root, '.oceanway-isolation.json'), JSON.stringify({
    purpose: 'oceanway-codex-isolated-acceptance', version: 1, protectedPids,
  }), { flag: 'wx' });
  return manifest;
}

export async function launchConfig({ root, executable, playwrightModule, parentEnvironment }) {
  const env = privateEnvironment(root, parentEnvironment);
  const { chromium } = createRequire(pathToFileURL(playwrightModule))('./index.js');
  const port = await freePort();
  const child = spawn(executable, ['--gui'], {
    cwd: path.join(root, 'workspace'),
    env: { ...env, WEBVIEW2_USER_DATA_FOLDER: path.join(root, 'config-webview'),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:
        `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1` },
    windowsHide: true, stdio: 'ignore',
  });
  const deadline = Date.now() + 45_000;
  let ready = false;
  while (Date.now() < deadline && child.exitCode === null) {
    try {
      if ((await fetch(`http://127.0.0.1:${port}/json/version`)).ok) { ready = true; break; }
    } catch { /* WebView2 is starting. */ }
    await sleep(300);
  }
  if (!ready) { child.kill(); throw new Error('Isolated configuration UI did not start.'); }
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const page = browser.contexts()[0].pages()[0];
  await page.waitForFunction(() => typeof window.__TAURI_INTERNALS__?.invoke === 'function');
  const invoke = (command, args = {}) => page.evaluate(
    ({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
  const status = await invoke('get_config_status');
  if (path.resolve(status.configPath) !== path.join(env.CODEX_HOME, 'config.toml')) {
    await invoke('exit_app').catch(() => {});
    throw new Error('Unexpected configuration home.');
  }
  return { root, env, child, browser, page, port, invoke };
}
