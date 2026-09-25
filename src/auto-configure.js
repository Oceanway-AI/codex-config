// One user action; stop immediately on failure, without automatic retry.
export async function runAutoConfiguration({ invoke, values, onStage, onConfigured, resumeFrom = 'writing' }) {
  if (!['writing', 'checking', 'language', 'restarting'].includes(resumeFrom)) throw new Error('无效的恢复步骤');
  if (resumeFrom === 'writing') {
    onStage('writing', '正在保存认证、图片 MCP 和短规则…');
    await invoke('configure_provider', values);
  }
  onStage('checking', '配置已写入，正在回读检查…');
  const status = await invoke('get_config_status');
  onConfigured(status);
  if (!status.configured || !status.hasApiKey || !status.directImageConfigured) {
    throw new Error('配置回读检查未通过，已停止自动重启。请查看问题诊断。');
  }
  const mcp = await invoke('check_image_mcp');
  if (!mcp?.toolsAvailable) throw new Error(mcp?.message || '图片 MCP 工具握手失败，未重启。');
  let language;
  onStage('language', '正在确认中文界面设置…');
  language = await invoke('configure_language', { enabled: values.chineseInterface !== false });
  onStage('restarting', '配置和工具检查通过，正在重新启动 Codex…');
  const restart = await invoke('restart_codex');
  if (!restart.restarted) throw new Error(restart.message || '未能重新启动 Codex。');
  const afterRestart = await invoke('get_config_status');
  if (!afterRestart.configured || !afterRestart.hasApiKey || !afterRestart.directImageConfigured) {
    throw new Error('重启后配置回读未通过，不能确认配置生效。请检查后重新配置。');
  }
  language = await invoke('get_language_status').catch(() => ({
    applied: false, verified: false, error: true, message: '语言状态读取失败，尚未确认。',
  }));
  onConfigured({ ...afterRestart, imageMcpStatus: mcp, languageStatus: language });
  const languageNote = language?.error ? '语言状态尚未确认。' : values.chineseInterface === false ? ''
    : language?.applied ? '中文设置已保存，界面待确认。' : '中文界面未应用，请查看语言状态。';
  onStage('complete', `配置和 MCP 握手通过，Codex 已重启。${languageNote}实际生图未自动测试。`,
    { warning: language?.error || (values.chineseInterface !== false && !language?.applied) });
  return { ...afterRestart, imageMcpStatus: mcp, languageStatus: language };
}
