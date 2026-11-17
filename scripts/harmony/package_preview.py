#!/usr/bin/env python3
"""Package one verified build candidate with portable example sources and symbols."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import tarfile
import tempfile
import zipfile
from candidate_identity import digest, require_candidate
from consumer_project import project_files


MODULES = ('vesper-player-kit', 'vesper-player-kit-arkui', 'vesper-player-kit-arkui-ui')


def verify_example_project(root, receipt, har_hashes):
    project = Path(receipt['projectDirectory']).resolve()
    if not project.is_relative_to((root / 'target/harmony-kit').resolve()):
        raise ValueError('Example project is outside the generated consumer staging directory')
    if not receipt.get('projectFiles') or project_files(project) != receipt['projectFiles']:
        raise ValueError('Example source changed after its consumer build')
    expected = {f'@vesper/{name.removeprefix("vesper-")}': f'file:../vendor/{name}.har' for name in MODULES}
    entry = json.loads((project / 'entry/oh-package.json5').read_text())
    package = json.loads((project / 'oh-package.json5').read_text())
    if entry['dependencies'] != expected or package['overrides'] != {
            name: path.replace('file:../', 'file:./') for name, path in expected.items()}:
        raise ValueError('Portable example must resolve all three packages from local HARs')
    for name, expected_hash in har_hashes.items():
        if digest(project / 'vendor' / name) != expected_hash:
            raise ValueError('Example vendor HAR differs from the candidate')
    return project


def verify_bundle(path):
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        if len(names) != len(set(names)) or any(Path(name).is_absolute() or '..' in Path(name).parts for name in names):
            raise ValueError('Invalid preview archive paths')
        sums = [name for name in names if Path(name).name == 'SHA256SUMS']
        if len(sums) != 1:
            raise ValueError('Expected one archive checksum inventory')
        parent = str(Path(sums[0]).parent)
        prefix = '' if parent == '.' else parent + '/'
        expected = {}
        for line in archive.read(sums[0]).decode().splitlines():
            value, relative = line.split('  ', 1)
            expected[prefix + relative] = value
        if set(expected) != set(names) - set(sums):
            raise ValueError('Preview checksum inventory omits archive entries')
        for name, value in expected.items():
            if hashlib.sha256(archive.read(name)).hexdigest() != value:
                raise ValueError(f'Preview archive checksum mismatch: {name}')


def package_preview(root, candidate_path, evidence, output):
    candidate = require_candidate(root, candidate_path)
    if candidate.get('runtimeAcceptanceReady') is not False or candidate['media']:
        raise ValueError('CI preview packaging requires an explicit build-only candidate')
    checks = json.loads((evidence / 'checks.json').read_text())
    source = candidate['source']['sourceSha256']
    if checks['status'] != 'PASS' or checks['sourceSha256'] != source:
        raise ValueError('Build checks do not match the candidate')
    required_checks = {'rust-tests', 'host-contracts', 'build-hars', 'build-core', 'build-ui',
                       'build-offline', 'build-example', 'verify-core', 'verify-ui', 'verify-offline', 'verify-example'}
    if {check['name'] for check in checks['checks'] if check['status'] == 'PASS'} != required_checks:
        raise ValueError('Required CI artifact checks did not all pass')
    dist = root / 'target/harmony-kit/dist'
    har_hashes = {f'{name}.har': digest(dist / f'{name}.har') for name in MODULES}
    receipt = json.loads((dist / 'vesper-harmony-example-unsigned.build.json').read_text())
    project = verify_example_project(root, receipt, har_hashes)
    with tarfile.open(dist / 'vesper-player-kit.har', 'r:gz') as archive:
        version = json.load(archive.extractfile('package/oh-package.json5'))['version']
        notice = archive.extractfile('package/NOTICE-libcxx.txt').read()
    name = f'vesper-harmony-{version}-{source[:8]}'
    output.mkdir(parents=True, exist_ok=True)
    main_zip, symbols_zip = output / f'{name}.zip', output / f'{name}-symbols.zip'
    if main_zip.exists() or symbols_zip.exists():
        raise FileExistsError('A package already exists for this source identity')
    with tempfile.TemporaryDirectory(prefix='preview-package-', dir=output) as temporary:
        staging = Path(temporary) / name
        staging.mkdir()

        def copy(path, relative):
            destination = staging / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, destination)

        for module in MODULES:
            copy(dist / f'{module}.har', f'hars/{module}.har')
        copy(dist / 'vesper-harmony-example-unsigned.hap', 'example/vesper-harmony-example-unsigned.hap')
        for relative in receipt['projectFiles']:
            copy(project / relative, 'example-source/' + relative)
        copy(root / 'LICENSE', 'LICENSE')
        (staging / 'NOTICE-libcxx.txt').write_bytes(notice)
        copy(root / 'lib/harmony/TESTING.md', 'TESTING.md')
        copy(root / 'lib/harmony/vesper-player-kit/diagnostic-feedback.md', 'vesper-player-kit/diagnostic-feedback.md')
        feedback = staging / 'vesper-player-kit/diagnostic-feedback.md'
        feedback.write_text(feedback.read_text().replace('../../../CURRENT-CHECKLIST.md', '../TESTING.md'))
        copy(candidate_path, 'evidence/candidate.json')
        for relative in candidate['artifacts']:
            path = root / relative
            if path.suffix == '.json':
                copy(path, 'evidence/' + path.name)
        copy(evidence / 'checks.json', 'evidence/checks.json')
        for check in checks['checks']:
            copy(evidence / check['log'], 'evidence/' + check['log'])
        identity = {'schemaVersion': 1, 'version': version, 'gitHead': candidate['source']['gitHead'],
                    'sourceSha256': source, 'scope': 'unsigned-harmony-ci-artifacts',
                    'checks': 'PASS', 'runtimeTests': 'NOT RUN', 'physicalDevices': 'NOT RUN',
                    'productionSigning': 'NOT RUN', 'ohpmPublished': False,
                    'sdkArchiveSha256': checks['sdkArchiveSha256'], 'workflow': checks['workflow']}
        (staging / 'build-identity.json').write_text(json.dumps(identity, indent=2) + '\n')
        (staging / 'README.md').write_text(f'''# Vesper HarmonyOS preview {version}

Source identity: `{source}`. Build/host checks passed for this candidate.
Runtime and physical-device tests have not been run by the artifact job.

- `hars/`: core, ArkUI binding and optional controls HARs.
- `example/`: unsigned Release example HAP.
- `example-source/`: the matching consumer with three vendored local HARs.
- `evidence/`: build checks, candidate identity and artifact receipts.
- `build-identity.json`: source, toolchain and workflow identifiers.

Open `example-source/` in DevEco Studio with the stable API 26 SDK. Configure
device signing to run on a phone or tablet. The application targets API 20+
arm64 devices. [Testing and feedback](TESTING.md) covers installation, local HAR
integration, diagnostic reports and known capability limits.

The SDK uses system Media Kit AVPlayer and a native Surface. OHPM publication
is deferred. Debug symbols are provided in the separate symbols archive.
Vesper source retains its Apache-2.0 license; the C++ runtime retains its notice.
''')
        hashes = {str(path.relative_to(staging)): digest(path) for path in sorted(staging.rglob('*')) if path.is_file()}
        (staging / 'SHA256SUMS').write_text(''.join(f'{value}  {path}\n' for path, value in hashes.items()))
        with zipfile.ZipFile(main_zip, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
            for path in sorted(staging.rglob('*')):
                if path.is_file():
                    archive.write(path, str(path.relative_to(staging.parent)))
    symbols = {Path(path).name: value for path, value in candidate['artifacts'].items()
               if path.startswith('target/harmony-kit/symbols/')}
    if len(symbols) != 2:
        raise ValueError('Expected exactly two native symbol files')
    symbols_json = json.dumps({'sourceSha256': source, 'files': symbols}, indent=2) + '\n'
    with zipfile.ZipFile(symbols_zip, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
        for filename in symbols:
            archive.write(root / 'target/harmony-kit/symbols' / filename, filename)
        archive.writestr('SYMBOLS.json', symbols_json)
        symbol_hashes = {**symbols, 'SYMBOLS.json': hashlib.sha256(symbols_json.encode()).hexdigest()}
        archive.writestr('SHA256SUMS', ''.join(f'{value}  {path}\n' for path, value in symbol_hashes.items()))
    require_candidate(root, candidate_path)
    verify_bundle(main_zip)
    verify_bundle(symbols_zip)
    result = {'schemaVersion': 1, 'status': 'PASS', 'sourceSha256': source,
              'scope': 'verified-build-artifact-bundle', 'runtimeTests': 'NOT RUN',
              'mainArchive': {'path': str(main_zip), 'sha256': digest(main_zip)},
              'symbolsArchive': {'path': str(symbols_zip), 'sha256': digest(symbols_zip)}}
    (output / 'SHA256SUMS').write_text(''.join(f'{result[key]["sha256"]}  {Path(result[key]["path"]).name}\n'
                                             for key in ['mainArchive', 'symbolsArchive']))
    (output / f'{name}.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--candidate', type=Path, default=Path('target/harmony-kit/dist/candidate.json'))
    parser.add_argument('--evidence', type=Path, default=Path('target/harmony-kit/ci-evidence'))
    parser.add_argument('--output', type=Path, default=Path('target/harmony-kit/release'))
    options = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    print(json.dumps(package_preview(root, options.candidate, options.evidence, options.output), indent=2))


if __name__ == '__main__':
    main()
