from pathlib import Path
import json, os, subprocess, sys
KIND = 'config'
def classify(paths):
    workflow = any(p.startswith('.github/') for p in paths)
    dependencies = any(p in ('package.json', 'package-lock.json', 'pnpm-lock.yaml', 'design-system/package.json', 'design-system/pnpm-lock.yaml', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock') for p in paths)
    if KIND == 'docs':
        app = workflow or dependencies or any(p.startswith(('content/docs/', 'src/', 'scripts/', 'public/')) or p in ('source.config.ts', 'next.config.mjs', 'docs-redirects.json', 'index.md', 'tsconfig.json') for p in paths)
        deploy = workflow or any(p.startswith('deploy/') or p.startswith('docker-compose') or p == 'Dockerfile' for p in paths)
        brand = any(p.startswith(('public/brand/', 'public/logo', 'public/site.webmanifest')) for p in paths)
    elif KIND == 'design':
        app = workflow or dependencies or any(p.startswith(('design-system/apps/', 'design-system/packages/', 'design-system/scripts/', 'design-system/tests/')) or p in ('design-system/vitest.config.ts', 'design-system/pnpm-workspace.yaml') for p in paths)
        deploy = False; brand = False
    else:
        app = workflow or dependencies or any(p.startswith(('src/', 'src-tauri/', 'tests/')) for p in paths)
        deploy = False; brand = False
    return dict(app=app, dependencies=dependencies, deploy=deploy, brand=brand)
def discover():
    if len(sys.argv) > 1: return sys.argv[1:]
    try: event = json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text(encoding='utf-8'))
    except (KeyError, OSError, ValueError): return None
    if not isinstance(event, dict): return None
    pull_request = event.get('pull_request')
    if pull_request is not None:
        if not isinstance(pull_request, dict): return None
        target = pull_request.get('base')
        if not isinstance(target, dict): return None
        base = target.get('sha')
    else:
        base = event.get('before')
    if not isinstance(base, str) or len(base) not in (40, 64): return None
    if set(base) == {'0'} or any(c not in '0123456789abcdefABCDEF' for c in base): return None
    try:
        # Keep both sides of a code-to-prose rename in the affected input set.
        result = subprocess.run(['git', 'diff', '--no-renames', '--name-only', '-z', base, 'HEAD', '--'], capture_output=True, timeout=30)
    except (OSError, subprocess.TimeoutExpired): return None
    if result.returncode != 0: return None
    if not result.stdout: return []
    if not result.stdout.endswith(b'\0'): return None
    # NUL separation preserves whitespace/newlines; fsdecode preserves Git path bytes.
    paths = [os.fsdecode(path) for path in result.stdout[:-1].split(b'\0')]
    return paths if all(paths) else None
if __name__ == '__main__':
    paths = discover()
    flags = classify(paths) if paths is not None else dict(app=True, dependencies=True, deploy=True, brand=True)
    lines = ''.join(f'{key}={str(value).lower()}\n' for key, value in flags.items())
    if os.getenv('GITHUB_OUTPUT'):
        with open(os.environ['GITHUB_OUTPUT'], 'a') as output: output.write(lines)
    print(lines, end='')
