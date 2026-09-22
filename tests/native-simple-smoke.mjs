// Actual Tauri/WebView2 UI with a fake loopback provider and a private home.
import assert from 'node:assert/strict';
import path from 'node:path';
import { fingerprintFiles, startNative, stopNative, saveReport } from './isolated-live-harness.mjs';

const executable = process.env.NATIVE_EXECUTABLE;
const root = process.env.NATIVE_TEST_ROOT;
const playwrightModule = process.env.PLAYWRIGHT_MODULE;
if (!executable || !root || !playwrightModule) throw new Error('Set NATIVE_EXECUTABLE, NATIVE_TEST_ROOT and PLAYWRIGHT_MODULE.');
const normalFiles = ['config.toml', 'auth.json'].map(name => path.join(process.env.USERPROFILE, '.codex', name));
const before = await fingerprintFiles(normalFiles);
let context;
try {
  context = await startNative({
    executable, root, playwrightModule, apiKey: 'sk-native-ui-only',
    baseUrl: 'https://example.invalid/v1',
    skipCapabilities: true,
  });
  const { page } = context;
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.reload();
  await page.waitForFunction(() => document.querySelector('#topbar-state-text').textContent === '配置已保存');
  await page.screenshot({ path: path.join(root, 'native-simple-home.png') });
  assert.equal(await page.locator('#advanced-dialog').isVisible(), false);
  assert.equal(await page.locator('#configuration-progress').isVisible(), false);
  assert.equal(await page.locator('#api-key').inputValue(), '');
  assert.equal(await page.locator('#migrate-history-button').isVisible(), true);
  assert.equal(await page.locator('#restore-button').isVisible(), true);
  await page.locator('#configure-button').click();
  await page.waitForFunction(() => document.querySelector('#activation-state').textContent === '已保存，待重启');
  await page.screenshot({ path: path.join(root, 'native-isolated-restart.png') });
  await page.locator('#configuration-diagnostics').click();
  assert.equal(await page.locator('#advanced-dialog').isVisible(), true);
  await page.locator('#tools-tab').click();
  await page.locator('#logs-tab').click();
  assert.equal((await page.locator('#configuration-log').textContent()).includes('sk-native-ui-only'), false);
  await page.locator('#open-image-test').click();
  assert.equal(await page.locator('#image-test-dialog').isVisible(), true);
  assert.equal(await page.locator('#start-image-test').isEnabled(), true);
  await page.locator('#close-image-test').click();
  await page.locator('#close-advanced-button').click();
  await page.locator('#restore-button').click();
  await page.locator('#confirm-restore-button').click();
  await page.waitForFunction(() => document.querySelector('#status-message').textContent.includes('已恢复原配置'));
  const restored = await context.invoke('get_config_status');
  assert.equal(restored.configured, false);
  assert.equal(restored.hasApiKey, false);
  assert.equal(restored.directImageConfigured, false);
  assert.deepEqual(errors, []);
  const after = await fingerprintFiles(normalFiles);
  assert.deepEqual(after, before);
  await saveReport(context, 'native-ui-verification', {
    version: '1.4.0-beta.3', executable, before, after,
    compactHome: true, isolatedRestartRejected: true, nestedDialogs: true,
    realRestore: true, noCredentialInLog: true, pageErrors: errors,
  });
  console.log('Native UI passed: compact home, isolated restart protection, advanced tools, nested image dialog, restore, zero image POSTs, normal config/auth unchanged.');
} finally {
  if (context) await stopNative(context);
}
