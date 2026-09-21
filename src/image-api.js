export const DEFAULT_IMAGE_MODEL = 'gpt-image-2';

export function buildImageRequest({ model, prompt, count, referencePaths = [], size = 'auto' }) {
  const total = count === '' || count == null ? 1 : Number(count);
  if (!Number.isSafeInteger(total) || total <= 0) throw new Error('图片数量必须为正整数。');
  if (!prompt?.trim()) throw new Error('请输入图片提示词。');
  if (!model?.trim()) throw new Error('请输入模型名称。');
  return { model: model.trim(), prompt: prompt.trim(), count: total, referencePaths: [...new Set(referencePaths)], size };
}

export function imageTestBlockReason({ native, configured, dirty, busy }) {
  if (!native) return '浏览器预览：未连接本机后端，不能执行真实图片测试。';
  if (busy) return '配置或维护正在进行，请稍后测试。';
  if (dirty) return '连接信息有未保存修改，请先保存配置。测试仅使用已保存的 Key 和 Base URL。';
  if (!configured) return '请先保存并同步直连图片 API 配置。';
  return '';
}

export function createImageEvidence() {
  let revision = 0;
  const records = new Map();
  return {
    invalidate() { revision += 1; },
    get revision() { return revision; },
    record(model, mode, state, atRevision = revision) {
      records.set(`${model}:${mode}`, { state, revision: atRevision });
    },
    status(model, mode) {
      const record = records.get(`${model}:${mode}`);
      return !record ? 'untested' : record.revision === revision ? record.state : 'outdated';
    },
  };
}

export function safeImagePreview(value) {
  return typeof value === 'string' && /^data:image\/(?:png|jpeg|webp|gif);base64,[a-z0-9+/=\s]+$/i.test(value) ? value : '';
}

// Mutating replies supersede in-flight polls. Poll failures retain the last job and lock.
export function createImageJobController({ invoke, onChange = () => {}, onError = () => {}, schedule = setTimeout, unschedule = clearTimeout, interval = 1000 }) {
  let job = null;
  let pending = false;
  let error = null;
  let timer;
  let epoch = 0;
  const notify = () => onChange({ job, pending, error, locked: pending || job?.status === 'running' });
  const stopTimer = () => { unschedule(timer); timer = undefined; };
  const queuePoll = () => {
    stopTimer();
    if (job?.status === 'running') timer = schedule(poll, interval);
  };
  const accept = result => {
    if (!result?.id || !Array.isArray(result.items) || !['running', 'completed', 'partial', 'failed', 'cancelled'].includes(result.status)) {
      throw new Error('图片任务返回格式无效。');
    }
    // A retry may only replace failed slots; keep successful outputs visible.
    if (job?.id === result.id) {
      const successes = new Map(job.items.filter(item => item.status === 'succeeded').map(item => [item.index, item]));
      const returned = new Set(result.items.map(item => item.index));
      result = { ...result, items: result.items.map(item => successes.get(item.index) || item) };
      for (const [index, item] of successes) if (!returned.has(index)) result.items.push(item);
      result.items.sort((a, b) => a.index - b.index);
    }
    job = result;
    error = null;
  };
  async function poll() {
    if (!job || pending) return;
    const token = epoch;
    try {
      const result = await invoke('get_image_test_status', { jobId: job.id });
      if (token !== epoch) return;
      accept(result);
      notify();
    } catch (caught) {
      if (token === epoch) { error = caught; onError(caught); }
    } finally {
      if (token === epoch) queuePoll();
    }
  }
  async function mutate(command, args) {
    if (pending) return;
    pending = true;
    error = null;
    epoch += 1;
    stopTimer();
    notify();
    try { accept(await invoke(command, args)); }
    catch (caught) { error = caught; onError(caught); }
    finally { pending = false; notify(); queuePoll(); }
  }
  return {
    get job() { return job; },
    get locked() { return pending || job?.status === 'running'; },
    start(request) {
      if (this.locked) return;
      return mutate('test_image_api', { request });
    },
    retry() {
      if (this.locked || !job?.items.some(item => item.status === 'failed')) return;
      return mutate('retry_image_test', { jobId: job.id });
    },
    cancel() {
      if (pending || job?.status !== 'running') return;
      return mutate('cancel_image_test', { jobId: job.id });
    },
    poll,
    dispose() { epoch += 1; stopTimer(); },
  };
}
