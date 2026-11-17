#!/usr/bin/env python3
"""Run one official-emulator consumer scenario and retain artifact-bound evidence."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time


def run(arguments, timeout=30):
    result = subprocess.run([str(a) for a in arguments], capture_output=True,
                            text=True, timeout=timeout)
    if result.returncode or '[Fail]' in result.stdout:
        raise RuntimeError(result.stderr or result.stdout)
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tools', type=Path, required=True)
    parser.add_argument('--instance', required=True)
    parser.add_argument('--device', required=True)
    parser.add_argument('--hap', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--capture-only', action='store_true')
    parser.add_argument('--button', default='Run kit checks')
    parser.add_argument('--bundle', default='dev.vesper.harmony.kitconsumer')
    parser.add_argument('--timeout-seconds', type=int, default=300)
    options = parser.parse_args()
    if not 15 <= options.timeout_seconds <= 600:
        parser.error('--timeout-seconds must be between 15 and 600')
    hdc = [options.tools / 'sdk/default/openharmony/toolchains/hdc', '-t', options.device]
    emulator = [options.tools / 'bin/Emulator', '-instance', options.instance,
                '-instancePath', options.tools / 'emulator-instances']
    if not options.capture_only:
        for port in [18765, 7777]:
            # Existing reverse forwards can return a nonzero code on some SDKs.
            subprocess.run([str(a) for a in hdc + ['rport', f'tcp:{port}', f'tcp:{port}']],
                           capture_output=True, timeout=15)
        run(hdc + ['install', '-r', options.hap])
        run(hdc + ['shell', 'aa', 'force-stop', options.bundle])
        run(hdc + ['shell', 'aa', 'start', '-a', 'EntryAbility', '-b', options.bundle])
        started = time.monotonic()
        while True:
            layout = run(emulator + ['-uiLayout'])
            button = re.search(r'Button ' + re.escape(options.button)
                               + r' \[id:(\d+)\] \[top: (-?\d+), left: (-?\d+), width: (\d+), height: (\d+)\]', layout)
            if button:
                top, left, width, height = map(int, button.groups()[1:])
                # The bottom diagnostic row can overlap the system gesture strip.
                # Use its observed upper interior rather than a centered SDK click.
                run(hdc + ['shell', 'uitest', 'uiInput', 'click',
                           str(left + width // 2), str(top + max(1, height // 4))])
                break
            if time.monotonic() - started > 20:
                raise RuntimeError('Consumer scenario button did not appear')
            time.sleep(0.5)
    started = time.monotonic()
    while True:
        layout = run(emulator + ['-uiLayout'])
        match = re.search(r'- Text (\{.*\}) \[id:', layout)
        if match:
            result = json.loads(match.group(1))
            break
        if time.monotonic() - started > options.timeout_seconds:
            raise RuntimeError('Consumer did not produce a terminal JSON result within the scenario deadline')
        time.sleep(1)
    options.output.parent.mkdir(parents=True, exist_ok=True)
    options.output.with_suffix('.ui.txt').write_text(layout)
    report = {
        'schemaVersion': 1, 'scope': 'official-harmony-emulator-single-scenario',
        'hap': str(options.hap.resolve()),
        'hapSha256': hashlib.sha256(options.hap.read_bytes()).hexdigest(),
        'instance': options.instance, 'device': options.device,
        'runtimeApi': run(hdc + ['shell', 'param', 'get', 'const.ohos.apiversion']).strip(),
        'runtimeVersion': run(hdc + ['shell', 'param', 'get', 'const.ohos.fullname']).strip(),
        'result': result,
        'unmeasured': ['Physical devices', 'Audible output', 'Hardware decoder identity', 'HDR', 'Power'],
    }
    options.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'status': result['status'], 'report': str(options.output)}, indent=2))
    return 0 if result['status'] == 'PASS' else 1


if __name__ == '__main__':
    raise SystemExit(main())
