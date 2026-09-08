
export function redactLogMessage(message, secrets = []) {
  let text = String(message);
  for (const secret of secrets.filter(Boolean).sort((a, b) => b.length - a.length)) text = text.split(secret).join('[已隐藏]');
  return text.replace(/sk-[A-Za-z0-9_-]+/g, '[Key 已隐藏]')
    .replace(/Bearer\s+\S+/gi, 'Bearer [已隐藏]')
    .replace(/(https?:\/\/)[^\s/@]+:[^\s/@]+@/gi, '$1[认证已隐藏]@')
    .replace(/([?&](?:api[_-]?key|token|key|secret)=)[^\s&#]+/gi, '$1[已隐藏]')
    .slice(0, 1200);
}

export function createConfigurationLog({ $, getSecret = () => '', limit = 200 }) {
  const container = $('#configuration-log');
  let entries = [];
  const document = container.ownerDocument;
  function append(message, kind = 'info') {
    if (!message) return;
    const follow = container.scrollHeight - container.scrollTop - container.clientHeight < 48;
    $('#log-placeholder')?.remove();
    const row = document.createElement('div');
    row.className = 'log-entry'; row.dataset.kind = kind;
    const time = document.createElement('time');
    const now = new Date(); time.dateTime = now.toISOString(); time.textContent = now.toLocaleTimeString();
    const text = document.createElement('span');
    // Raw backend failures can contain unknown credentials; retain only a safe
    // failure notice here. Detailed errors remain in the existing action status.
    text.textContent = kind === 'error' ? '操作失败，请查看当前操作提示；敏感错误详情未写入日志。' : redactLogMessage(message, [getSecret()]);
    row.append(time, text); container.append(row); entries.push(row);
    while (entries.length > limit) entries.shift().remove();
    if (follow) container.scrollTop = container.scrollHeight;
  }
  function clear() {
    entries = []; container.replaceChildren();
    const placeholder = document.createElement('p'); placeholder.id = 'log-placeholder';
    placeholder.textContent = '日志已清空。后续操作将继续记录。'; container.append(placeholder);
  }
  $('#clear-logs-button').addEventListener('click', clear);
  return { append, clear };
}
