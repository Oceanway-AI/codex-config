// Local-only UI fixture: never invokes Tauri or contacts a provider.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { resolve, sep } from 'node:path';
const root = fileURLToPath(new URL('../src/', import.meta.url));
createServer(async (req, res) => {
  try {
    const pathname = new URL(req.url, 'http://127.0.0.1').pathname;
    if (pathname === '/favicon.ico') { res.writeHead(204).end(); return; }
    const path = pathname === '/__fixture.js' ? fileURLToPath(new URL('./ui-fixture.js', import.meta.url)) : resolve(root, '.' + (pathname === '/' ? '/index.html' : pathname));
    if (pathname !== '/__fixture.js' && !path.startsWith(root.endsWith(sep) ? root : root + sep)) throw Error('invalid path');
    let data = await readFile(path);
    if (path.endsWith('index.html')) data = data.toString().replace('<head>', '<head><script src="/__fixture.js"></script>');
    res.setHeader('Content-Type', path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : path.endsWith('.png') ? 'image/png' : 'text/html');
    res.end(data);
  } catch { res.writeHead(404).end('Not found'); }
}).listen(4186, '127.0.0.1', () => console.log('Test fixture: http://127.0.0.1:4186 (fake credentials only)'));
