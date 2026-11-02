"""Fetch official full packages into staging; never publishes partial downloads."""
import concurrent.futures
import hashlib
import json
import subprocess
import sys
from pathlib import Path

root = Path(sys.argv[1])
root.mkdir(parents=True, exist_ok=True)
entries = []
entries.extend([
    dict(app='hermes', name='Hermes', platform='windows', architecture='x64', filename='hermes-windows-x64.msix', version='2026.6864.2132.0', source='https://hermes-assets.nousresearch.com/releases/tag/rc.3-v0.22.0/HermesBundled-0.22.0-win-x64.msix', install='msix', minimumOS='Windows 11 22H2+', minimumWindowsBuild=22621, expectedSha256='add268e6d6a02f7a4510e040b7513cd1c8dd4e5ff24af4e90462d962599c847c'),
    dict(app='hermes', name='Hermes', platform='macos', architecture='arm64', filename='hermes-macos-arm64.dmg', version='0.22.0', source='https://hermes-assets.nousresearch.com/releases/tag/rc.3-v0.22.0/HermesBundled-0.22.0-mac-arm64.dmg', install='dmg', expectedSha256='b560c349a006b127f9ce6e399a3ae60c12581f933b298cb991afa05ca5fc3b91'),
])
for target, version, sha in [
    ('win-x64', '0.2.1-rc.5', '50a2fedf234199e3319cdd6b89c4f0aea601a19fe48259c6e3000883f0d2c6f0'),
    ('mac-arm64', '0.2.1-rc.5', '7f387a78a7800de26af98a796c833295ebe57ee72f1f36a0810f296c871c2baf'),
    ('mac-x64', '0.2.1-rc.4', '08dcbdd520ddff62e302909c9dccc61d1b6fcdf0520cf63b2c7c5af0e15dbbfc'),
]:
    platform, arch = target.split('-')
    platform = 'windows' if platform == 'win' else 'macos'
    ext = '.exe' if platform == 'windows' else '.dmg'
    entries.append(dict(app='dsh', name='DeepSeek Harness', platform=platform, architecture=arch,
                        filename=f'dsh-{platform}-{arch}{ext}', version=version, expectedSha256=sha,
                        source=f'https://download.deepseek.com/dsh-desk/bin/{target}/deepseek-harness-{version}-{target}{ext}'))
for app, name, base, names, version in [
    ('kimi', 'Kimi Code Desktop', 'https://code.kimi.com/kimi-code/desktop/download/',
     {'windows-x64': 'KimiCode-win-x64.exe', 'macos-arm64': 'KimiCode-mac-arm64.dmg', 'macos-x64': 'KimiCode-mac-x64.dmg'}, '1.2.1'),
]:
    for target, original in names.items():
        platform, arch = target.split('-')
        entries.append(dict(app=app, name=name, platform=platform, architecture=arch,
                            filename=f'{app}-{target}{Path(original).suffix}', source=base+original, version=version))

for target, sha in {
    'win32-x64': 'e896a9d8da519353a49bf69ca4a695ba7e09067752687c675b3969041e632d52',
    'darwin-arm64': '64c327e92b2c5e0f78c0a980c5a518121e1f0d140ca771ca5dd4488d1a2dcb3f',
    'darwin-x64': '203d22fbafdab9f6b0cda962ec573e32ab970894c63b1dfb0409378f28b9c344',
}.items():
    platform, arch = target.split('-')
    exe = 'claude.exe' if platform == 'win32' else 'claude'
    entries.append(dict(app='claude', name='Claude Code', platform='windows' if platform=='win32' else 'macos',
                        architecture=arch, filename=f'claude-code-{target}-{exe}', version='2.1.316',
                        source=f'https://downloads.claude.ai/claude-code-releases/2.1.316/{target}/{exe}', expectedSha256=sha, install='native-binary'))

def fetch(entry):
    dest = root / entry['filename']
    cached_sha = None
    if dest.exists():
        with dest.open('rb') as stream:
            cached_sha = hashlib.file_digest(stream, 'sha256').hexdigest()
    if not dest.exists() or (entry.get('expectedSha256') and cached_sha != entry['expectedSha256']):
        part = dest.with_name(dest.name + '.part')
        subprocess.run(['curl', '--http1.1', '--retry-all-errors', '--fail', '--location', '--retry', '3', '--connect-timeout', '20', '--max-time', '1200',
                        '--silent', '--show-error', entry['source'], '-o', str(part)], check=True)
        with part.open('rb') as stream:
            downloaded_sha = hashlib.file_digest(stream, 'sha256').hexdigest()
        if entry.get('expectedSha256', downloaded_sha) != downloaded_sha:
            raise ValueError(f'Official checksum mismatch: {part.name}')
        part.replace(dest)
    with dest.open('rb') as stream:
        entry['sha256'] = hashlib.file_digest(stream, 'sha256').hexdigest()
    if entry.get('expectedSha256', entry['sha256']) != entry['sha256']:
        raise ValueError(f'Checksum mismatch: {dest.name}')
    entry['size'] = dest.stat().st_size
    print(json.dumps(entry, ensure_ascii=False), flush=True)
    return entry

with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
    results = list(pool.map(fetch, entries))
(root/'manifest.json').write_text(json.dumps({'schemaVersion':1,'packages':results}, ensure_ascii=False, indent=2), encoding='utf-8')
