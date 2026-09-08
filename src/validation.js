export function validateBaseUrl(value) {
  const message = 'Base URL 必须是完整的 HTTP(S) 地址，且不能包含账号密码、查询参数或片段。';
  let url;
  try { url = new URL(value); } catch { throw new Error(message); }
  if (!['http:', 'https:'].includes(url.protocol) || !url.hostname || url.username || url.password || url.search || url.hash || /[?#\s]/.test(value)) throw new Error(message);
  return value;
}
