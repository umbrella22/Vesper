#!/usr/bin/env python3
"""Bind source, toolchain, media and staged artifacts before final acceptance."""
import argparse
import json
from pathlib import Path
import subprocess
from candidate_identity import source_identity, digest

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--tools', type=Path, required=True)
parser.add_argument('--output', type=Path, default=Path('target/harmony-kit/dist/candidate.json'))
parser.add_argument('--without-media', action='store_true', help='Freeze build artifacts without permitting runtime acceptance')
options = parser.parse_args()
root = Path(__file__).resolve().parents[2]
source = source_identity(root)
build = json.loads((root / 'target/harmony-kit/dist/build-report.json').read_text())
if build['source']['sourceSha256'] != source['sourceSha256']:
    raise ValueError('Rebuild HARs from the final source before freezing a candidate')
dist = root / 'target/harmony-kit/dist'
modules = ['vesper-player-kit', 'vesper-player-kit-arkui', 'vesper-player-kit-arkui-ui']
har_hashes = {f'{name}.har': digest(dist / f'{name}.har') for name in modules}
for name in modules:
    if har_hashes[f'{name}.har'] != build['artifacts'][name]['sha256']:
        raise ValueError(f'HAR changed after its build: {name}')
haps = ['vesper-harmony-kit-consumer-unsigned.hap', 'vesper-harmony-kit-consumer-offline-unsigned.hap',
        'vesper-harmony-ui-consumer-unsigned.hap', 'vesper-harmony-example-unsigned.hap']
for name in haps:
    hap = dist / name
    receipt = json.loads(hap.with_suffix('.build.json').read_text())
    expected = har_hashes if 'ui-' in name or 'example-' in name else {'vesper-player-kit.har': har_hashes['vesper-player-kit.har']}
    if (receipt['sourceSha256'] != source['sourceSha256'] or receipt['hapSha256'] != digest(hap)
            or receipt['harSha256'] != expected):
        raise ValueError(f'Consumer was not built from the frozen source/HARs: {name}')
required = list(har_hashes) + haps + ['build-report.json'] + [str(Path(name).with_suffix('.build.json')) for name in haps]
artifacts = {str((dist / name).relative_to(root)): digest(dist / name) for name in required}
for path in sorted((root / 'target/harmony-kit/symbols').glob('*.debug')):
    artifacts[str(path.relative_to(root))] = digest(path)
if len(list((root / 'target/harmony-kit/symbols').glob('*.debug'))) != 2:
    raise ValueError('The two separated native debug symbols are required')
media_root = root / 'target/harmony-kit/media'
media = {} if options.without_media else {str(path.relative_to(root)): digest(path) for path in sorted(media_root.rglob('*')) if path.is_file()}
if not options.without_media and (not (media_root / 'catalog.json').is_file() or len(media) < 10):
    raise ValueError('Prepare the complete media fixture before freezing the candidate')
tools = options.tools.resolve()
sdk = json.loads((tools / 'sdk/default/openharmony/native/oh-uni-package.json').read_text())
report = {'schemaVersion': 1, 'scope': 'unsigned-harmony-build-candidate' if options.without_media else 'local-unsigned-harmony-preview-candidate',
          'runtimeAcceptanceReady': not options.without_media, 'source': source,
          'artifacts': artifacts, 'media': media, 'sdk': sdk,
          'rustc': subprocess.check_output(['rustc', '-Vv'], cwd=root, text=True, timeout=15).strip(),
          'cbindgen': subprocess.check_output(['cbindgen', '--version'], text=True, timeout=15).strip(),
          'toolsDirectory': str(tools), 'published': False}
options.output.parent.mkdir(parents=True, exist_ok=True)
options.output.write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({'candidate': str(options.output.resolve()), 'sourceSha256': source['sourceSha256'], 'artifacts': len(artifacts)}))
