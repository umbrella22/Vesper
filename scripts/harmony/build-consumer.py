#!/usr/bin/env python3
"""Build a Release HAP using staged HARs, without building Rust or library sources."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from candidate_identity import source_identity
from consumer_project import project_files

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--tools', type=Path, required=True)
parser.add_argument('--har', type=Path, required=True)
parser.add_argument('--ui', action='store_true', help='Consume all three sibling HARs and the Stage fixture')
parser.add_argument('--no-internet', action='store_true', help='Build an offline consumer without INTERNET permission')
parser.add_argument('--example', action='store_true', help='Build the standalone ArkUI example from staged HARs')
options = parser.parse_args()
if options.example:
    options.ui = True
root = Path(__file__).resolve().parents[2]
base = root / 'target/harmony-kit'
base.mkdir(parents=True, exist_ok=True)
project = Path(tempfile.mkdtemp(prefix='consumer-', dir=base))
tools = options.tools.resolve()
template = root / ('examples/harmony-arkui-host' if options.example else 'fixtures/harmony/kit-consumer')
files = ['build-profile.json5', 'hvigorfile.ts', 'oh-package.json5', 'hvigor/hvigor-config.json5',
    'AppScope/app.json5', 'AppScope/resources/base/media/app_icon.svg', 'AppScope/resources/base/element/string.json',
    'entry/build-profile.json5', 'entry/hvigorfile.ts', 'entry/src/main/module.json5',
    'entry/src/main/resources/base/profile/main_pages.json', 'entry/src/main/resources/base/element/color.json',
    'entry/src/main/ets/entryability/EntryAbility.ets']
for name in files:
    destination = project / name
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(template / name, destination)
shutil.copytree(template / 'AppScope/resources', project / 'AppScope/resources', dirs_exist_ok=True)
fixture = root / ('examples/harmony-arkui-host' if options.example else 'fixtures/harmony/ui-consumer' if options.ui else 'fixtures/harmony/kit-consumer')
identity = source_identity(root)
build_report = json.loads((options.har.parent / 'build-report.json').read_text())
if 'source' in build_report and build_report['source']['sourceSha256'] != identity['sourceSha256']:
    raise ValueError('Consumer sources differ from the HAR candidate; rebuild HARs before consumers')
shutil.copytree(fixture / 'entry/src/main/ets', project / 'entry/src/main/ets', dirs_exist_ok=True)
resources = fixture / 'entry/src/main/resources'
if resources.is_dir():
    shutil.copytree(resources, project / 'entry/src/main/resources', dirs_exist_ok=True)
app_path = project / 'AppScope/app.json5'
app = json.loads(app_path.read_text())
app['app']['bundleName'] = 'dev.vesper.harmony.example' if options.example else 'dev.vesper.harmony.uiconsumer' if options.ui else 'dev.vesper.harmony.kitconsumer'
app_path.write_text(json.dumps(app))
module_path = project / 'entry/src/main/module.json5'
module = json.loads(module_path.read_text())
if not any(permission['name'] == 'ohos.permission.KEEP_BACKGROUND_RUNNING' for permission in module['module']['requestPermissions']):
    module['module']['requestPermissions'].append({'name': 'ohos.permission.KEEP_BACKGROUND_RUNNING'})
module['module']['abilities'][0]['backgroundModes'] = ['audioPlayback']
if options.ui:
    module['module']['abilities'][0]['orientation'] = 'auto_rotation_restricted'
    module['module']['abilities'][0]['supportWindowMode'] = ['fullscreen', 'split', 'floating']
if options.no_internet:
    module['module']['requestPermissions'] = [permission for permission in module['module']['requestPermissions'] if permission['name'] != 'ohos.permission.INTERNET']
module_path.write_text(json.dumps(module))
(project / 'vendor').mkdir()
shutil.copy2(options.har, project / 'vendor/vesper-player-kit.har')
dependencies = {'@vesper/player-kit': 'file:../vendor/vesper-player-kit.har'}
if options.ui:
    for name, package in [('vesper-player-kit-arkui', '@vesper/player-kit-arkui'), ('vesper-player-kit-arkui-ui', '@vesper/player-kit-arkui-ui')]:
        shutil.copy2(options.har.parent / f'{name}.har', project / f'vendor/{name}.har')
        dependencies[package] = f'file:../vendor/{name}.har'
    root_manifest = project / 'oh-package.json5'
    root_package = json.loads(root_manifest.read_text())
    root_package['overrides'] = {package: version.replace('file:../', 'file:./') for package, version in dependencies.items()}
    root_manifest.write_text(json.dumps(root_package))
(project / 'entry/oh-package.json5').write_text(json.dumps({'name': 'entry', 'version': '0.9.1',
    'dependencies': dependencies}))
raw = project / 'entry/src/main/resources/rawfile'
raw.mkdir(parents=True, exist_ok=True)
if not options.example:
    shutil.copy2(root / 'fixtures/media/tiny-h264-aac-mediacodec.m4v', raw / 'probe.mp4')
(project / 'node_modules/@ohos').mkdir(parents=True)
for name in ['hvigor', 'hvigor-ohos-plugin']:
    (project / 'node_modules/@ohos' / name).symlink_to(tools / 'hvigor' / name, target_is_directory=True)
environment = os.environ.copy()
environment.update({'NODE_HOME': str(tools / 'tool/node'), 'DEVECO_NODE_HOME': str(tools / 'tool/node'), 'DEVECO_SDK_HOME': str(tools / 'sdk')})
log = project / 'build.log'
with log.open('w') as output:
    for arguments in [[tools / 'bin/ohpm', 'install', '--all'],
            [tools / 'bin/hvigorw', '--mode', 'module', '-p', 'module=entry@default', '-p', 'product=default',
             '-p', 'buildMode=release', 'assembleHap', '--no-daemon']]:
        result = subprocess.run([str(argument) for argument in arguments], cwd=project, env=environment,
                                stdout=output, stderr=subprocess.STDOUT, timeout=300)
        if result.returncode:
            raise RuntimeError(f'Consumer build failed; inspect {log}')
candidates = list((project / 'entry/build/default/outputs/default').glob('*unsigned.hap'))
if len(candidates) != 1:
    raise RuntimeError('Expected one unsigned Release HAP')
artifact = base / ('dist/vesper-harmony-example-unsigned.hap' if options.example else 'dist/vesper-harmony-' + ('ui' if options.ui else 'kit') + '-consumer' + ('-offline' if options.no_internet else '') + '-unsigned.hap')
shutil.copy2(candidates[0], artifact)
if source_identity(root)['sourceSha256'] != identity['sourceSha256']:
    raise ValueError('Source changed during the consumer build')
artifact.with_suffix('.build.json').write_text(json.dumps({'sourceSha256': identity['sourceSha256'], 'hapSha256': hashlib.sha256(artifact.read_bytes()).hexdigest(),
    'projectDirectory': str(project), 'projectFiles': project_files(project),
    'harSha256': {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in (project / 'vendor').glob('*.har')}}, indent=2) + '\n')
print(json.dumps({'status': 'PASS', 'scope': 'production-har-only-consumer-build', 'projectDirectory': str(project),
    'hap': str(artifact), 'hapSha256': hashlib.sha256(artifact.read_bytes()).hexdigest(),
    'coreHarSha256': hashlib.sha256(options.har.read_bytes()).hexdigest(),
    'unverified': ['Runtime execution', 'Physical devices', 'Registry package consumption']}, indent=2))
