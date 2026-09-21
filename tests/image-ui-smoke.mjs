// Local fixture only. Set PLAYWRIGHT_MODULE to a bundled playwright index.mjs if needed.
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ? pathToFileURL(process.env.PLAYWRIGHT_MODULE).href : 'playwright');
const browser = await chromium.launch({ headless: true, channel: 'msedge' });
const page = await browser.newPage({ viewport: { width: 1120, height: 680 } });
const errors = [];
page.on('pageerror', error => errors.push(error.message));
const expectText = async (selector, text) => {
  await page.waitForFunction(({ selector, text }) => document.querySelector(selector)?.textContent.includes(text), { selector, text });
};
const calls = async command => (await page.locator('#fixture-calls').textContent()).split(' → ').filter(call => call === command).length;
try {
  await page.goto('http://127.0.0.1:4186');
  await expectText('#service-status', '未配置');
  await page.waitForTimeout(1200);
  assert.equal(await calls('test_image_api'), 0);
  await page.locator('#open-image-test').click();
  assert.equal(await page.locator('#image-size').inputValue(), '1024x1024');
  assert.equal(await page.locator('#start-image-test').isDisabled(), true);
  await expectText('#image-rule-status', '自然触发待确认');
  await page.locator('#close-image-test').click();
  await page.locator('#api-key').fill('sk-simulation-only');
  await page.locator('#configure-button').click();
  await expectText('#activation-state', '配置完成');
  await page.locator('#open-image-test').click();
  await page.locator('#check-image-model').click();
  await expectText('#image-model-evidence', '已检查');
  await expectText('#image-generate-evidence', '未测试');
  assert.equal(await calls('test_image_api'), 0);
  await page.locator('#image-prompt').fill('Simulation only: a red square');
  await page.locator('#image-count').fill('3');
  await page.locator('#start-image-test').click();
  await expectText('#image-payment-detail', '请保持应用窗口开启');
  await expectText('#image-payment-detail', '不支持关闭或重启应用后自动恢复任务');
  await expectText('#image-payment-detail', '取消仅停止待发请求，已发送请求仍可能计费');
  assert.equal(await calls('test_image_api'), 0);
  await page.locator('#image-payment-dialog button[value="cancel"]').click();
  assert.equal(await calls('test_image_api'), 0);
  await page.locator('#start-image-test').click();
  await page.locator('#confirm-image-payment').click();
  await expectText('#image-job-summary', '执行中');
  await expectText('#image-block-reason', '关闭或重启应用后不会自动恢复任务');
  assert.equal(await calls('test_image_api'), 1);
  assert.equal(await page.locator('#configure-button').isDisabled(), true);
  assert.equal(await page.locator('#repair-button').isDisabled(), true);
  assert.equal(await page.locator('#update-button').isDisabled(), true);
  assert.equal(await page.locator('#cancel-image-test').isEnabled(), true);
  await expectText('#image-job-summary', '部分完成');
  await expectText('#image-generate-evidence', '部分通过');
  assert.equal(await page.locator('#image-results li[data-kind="succeeded"]').count(), 2);
  assert.deepEqual(await page.locator('#image-results li > strong').allTextContents(), [
    '模拟预览：#1 · 成功', '模拟预览：#2 · 失败', '模拟预览：#3 · 成功',
  ]);
  assert.equal(await page.locator('#image-results img').first().getAttribute('alt'), '模拟预览：图片 1');
  const successfulPreview = await page.locator('#image-results img').first().getAttribute('src');
  await page.locator('#retry-image-test').click();
  assert.equal(await calls('retry_image_test'), 0);
  await page.locator('#confirm-image-payment').click();
  await expectText('#image-job-summary', '执行中');
  assert.equal(await page.locator('#image-results img').first().getAttribute('src'), successfulPreview);
  await expectText('#image-job-summary', '已完成');
  await expectText('#image-generate-evidence', '已实测通过');
  await expectText('#image-edit-evidence', '未测试');
  assert.equal(await calls('retry_image_test'), 1);
  await page.locator('#image-results button').first().click();
  assert.equal(await calls('open_image_result'), 1);
  await page.waitForFunction(() => document.querySelector('#fixture-calls').dataset.openedIndex === '1');
  await page.locator('#pick-image-references').click();
  await expectText('#image-mode', '2 张参考图');
  await page.locator('#pick-image-references').click();
  await expectText('#image-mode', '4 张参考图');
  assert.deepEqual(await page.locator('#image-references li > span').allTextContents(), [
    '1. reference-1.png', '2. reference-2.png', '3. reference-1.png', '4. reference-2.png',
  ]);
  await page.locator('#image-references button').nth(2).click();
  await page.locator('#image-references button').nth(2).click();
  await expectText('#image-mode', '2 张参考图');
  await page.locator('#image-count').fill('');
  await page.locator('#start-image-test').click();
  await expectText('#image-payment-detail', '1 张图片');
  await page.locator('#confirm-image-payment').click();
  await expectText('#image-job-summary', '执行中');
  await page.locator('#cancel-image-test').click();
  await expectText('#image-job-summary', '已取消');
  await expectText('#image-edit-evidence', '已取消');
  assert.equal(await page.locator('#retry-image-test').isEnabled(), true);
  await page.locator('#retry-image-test').click();
  await expectText('#image-payment-detail', '仅重试失败或取消的 1 张图片');
  assert.equal(await calls('retry_image_test'), 1);
  await page.locator('#confirm-image-payment').click();
  await expectText('#image-job-summary', '已完成');
  assert.equal(await calls('retry_image_test'), 2);
  await expectText('#image-edit-evidence', '已实测通过');
  await expectText('#image-rule-status', '自然触发待确认');
  await page.locator('.image-dialog-body').evaluate(element => { element.scrollTop = 0; });
  if (process.env.IMAGE_UI_SCREENSHOTS) await page.screenshot({ path: 'tests/fixtures/image-ui-desktop.png' });
  for (const width of [390, 320]) {
    await page.setViewportSize({ width, height: 844 });
    await page.locator('.image-dialog-body').evaluate(element => { element.scrollTop = 0; });
    if (process.env.IMAGE_UI_SCREENSHOTS) await page.screenshot({ path: `tests/fixtures/image-ui-${width}.png` });
    const overflow = await page.locator('#image-test-dialog').evaluate(element => ({
      scroll: element.scrollWidth, width: element.clientWidth,
      left: element.getBoundingClientRect().left, right: element.getBoundingClientRect().right,
    }));
    assert.ok(overflow.scroll <= overflow.width + 1, JSON.stringify(overflow));
    assert.ok(overflow.left >= 0 && overflow.right <= width, JSON.stringify(overflow));
  }
  await page.setViewportSize({ width: 1120, height: 680 });
  await page.locator('#close-image-test').click();
  await page.locator('#base-url').fill('https://changed.example.invalid');
  await page.locator('#open-image-test').click();
  await expectText('#image-generate-evidence', '已过期');
  await expectText('#image-edit-evidence', '已过期');
  assert.equal(await page.locator('#start-image-test').isDisabled(), true);
  await page.locator('#close-image-test').click();
  await page.locator('#configure-button').click();
  await expectText('#activation-state', '配置完成');
  await page.locator('#open-image-test').click();
  await expectText('#image-edit-evidence', '已过期');
  await page.locator('#close-image-test').click();
  await page.locator('#fixture-failure').selectOption('get_image_test_status');
  await page.locator('#open-image-test').click();
  await page.locator('#start-image-test').click();
  await page.locator('#confirm-image-payment').click();
  await expectText('#image-operation-message', '图片操作失败');
  assert.equal(await page.locator('#configure-button').isDisabled(), true);
  assert.equal(await page.locator('#cancel-image-test').isEnabled(), true);
  await page.locator('#cancel-image-test').click();
  await expectText('#image-job-summary', '已取消');
  assert.equal(await page.locator('#configure-button').isEnabled(), true);
  await page.goto('http://127.0.0.1:4186/?preview');
  await page.locator('#open-image-test').click();
  await expectText('#image-block-reason', '不能执行真实');
  assert.equal(await page.locator('#start-image-test').isDisabled(), true);
  assert.equal(await page.locator('#check-image-model').isDisabled(), true);
  await expectText('#image-config-evidence', '模拟预览');
  await expectText('#image-generate-evidence', '模拟预览：未测试');
  assert.deepEqual(errors, []);
  console.log('UI smoke passed: explicit confirmation, model-only check, partial/retry/preserve, references, cancel, outdated credentials, poll recovery, desktop/mobile, no page errors.');
} finally {
  await browser.close();
}
