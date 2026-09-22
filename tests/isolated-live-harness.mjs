// Opt-in live verification helper. Import from a private runner and supply the
// credential in memory; this module never reads the normal Codex credentials.
import fs from 'node:fs/promises';
import path from 'node:path';
import net from 'node:net';
import { createHash } from 'node:crypto';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { pathToFileURL } from 'node:url';

const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

export async function fingerprintFiles(paths) {
  return Object.fromEntries(await Promise.all(paths.map(async filename => {
    try {
      return [filename, digest(await fs.readFile(filename))];
    } catch (error) {
      if (error.code === 'ENOENT') return [filename, null];
      throw error;
    }
  })));
}

async function unusedPort() {
  const server = net.createServer();
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const { port } = server.address();
  await new Promise(resolve => server.close(resolve));
  return port;
}

export function isolatedEnvironment(root) {
  const env = Object.fromEntries(Object.entries(process.env).filter(([name]) =>
    !/^(CODEX|OPENAI|AZURE_OPENAI|CHATGPT|WEBVIEW2|ELECTRON)/i.test(name)));
  return {
    ...env,
    CODEX_HOME: path.join(root, 'codex-home'),
    HOME: path.join(root, 'profile'),
    USERPROFILE: path.join(root, 'profile'),
    APPDATA: path.join(root, 'profile', 'AppData', 'Roaming'),
    LOCALAPPDATA: path.join(root, 'profile', 'AppData', 'Local'),
    TEMP: path.join(root, 'temp'),
    TMP: path.join(root, 'temp'),
  };
}

export async function startNative({ executable, root, playwrightModule, apiKey, baseUrl }) {
  if (!path.isAbsolute(root) || !path.isAbsolute(executable)) {
    throw new Error('Use absolute, dedicated test paths.');
  }
  const env = isolatedEnvironment(root);
  for (const name of ['CODEX_HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'TEMP']) {
    await fs.mkdir(env[name], { recursive: true });
  }
  await fs.mkdir(path.join(root, 'workspace'), { recursive: true });
  const privateHome = await fs.realpath(env.CODEX_HOME);
  if (privateHome.toLowerCase() === path.join(process.env.USERPROFILE, '.codex').toLowerCase()) {
    throw new Error('Refusing the normal Codex home.');
  }
  const port = await unusedPort();
  const child = spawn(executable, ['--gui'], {
    env: {
      ...env,
      WEBVIEW2_USER_DATA_FOLDER: path.join(root, 'webview'),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`,
    },
    cwd: path.join(root, 'workspace'),
    windowsHide: true,
    stdio: 'ignore',
  });
  let launchError;
  child.on('error', error => { launchError = error; });
  const deadline = Date.now() + 45_000;
  let ready = false;
  while (Date.now() < deadline && child.exitCode === null && !launchError) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/version`);
      if (response.ok) { ready = true; break; }
    } catch { /* WebView2 is starting. */ }
    await sleep(300);
  }
  if (!ready) {
    if (child.exitCode === null) child.kill();
    throw new Error(launchError?.message || 'Private WebView2 did not start.');
  }
  const { chromium } = await import(pathToFileURL(playwrightModule).href);
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const page = browser.contexts()[0].pages()[0];
  await page.waitForFunction(() => typeof window.__TAURI_INTERNALS__?.invoke === 'function');
  const invoke = (command, args = {}) => page.evaluate(
    ({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
  const status = await invoke('get_config_status');
  if (!status.configPath?.toLowerCase().startsWith(privateHome.toLowerCase() + path.sep)) {
    await invoke('exit_app').catch(() => {});
    await browser.close();
    throw new Error('Native application is not using the isolated config path.');
  }
  const configured = await invoke('configure_provider', { apiKey, baseUrl });
  if (!configured.directImageConfigured) throw new Error('Image rules were not configured.');
  const capabilities = await invoke('check_image_capabilities', { model: 'gpt-image-2' });
  const context = { root, home: privateHome, env, child, browser, page, invoke, capabilities };
  await saveReport(context, 'setup', {
    executable, pid: child.pid, codexHome: privateHome, configured,
    capabilities, restartCalled: false,
    scope: 'Real installed Tauri backend in a private CODEX_HOME and WebView2 profile.',
  });
  return context;
}

export async function saveReport(context, name, data) {
  const filename = path.join(context.root, `${name}.json`);
  await fs.writeFile(filename, JSON.stringify(data, null, 2), { flag: 'wx' });
  return filename;
}

export async function startImageTest(context, request) {
  const job = await context.invoke('test_image_api', { request });
  return { id: job.id, request, started: Date.now() };
}

export async function inspectImageTest(context, test) {
  const job = await context.invoke('get_image_test_status', { jobId: test.id });
  const clean = { ...job, items: job.items.map(({ previewDataUrl, ...item }) => item) };
  if (job.status !== 'running') {
    for (const item of clean.items) {
      if (item.path) {
        const bytes = await fs.readFile(item.path);
        item.sha256 = digest(bytes);
        item.bytes = bytes.length;
      }
    }
    clean.wallElapsedMs = Date.now() - test.started;
    await fs.writeFile(path.join(context.root, `job-${job.id}.json`),
      JSON.stringify({ request: test.request, result: clean }, null, 2));
  }
  return clean;
}

export async function stopNative(context) {
  await context.invoke('exit_app').catch(() => {});
  await context.browser.close().catch(() => {});
  if (context.child.exitCode === null) {
    await Promise.race([once(context.child, 'exit'), sleep(5000)]);
  }
  if (context.child.exitCode === null) context.child.kill();
}
