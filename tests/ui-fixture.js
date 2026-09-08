// This file is served ONLY by serve-fixture.mjs, never packaged with the app.
(() => {
  let saved = false, baseUrl = 'https://example.invalid';
  const calls = [];
  window.__TAURI__ = { core: { invoke: async (name, args) => {
    calls.push(name);
    const output = document.querySelector('#fixture-calls');
    if (output) output.textContent = calls.join(' → ');
    await new Promise(resolve => setTimeout(resolve, 40));
    const failure = document.querySelector('#fixture-failure')?.value;
    if (failure === name) throw new Error('模拟故障 sk-fake-acceptance');
    if (name === 'configure_provider') { saved = true; baseUrl = args.baseUrl; return {}; }
    if (name === 'get_config_status') return { configured:saved, hasApiKey:saved, imagegenCliConfigured:saved, baseUrl };
    if (name === 'restart_codex') return { restarted:true };
    if (name === 'restore_defaults') { saved=false; return {}; }
    if (name === 'get_system_info') return { osName:'隔离测试', codexVersion:'模拟版本' };
    if (name === 'run_diagnostics') return { passed:1, checks:[{status:'pass',label:'模拟诊断',detail:'不进行网络请求'}] };
    return {};
  } } };
  document.addEventListener('DOMContentLoaded', () => {
    const fixture = document.createElement('aside');
    fixture.style.cssText = 'position:fixed;bottom:0;left:0;right:0;z-index:9999;background:#fff4cf;padding:4px;font-size:11px;max-height:60px;overflow:auto';
    fixture.innerHTML = '<label>隔离故障注入 <select id="fixture-failure"><option value="">无故障</option><option value="configure_provider">保存失败</option><option value="get_config_status">检查失败</option><option value="restart_codex">重启失败</option></select></label> <button id="fixture-reset-calls">清空调用计数</button> <span id="fixture-calls"></span>';
    document.body.append(fixture);
    document.querySelector('#fixture-reset-calls').onclick = () => {calls.length=0;document.querySelector('#fixture-calls').textContent='';};
  });
})();
