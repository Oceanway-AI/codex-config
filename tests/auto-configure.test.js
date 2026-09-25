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
      if (name === 'restart_codex') return { restarted: true };
      return {};
    },
  };
}

test('one action only saves, handshakes and restarts without paid tools', async () => {
  const f = fixture();
  const result = await runAutoConfiguration(f);
  assert.deepEqual(f.calls, ['configure_provider', 'get_config_status', 'check_image_mcp',
    'restart_codex', 'get_config_status']);
  assert.deepEqual(f.stages, ['writing', 'checking', 'restarting', 'complete']);
  assert.equal(result.configured, true);
  assert.equal(f.calls.some(name => /test_image|generate/.test(name)), false);
  assert.equal(f.args[f.calls.indexOf('restart_codex')], undefined);
});
test('retrying a restart rechecks saved configuration without rewriting credentials', async () => {
  const f = fixture();
  await runAutoConfiguration({ ...f, resumeFrom: 'restarting' });
  assert.deepEqual(f.calls, ['get_config_status', 'check_image_mcp', 'restart_codex', 'get_config_status']);
});
test('checking retries do not rewrite provider credentials', async () => {
  const f = fixture();
  await runAutoConfiguration({ ...f, resumeFrom: 'checking' });
  assert.equal(f.calls.includes('configure_provider'), false);
});
test('unknown phase is rejected before any configuration operation', async () => {
  const f = fixture();
  await assert.rejects(runAutoConfiguration({ ...f, resumeFrom: 'unknown' }), /无效/);
  assert.deepEqual(f.calls, []);
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
test('completion distinguishes a saved setup from actual generation evidence', async () => {
  const f = fixture();
  let completion;
  await runAutoConfiguration({ ...f,
    onStage: (phase, message) => { if (phase === 'complete') completion = message; } });
  assert.match(completion, /配置和 MCP 握手通过/);
  assert.match(completion, /实际生图未自动测试/);
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
