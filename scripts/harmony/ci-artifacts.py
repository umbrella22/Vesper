#!/usr/bin/env python3
"""Run the same host checks, HAR consumers and build-only packaging locally or in CI."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time
from candidate_identity import source_identity
from ci_toolchain import validate_tools
from package_preview import package_preview


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tools', type=Path, required=True)
    parser.add_argument('--sdk-archive-sha256')
    options = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    tools = options.tools.resolve()
    validate_tools(tools)
    source = source_identity(root)
    evidence = root / 'target/harmony-kit/ci-evidence'
    evidence.mkdir(parents=True, exist_ok=True)
    report = {'schemaVersion': 1, 'scope': 'harmony-ci-artifact-build', 'status': 'RUNNING',
              'sourceSha256': source['sourceSha256'], 'checks': [], 'runtimeTests': 'NOT RUN',
              'sdkArchiveSha256': options.sdk_archive_sha256 or 'not-supplied',
              'workflow': {key: os.environ.get(key, '') for key in
                           ['GITHUB_REPOSITORY', 'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT', 'GITHUB_SHA', 'GITHUB_EVENT_NAME']}}

    def persist():
        (evidence / 'checks.json').write_text(json.dumps(report, indent=2) + '\n')

    def run(name, command, timeout=1200):
        started = time.monotonic()
        log = evidence / f'{name}.log'
        print(f'Running {name}', flush=True)
        with log.open('w') as output:
            try:
                result = subprocess.run([str(item) for item in command], cwd=root,
                                        stdout=output, stderr=subprocess.STDOUT, timeout=timeout)
                status = 'PASS' if result.returncode == 0 else 'FAIL'
            except subprocess.TimeoutExpired:
                status = 'TIMEOUT'
        report['checks'].append({'name': name, 'status': status, 'log': log.name,
                                 'durationSeconds': round(time.monotonic() - started, 3)})
        if status != 'PASS':
            report['status'] = 'FAIL'
            persist()
            print(log.read_text()[-6000:], file=sys.stderr)
            raise RuntimeError(f'{name}: {status}; inspect {log}')
        persist()

    scripts = root / 'scripts/harmony'
    run('rust-tests', ['cargo', 'test', '--locked', '--offline', '-p', 'player-download',
                       '-p', 'player-platform-harmony', '-p', 'player-ffi-harmony'])
    run('host-contracts', [sys.executable, scripts / 'host-contracts.py', '--tools', tools])
    run('build-hars', [sys.executable, scripts / 'build-kit.py', '--tools', tools, '--offline'])
    dist = root / 'target/harmony-kit/dist'
    consumers = [('core', [], 'kit-consumer'), ('ui', ['--ui'], 'ui-consumer'),
                 ('offline', ['--no-internet'], 'kit-consumer-offline'), ('example', ['--example'], 'example')]
    for name, flags, filename in consumers:
        run('build-' + name, [sys.executable, scripts / 'build-consumer.py', '--tools', tools,
                             '--har', dist / 'vesper-player-kit.har', *flags])
        run('verify-' + name, [sys.executable, scripts / 'verify-artifacts.py', '--directory', dist,
                              '--hap', dist / f'vesper-harmony-{filename}-unsigned.hap',
                              *(['--no-internet'] if name == 'offline' else [])])
    if source_identity(root)['sourceSha256'] != source['sourceSha256']:
        raise ValueError('Source changed while CI artifact checks were running')
    subprocess.run([sys.executable, scripts / 'freeze-candidate.py', '--tools', str(tools), '--without-media'],
                   cwd=root, check=True, timeout=60)
    report['status'] = 'PASS'
    persist()
    print(json.dumps(package_preview(root, dist / 'candidate.json', evidence, root / 'target/harmony-kit/release'), indent=2))


if __name__ == '__main__':
    main()
