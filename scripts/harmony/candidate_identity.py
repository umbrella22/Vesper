"""Fingerprint the working source tree without assuming a clean Git checkout."""
import hashlib
import json
from pathlib import Path
import subprocess


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_identity(root):
    # Editor indexes are protected separately and are not build inputs.
    excluded = {'target', 'build', 'oh_modules', 'node_modules', '.hvigor', '.cxx', '__pycache__', 'libs', '.git',
                '.codeartsdoer', '.merkle-snapshot.json'}
    extensions = {'.rs', '.toml', '.ets', '.ts', '.cpp', '.h', '.json', '.json5', '.py', '.cjs', '.md', '.txt', '.yml', '.svg', '.m4v'}
    paths = [root / name for name in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'LICENSE',
             'THIRD_PARTY_NOTICES.md', 'include/vesper_harmony.h', '.github/workflows/harmony-ci.yml']]
    for directory in ['crates', 'lib/harmony', 'scripts/harmony', 'fixtures/harmony', 'examples/harmony-arkui-host']:
        paths.extend(path for path in (root / directory).rglob('*') if path.is_file()
                     and (path.suffix in extensions or 'examples/harmony-arkui-host/entry/src/main/resources' in str(path.relative_to(root)))
                     and not any(part in excluded for part in path.relative_to(root).parts))
    paths.append(root / 'fixtures/media/tiny-h264-aac-mediacodec.m4v')
    files = {str(path.relative_to(root)): digest(path) for path in sorted(set(paths)) if path.is_file()}
    canonical = json.dumps(files, sort_keys=True, separators=(',', ':')).encode()
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True, timeout=10).strip()
    return {'gitHead': head, 'sourceSha256': hashlib.sha256(canonical).hexdigest(), 'files': files,
            'scope': 'working-tree-build-inputs-including-uncommitted-files'}


def require_candidate(root, path):
    candidate = json.loads(Path(path).read_text())
    if candidate['source']['sourceSha256'] != source_identity(root)['sourceSha256']:
        raise ValueError('Working source changed after the candidate was frozen')
    for relative, expected in candidate['artifacts'].items():
        if digest(root / relative) != expected:
            raise ValueError(f'Frozen artifact changed: {relative}')
    for relative, expected in candidate['media'].items():
        if digest(root / relative) != expected:
            raise ValueError(f'Frozen media changed: {relative}')
    return candidate


def require_runtime_candidate(root, path):
    candidate = require_candidate(root, path)
    if (candidate.get('runtimeAcceptanceReady') is False
            or 'target/harmony-kit/media/catalog.json' not in candidate['media']):
        raise ValueError('Build-only candidates cannot run final runtime acceptance; prepare and freeze media first')
    return candidate
