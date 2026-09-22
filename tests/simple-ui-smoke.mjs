// Loopback fixture only: never touches the user's Codex configuration or provider.
import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ? pathToFileURL(process.env.PLAYWRIGHT_MODULE).href : 'playwright');
const browser = await chromium.launch({ headless: true, channel: 'msedge' });
const page = await browser.newPage({ viewport: { width: 640, height: 620 } });
const errors = [];
page.on('pageerror', error => errors.push(error.message));
const text = (selector, value) => page.waitForFunction(
  ({ selector, value }) => document.querySelector(selector)?.textContent.includes(value), { selector, value });
const calls = async command => (await page.locator('#fixture-calls').textContent()).split(' → ').filter(name => name === command).length;
const idle = () => page.waitForFunction(() => !document.querySelector('#configure-button').disabled);
const start = async (query = '') => {
  await page.goto(`http://127.0.0.1:4186/${query}`);
  await text('#topbar-state-text', query ? '配置已保存' : '等待完成配置');
  await page.waitForTimeout(1100);
  await idle();
};
const overflow = async selector => {
  const box = await page.locator(selector).evaluate(element => ({
    scroll: element.scrollWidth, width: element.clientWidth,
    left: element.getBoundingClientRect().left, right: element.getBoundingClientRect().right,
  }));
  assert.ok(box.scroll <= box.width + 1 && box.left >= 0 && box.right <= page.viewportSize().width + 1, JSON.stringify(box));
};
try {
  await mkdir('dist/ui-tests', { recursive: true });
  await start();
  assert.equal(await page.locator('#advanced-dialog').isVisible(), false);
  assert.equal(await page.locator('#configuration-progress').isVisible(), false);
  assert.equal(await page.locator('#migrate-history-button').isVisible(), true);
  assert.equal(await page.locator('#restore-button').isVisible(), true);
  assert.equal(await calls('run_diagnostics'), 0);
  assert.equal(await calls('get_history_migration_status'), 0);
  assert.equal(await calls('test_image_api'), 0);
  assert.equal(await page.locator('#config-form input').count(), 2);
  for (const width of [1120, 640, 480, 390, 320]) {
    await page.setViewportSize({ width, height: 620 });
    await overflow('.workspace');
    await overflow('#quick-actions');
    await page.screenshot({ path: `dist/ui-tests/simple-home-${width}.png` });
  }
  await page.setViewportSize({ width: 640, height: 620 });
  await page.locator('#api-key').fill('sk-fake-acceptance');
  await page.evaluate(() => { window.__UI_FIXTURE__.delay = 180; });
  await page.locator('#configure-button').click();
  assert.equal(await page.locator('#advanced-dialog').isVisible(), false);
  assert.equal(await page.locator('#configuration-progress').isVisible(), true);
  assert.equal(await page.locator('#migrate-history-button').isDisabled(), true);
  assert.equal(await page.locator('#restore-button').isDisabled(), true);
  await text('#activation-state', '配置完成');
  await idle();
  assert.equal(await page.locator('#configuration-progress').isVisible(), false);
  assert.equal(await page.locator('#status').isVisible(), true);
  assert.equal(await page.locator('#api-key').inputValue(), '');
  assert.equal(await calls('configure_provider'), 1);
  assert.equal(await calls('configure_direct_image_api'), 1);
  assert.equal(await calls('restart_codex'), 1);
  assert.equal(await calls('test_image_api'), 0);
  await page.screenshot({ path: 'dist/ui-tests/simple-complete.png' });
  await page.locator('#open-advanced-button').click();
  await page.locator('#diagnosis-tab').focus();
  await page.keyboard.press('ArrowRight');
  assert.equal(await page.locator('#tools-tab').getAttribute('aria-selected'), 'true');
  assert.equal(await calls('get_history_migration_status'), 0);
  await page.locator('#test-button').click();
  await text('#advanced-status', '模拟连接可用');
  await page.locator('#logs-tab').click();
  await text('#configuration-log', '配置完成');
  await page.locator('#diagnosis-tab').click();
  await page.locator('#run-diagnostics-button').click();
  await text('#diagnostic-summary-title', '全部核心检查通过');
  await page.screenshot({ path: 'dist/ui-tests/simple-advanced.png' });
  await page.keyboard.press('Escape');
  assert.equal(await page.locator('#advanced-dialog').isVisible(), false);

  // Restart failure must not look like a failed save, or rewrite on explicit retry.
  await page.locator('#fixture-failure').selectOption('restart_codex');
  await page.locator('#configure-button').click();
  await text('#activation-state', '已保存，待重启');
  await idle();
  assert.equal(await page.locator('#configuration-progress').isVisible(), true);
  assert.equal(await page.locator('#next-step-title').textContent().then(value => value.includes('sk-fake-acceptance')), false);
  const writes = await calls('configure_provider');
  await page.screenshot({ path: 'dist/ui-tests/simple-restart-blocked.png' });
  await page.locator('#configuration-diagnostics').click();
  assert.equal(await page.locator('#advanced-dialog').isVisible(), true);
  assert.equal(await page.locator('#diagnosis-tab').getAttribute('aria-selected'), 'true');
  await page.keyboard.press('Escape');
  await page.locator('#fixture-failure').selectOption('');
  await page.locator('#retry-configuration').click();
  await text('#activation-state', '配置完成');
  await idle();
  assert.equal(await calls('configure_provider'), writes);

  // A returned restart rejection is also incomplete, not just a thrown error.
  await page.evaluate(() => { window.__UI_FIXTURE__.restart = false; });
  await page.locator('#configure-button').click();
  await text('#activation-state', '已保存，待重启');
  await idle();
  await page.locator('#base-url').fill('https://changed.example.invalid/v1');
  await text('#topbar-state-text', '修改未保存');
  await page.evaluate(() => { window.__UI_FIXTURE__.restart = true; });
  await page.locator('#retry-configuration').click();
  await text('#activation-state', '配置完成');
  await idle();
  assert.equal(await calls('configure_provider'), writes + 2);

  // Explicit migration only, with encrypted-content and backup confirmation.
  await page.evaluate(() => { window.__UI_FIXTURE__.migration = 'unsupported'; });
  await page.locator('#migrate-history-button').click();
  await text('#status-message', '请先完成');
  await idle();
  assert.equal(await calls('migrate_history_visibility'), 0);
  await page.evaluate(() => { window.__UI_FIXTURE__.migration = 'done'; });
  await page.locator('#migrate-history-button').click();
  await text('#status-message', '无需迁移');
  await idle();
  await page.evaluate(() => { window.__UI_FIXTURE__.migration = 'needed'; });
  page.once('dialog', async dialog => {
    assert.match(dialog.message(), /加密内容/);
    assert.match(dialog.message(), /自动备份/);
    await dialog.dismiss();
  });
  await page.locator('#migrate-history-button').click();
  await text('#status-message', '已取消历史迁移');
  await idle();
  assert.equal(await calls('migrate_history_visibility'), 0);
  page.once('dialog', dialog => dialog.accept());
  await page.locator('#migrate-history-button').click();
  await text('#status-message', '历史迁移完成');
  await idle();
  assert.equal(await calls('migrate_history_visibility'), 1);

  await page.locator('#restore-button').click();
  await page.locator('#cancel-restore-button').click();
  assert.equal(await calls('restore_defaults'), 0);
  await page.locator('#restore-button').click();
  await page.locator('#confirm-restore-button').click();
  await text('#status-message', '已恢复原配置');
  await idle();
  await text('#status-message', '撤销 2 个历史文件');
  assert.equal(await calls('restore_defaults'), 1);
  assert.equal(await page.locator('#configuration-progress').isVisible(), false);
  assert.equal(await page.locator('#api-key').inputValue(), '');

  for (const failure of ['configure_provider', 'configure_direct_image_api', 'get_config_status']) {
    await start();
    await page.locator('#api-key').fill('sk-fake-acceptance');
    await page.locator('#fixture-failure').selectOption(failure);
    await page.locator('#configure-button').click();
    await text('#activation-state', '配置未完成');
    await idle();
    assert.equal(await calls('restart_codex'), 0);
    assert.equal(await page.locator('#next-step-title').textContent().then(value => value.includes('sk-fake-acceptance')), false);
  }
  await start('?saved');
  await page.evaluate(() => { window.__UI_FIXTURE__.failRestoreReadback = true; });
  await page.locator('#restore-button').click();
  await page.locator('#confirm-restore-button').click();
  await text('#status-message', '尚未确认当前状态');
  assert.equal(await page.locator('#status').getAttribute('data-kind'), 'warning');
  assert.deepEqual(errors, []);
  console.log('Simple UI passed: home disclosure, widths, one-click flow, locks, diagnostics/logs, failure/retry, dirty input, migration confirmation and restore.');
} finally {
  await browser.close();
}
