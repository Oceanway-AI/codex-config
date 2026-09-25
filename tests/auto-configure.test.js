import { test } from 'node:test';
import assert from 'node:assert/strict';
import { runAutoConfiguration } from '../src/auto-configure.js';

const ready = { configured: true, hasApiKey: true, directImageConfigured: true };
function fixture(overrides = {}) {
  const calls = [], stages = [], args = [];
  return {
    calls, stages, args, values: {}, onStage: phase => stages.push(phase), onConfigured: () => {},
    invoke: async (name, input) => {
      calls.push(name); args.push(input);
      if (name in overrides) {
        if (overrides[name] instanceof Error) throw overrides[name];
        return overrides[name];
      }
      if (name === 'get_config_status') return ready;
      if (name === 'check_image_mcp') return { toolsAvailable: true };
      if (name === 'get_language_status') return { applied: true, verified: false };
      if (name === 'restart_codex') return { restarted: true };
      return {};
    },
  };
}

test('one action saves, handshakes, stages language and restarts without paid tools', async () => {
  const f = fixture();
  const result = await runAutoConfiguration(f);
  assert.deepEqual(f.calls, ['configure_provider', 'get_config_status', 'check_image_mcp',
    'configure_language', 'restart_codex', 'get_config_status', 'get_language_status']);
  assert.deepEqual(f.stages, ['writing', 'checking', 'language', 'restarting', 'complete']);
  assert.equal(result.languageStatus.verified, false);
  assert.equal(f.calls.some(name => /test_image|generate/.test(name)), false);
});
test('retrying a restart rechecks saved configuration and the current language choice', async () => {
  const f = fixture();
  await runAutoConfiguration({ ...f, resumeFrom: 'restarting' });
  assert.deepEqual(f.calls, ['get_config_status', 'check_image_mcp', 'configure_language', 'restart_codex', 'get_config_status', 'get_language_status']);
});
test('checking and language retries do not rewrite provider credentials', async () => {
  for (const resumeFrom of ['checking', 'language']) {
    const f = fixture();
    await runAutoConfiguration({ ...f, resumeFrom });
    assert.equal(f.calls.includes('configure_provider'), false);
    assert.equal(f.calls.filter(name => name === 'configure_language').length, 1);
  }
});
test('failed write, readback or handshake prevents restart and fake completion', async () => {
  for (const overrides of [
    { configure_provider: new Error('write failed') },
    { get_config_status: {} },
    { check_image_mcp: { toolsAvailable: false, message: 'handshake failed' } },
  ]) {
    const f = fixture(overrides);
    await assert.rejects(runAutoConfiguration(f));
    assert.equal(f.calls.includes('restart_codex'), false);
    assert.equal(f.stages.includes('complete'), false);
  }
});
test('restart failure is surfaced without automatic retry', async () => {
  const f = fixture({ restart_codex: { restarted: false, message: 'quit refused' } });
  await assert.rejects(runAutoConfiguration(f), /quit refused/);
  assert.equal(f.calls.filter(name => name === 'restart_codex').length, 1);
  assert.equal(f.stages.includes('complete'), false);
});
test('unsupported language does not discard usable provider and MCP setup', async () => {
  const f = fixture({ configure_language: { supported: false }, get_language_status: { applied: false } });
  let last = '';
  await runAutoConfiguration({ ...f, onStage: (_, message) => { last = message; } });
  assert.ok(last.includes('中文界面未应用'));
});
test('unchecking Chinese passes disabled without implicitly restoring a user language', async () => {
  const f = fixture();
  await runAutoConfiguration({ ...f, values: { chineseInterface: false } });
  assert.deepEqual(f.args[f.calls.indexOf('configure_language')], { enabled: false });
  assert.equal(f.calls.includes('restore_language'), false);
});
test('legacy CLI readiness cannot satisfy MCP configuration readiness', async () => {
  const f = fixture({ get_config_status: { configured: true, hasApiKey: true, imagegenCliConfigured: true } });
  await assert.rejects(runAutoConfiguration({ ...f, resumeFrom: 'checking' }), /回读检查未通过/);
});
test('post-restart configuration drift cannot be reported as success', async () => {
  const f = fixture();
  let restarted = false;
  const call = f.invoke;
  await assert.rejects(runAutoConfiguration({ ...f, invoke: async (name, args) => {
    if (name === 'restart_codex') restarted = true;
    if (name === 'get_config_status' && restarted) return {};
    return call(name, args);
  } }), /重启后配置回读未通过/);
  assert.equal(f.stages.includes('complete'), false);
});
test('language status read failure preserves provider readiness with an explicit warning', async () => {
  const f = fixture({ get_language_status: new Error('language read failed') });
  let completion;
  const result = await runAutoConfiguration({ ...f, onStage: (phase, message, options) => {
    if (phase === 'complete') completion = { message, options };
  } });
  assert.equal(result.configured, true);
  assert.equal(result.languageStatus.verified, false);
  assert.equal(result.languageStatus.error, true);
  assert.equal(completion.options.warning, true);
  assert.match(completion.message, /语言状态尚未确认/);
  assert.equal(f.calls.filter(name => name === 'restart_codex').length, 1);
});
