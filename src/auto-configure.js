// One user action; stop immediately on failure, without automatic retry.
export async function runAutoConfiguration({ invoke, values, onStage, onConfigured, resumeFrom = 'writing' }) {
  if (!['writing', 'checking', 'restarting'].includes(resumeFrom)) throw new Error('无效的恢复步骤');
  if (resumeFrom === 'writing') {
    onStage('writing', '正在写入 Provider、认证和图片备用配置…');
    await invoke('configure_provider', values);
  }
  onStage('checking', '配置已写入，正在回读检查…');
  const status = await invoke('get_config_status');
  onConfigured(status);
  if (!status.configured || !status.hasApiKey || !status.imagegenCliConfigured) {
    throw new Error('配置回读检查未通过，已停止自动重启。请查看问题诊断。');
  }
  onStage('restarting', '回读检查通过，正在重新启动 Codex…');
  const restart = await invoke('restart_codex');
  if (!restart.restarted) throw new Error(restart.message || '未能重新启动 Codex。');
  onStage('complete', '一键配置完成，已请求重新打开 Codex。请在新任务中使用；图片能力未自动验证。');
  return status;
}
