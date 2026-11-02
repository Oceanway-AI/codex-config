"""Assemble verified full agent packages into a versioned download directory."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def publish(windows, macos, output, additional=None):
    entries = json.loads((windows / 'manifest.json').read_text())['packages']
    for entry in entries:
        entry.update(platform='windows', minimumOS='Windows 10 2004+', minimumWindowsBuild=19041)
        entry['_source'] = windows / entry['filename']
    for arch in ('arm64', 'x64'):
        folder = macos / f'agent-offline-macos-{arch}'
        checksums = dict((name.strip(), sha) for sha, name in
                         (line.split(None, 1) for line in (folder / 'SHA256SUMS.txt').read_text().splitlines()))
        filename = f'codex-macos-{arch}.dmg'
        entries.append(dict(app='codex', name='Codex 桌面版', platform='macos',
                            architecture=arch, minimumOS='macOS 13+', version='26.1027.61615',
                            filename=filename, sha256=checksums[filename],
                            source=f'https://persistent.oaistatic.com/codex-app-prod/ChatGPT-darwin-{arch}-26.1027.61615.zip',
                            documentation='https://developers.openai.com/codex/app',
                            verification='codesign --verify --deep --strict; spctl accepted: Notarized Developer ID',
                            _source=folder / filename))
        if arch == 'arm64':
            filename = 'claude-desktop-macos-universal.pkg'
            entries.append(dict(app='claude-desktop', name='Claude Desktop', platform='macos',
                                architecture='universal', version='4.4212.0', filename=filename,
                                sha256=checksums[filename],
                                source='https://downloads.claude.ai/releases/darwin/universal/4.4212.0/Claude-cbfe3c56d49c49332ad5a9588303c9dbc70a5e7f.pkg',
                                documentation='https://support.claude.com/en/articles/12611117-deploy-claude-desktop-for-macos',
                                verification='pkgutil trusted Anthropic PBC; spctl accepted: Notarized Developer ID',
                                _source=folder / filename))
    if additional:
        for entry in json.loads((additional / 'manifest.json').read_text())['packages']:
            entry['_source'] = additional / entry['filename']
            entries.append(entry)
    output.mkdir(parents=True, exist_ok=False)
    for entry in entries:
        source = entry.pop('_source')
        if digest(source) != entry['sha256']:
            raise ValueError(f'Checksum mismatch: {source.name}')
        target = output / entry['filename']
        shutil.copyfile(source, target)
        if digest(target) != entry['sha256']:
            raise ValueError(f'Published checksum mismatch: {target.name}')
        entry['size'] = target.stat().st_size
    (output / 'manifest.json').write_text(json.dumps(dict(schemaVersion=1, packages=entries), ensure_ascii=False, indent=2), encoding='utf-8')
    (output / 'SHA256SUMS.txt').write_text(''.join(f"{item['sha256']}  {item['filename']}\n" for item in entries), encoding='ascii')
    shutil.copyfile(Path(__file__).with_name('index.html'), output / 'index.html')
    print(json.dumps([{'filename': e['filename'], 'size': e['size'], 'sha256': e['sha256']} for e in entries]))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--windows', type=Path, required=True)
    parser.add_argument('--macos', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--additional', type=Path)
    args = parser.parse_args()
    publish(args.windows, args.macos, args.output, args.additional)
