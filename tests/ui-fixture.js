// This file is served ONLY by serve-fixture.mjs, never packaged with the app.
(() => {
  window.__IMAGE_API_SIMULATION__ = true;
  let saved = false, baseUrl = 'https://example.invalid';
  let imageJob = null, ticks = 0, retried = false, serial = 0;
  const imageCounts = () => {
    imageJob.completed = imageJob.items.filter(item => item.status === 'succeeded').length;
    imageJob.failed = imageJob.items.filter(item => item.status === 'failed').length;
    imageJob.cancelled = imageJob.items.filter(item => item.status === 'cancelled').length;
    return structuredClone(imageJob);
  };
  const succeed = item => {
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 160;
    const context = canvas.getContext('2d');
    context.fillStyle = '#e9faf5';
    context.fillRect(0, 0, 160, 160);
    context.fillStyle = '#147653';
    context.font = '18px sans-serif';
    context.fillText('SIMULATION', 16, 72);
    context.fillText(`#${item.index + 1}`, 65, 102);
    return { index: item.index, status: 'succeeded', path: `simulation/image-${item.index + 1}.png`, previewDataUrl: canvas.toDataURL(), requestId: 'simulation-request', elapsedMs: 1000 };
  };
  const calls = [];
  window.__TAURI__ = { core: { invoke: async (name, args) => {
    calls.push(name);
    const output = document.querySelector('#fixture-calls');
    if (output) output.textContent = calls.join(' → ');
    await new Promise(resolve => setTimeout(resolve, 40));
    const failure = document.querySelector('#fixture-failure')?.value;
    if (failure === name) throw new Error('模拟故障 sk-fake-acceptance');
    if (name === 'configure_provider') { saved = true; baseUrl = args.baseUrl; return {}; }
    if (name === 'get_config_status') return { configured:saved, hasApiKey:saved, directImageConfigured:saved, baseUrl };
    if (name === 'restart_codex') return { restarted:true };
    if (name === 'restore_defaults') { saved=false; return {}; }
    if (name === 'get_system_info') return { osName:'隔离测试', codexVersion:'模拟版本' };
    if (name === 'run_diagnostics') return { passed:1, checks:[{status:'pass',label:'模拟诊断',detail:'不进行网络请求'}] };
    if (name === 'check_image_capabilities') return { model: args.model, available: true, models: ['gpt-image-2', 'simulation-custom-model'], endpoint: `${baseUrl}/v1/models`, message: '模拟模型列表，不代表真实能力' };
    if (name === 'pick_reference_images') return ['C:\\simulation\\reference-1.png', 'C:\\simulation\\reference-2.png'];
    if (name === 'test_image_api') {
      ticks = 0; retried = false;
      imageJob = { id: `simulation-${++serial}`, status: 'running', model: args.request.model, mode: args.request.referencePaths.length ? 'edit' : 'generate', total: args.request.count, items: Array.from({ length: args.request.count }, (_, index) => ({ index, status: index < 2 ? 'running' : 'queued' })), message: '模拟任务：不会发送网络请求或产生费用' };
      return imageCounts();
    }
    if (name === 'get_image_test_status') {
      if (imageJob.status === 'running' && ++ticks >= 3) {
        imageJob.items = imageJob.items.map(item => item.status === 'succeeded' || item.status === 'cancelled' ? item : !retried && item.index === 1 ? { index: 1, status: 'failed', error: '模拟失败，可显式重试' } : succeed(item));
        imageJob.status = imageJob.items.some(item => item.status === 'failed') ? 'partial' : 'completed';
      }
      return imageCounts();
    }
    if (name === 'cancel_image_test') {
      imageJob.items = imageJob.items.map(item => ['queued', 'running'].includes(item.status) ? { index: item.index, status: 'cancelled' } : item);
      imageJob.status = 'cancelled';
      return imageCounts();
    }
    if (name === 'retry_image_test') {
      retried = true; ticks = 0; imageJob.status = 'running';
      imageJob.items = imageJob.items.map(item => item.status === 'failed' ? { index: item.index, status: 'running' } : item);
      return imageCounts();
    }
    if (name === 'open_image_result') return {};
    return {};
  } } };
  document.addEventListener('DOMContentLoaded', () => {
    const fixture = document.createElement('aside');
    fixture.style.cssText = 'position:fixed;bottom:0;left:0;right:0;z-index:9999;background:#fff4cf;padding:4px;font-size:11px;max-height:60px;overflow:auto';
    fixture.innerHTML = '<label>模拟预览 / 故障注入 <select id="fixture-failure"><option value="">无故障</option><option value="configure_provider">保存失败</option><option value="configure_direct_image_api">图片同步失败</option><option value="get_config_status">检查失败</option><option value="restart_codex">重启失败</option><option value="test_image_api">图片提交失败</option><option value="get_image_test_status">图片轮询失败</option><option value="cancel_image_test">图片取消失败</option><option value="retry_image_test">图片重试失败</option></select></label> <button id="fixture-reset-calls">清空调用计数</button> <span id="fixture-calls"></span>';
    document.body.append(fixture);
    document.querySelector('#fixture-reset-calls').onclick = () => {calls.length=0;document.querySelector('#fixture-calls').textContent='';};
  });
})();
