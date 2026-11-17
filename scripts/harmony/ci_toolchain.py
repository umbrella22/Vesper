#!/usr/bin/env python3
"""Package a local SDK or install a checksum-pinned SDK for a macOS arm64 CI job."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile
import tempfile


# The build toolchain excludes emulator images, device data and user-home caches.
COMPONENTS = ('LICENSE.txt', 'NOTICE.txt', 'version.txt', 'arktsdoc', 'bin',
              'codelinter', 'hstack', 'hvigor', 'ohpm', 'sdk', 'tool')
# These ceilings cover the installed 6.5 GB SDK while bounding CI disk consumption.
MAX_ARCHIVE_BYTES = 4 * 1024**3
MAX_EXPANDED_BYTES = 12 * 1024**3
MAX_ENTRIES = 150_000


def sha256(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def validate_tools(directory):
    directory = Path(directory)
    version = (directory / 'version.txt').read_text()
    sdk = json.loads((directory / 'sdk/default/openharmony/native/oh-uni-package.json').read_text())
    if ('mac-arm64' not in version or not re.search(r'^releaseType\s*:\s*release\s*$', version, re.M)
            or str(sdk['apiVersion']) != '26' or sdk['releaseType'] != 'Release'):
        raise ValueError('A stable API 26 macOS arm64 Command Line Tools installation is required')
    for relative in ['bin/hvigorw', 'bin/ohpm', 'tool/node/bin/node',
                     'sdk/default/openharmony/native/llvm/bin/clang',
                     'sdk/default/openharmony/native/llvm/bin/clang++',
                     'sdk/default/openharmony/native/llvm/bin/llvm-objcopy',
                     'sdk/default/openharmony/native/llvm/bin/llvm-strip',
                     'sdk/default/openharmony/native/llvm/bin/llvm-readelf']:
        if not os.access(directory / relative, os.X_OK):
            raise ValueError(f'Missing executable in SDK: {relative}')
    for relative in ['hvigor/hvigor/bin/hvigor.js',
                     'hvigor/hvigor-ohos-plugin/node_modules/typescript/lib/typescript.js',
                     'sdk/default/openharmony/native/llvm/lib/aarch64-linux-ohos/libc++_shared.so']:
        if not (directory / relative).is_file():
            raise ValueError(f'Missing build dependency in SDK: {relative}')
    return {'sdk': sdk, 'commandLineTools': version.strip()}


def pack_tools(directory, output):
    directory, output = Path(directory).resolve(), Path(output).resolve()
    identity = validate_tools(directory)
    if output.is_relative_to(directory):
        raise ValueError('The archive must be outside the SDK directory')
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists():
        raise FileExistsError(output)
    expanded, count = 0, 0
    try:
        with tarfile.open(output, 'w:gz', compresslevel=6, dereference=False) as archive:
            for component in COMPONENTS:
                path = directory / component
                if not path.exists():
                    raise FileNotFoundError(path)
                paths = [path] + sorted(path.rglob('*')) if path.is_dir() else [path]
                for item in paths:
                    if item.name == '.DS_Store' or item.name.startswith('._'):
                        continue
                    if not item.resolve().is_relative_to(directory):
                        raise ValueError(f'SDK link escapes the installation: {item.relative_to(directory)}')
                    entry = archive.gettarinfo(str(item), 'command-line-tools/' + str(item.relative_to(directory)))
                    if not (entry.isfile() or entry.isdir() or entry.issym()):
                        raise ValueError(f'Unsupported SDK entry: {entry.name}')
                    count += 1
                    expanded += entry.size
                    if count > MAX_ENTRIES or expanded > MAX_EXPANDED_BYTES:
                        raise ValueError('SDK exceeds the CI archive capacity')
                    # Local account names and extended attributes are not build inputs.
                    entry.uid = entry.gid = 0
                    entry.uname = entry.gname = ''
                    entry.pax_headers = {}
                    if entry.isfile():
                        with item.open('rb') as stream:
                            archive.addfile(entry, stream)
                    else:
                        archive.addfile(entry)
        if output.stat().st_size > MAX_ARCHIVE_BYTES:
            raise ValueError('Compressed SDK exceeds the CI archive capacity')
        report = {'schemaVersion': 1, 'scope': 'restricted-ci-toolchain', 'archive': output.name,
                  'sha256': sha256(output), 'bytes': output.stat().st_size,
                  'expandedBytes': expanded, 'entries': count, 'components': list(COMPONENTS), **identity}
        output.with_name(output.name + '.sha256').write_text(f'{report["sha256"]}  {output.name}\n')
        output.with_name(output.name + '.json').write_text(json.dumps(report, indent=2) + '\n')
        return report
    except BaseException:
        output.unlink(missing_ok=True)
        raise


def extract_verified(archive_path, expected_sha256, destination):
    if not re.fullmatch(r'[0-9a-fA-F]{64}', expected_sha256 or ''):
        raise ValueError('An explicit SHA-256 is required')
    if Path(archive_path).stat().st_size > MAX_ARCHIVE_BYTES:
        raise ValueError('Compressed SDK exceeds the CI archive capacity')
    if sha256(archive_path) != expected_sha256.lower():
        raise ValueError('SDK archive SHA-256 mismatch')
    destination = Path(destination)
    if destination.exists():
        raise FileExistsError(destination)
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='sdk-extract-', dir=destination.parent) as temporary:
        staging = Path(temporary).resolve()
        with tarfile.open(archive_path, 'r:gz') as archive:
            entries, names, total = [], set(), 0
            for entry in archive:
                path = PurePosixPath(entry.name)
                if (path.is_absolute() or '..' in path.parts or not path.parts
                        or path.parts[0] != 'command-line-tools' or entry.name in names
                        or not (entry.isfile() or entry.isdir() or entry.issym())):
                    raise ValueError('Invalid SDK archive entry')
                if entry.issym():
                    target = (staging / entry.name).parent / entry.linkname
                    if not target.resolve().is_relative_to(staging / 'command-line-tools'):
                        raise ValueError('SDK symlink escapes the toolchain')
                names.add(entry.name)
                entries.append(entry)
                total += entry.size
                if len(entries) > MAX_ENTRIES or total > MAX_EXPANDED_BYTES:
                    raise ValueError('SDK exceeds the CI archive capacity')
            archive.extractall(staging, members=entries, filter='data')
        identity = validate_tools(staging / 'command-line-tools')
        (staging / 'command-line-tools').rename(destination)
    return identity


def download_release(manifest_path, destination):
    manifest = json.loads(Path(manifest_path).read_text())
    repository, tag, asset = (manifest[key] for key in ['repository', 'tag', 'asset'])
    if (not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository)
            or not re.fullmatch(r'[A-Za-z0-9_.-]+', tag)
            or not re.fullmatch(r'[A-Za-z0-9_.-]+\.tar\.gz', asset)
            or not re.fullmatch(r'[0-9a-f]{64}', manifest['sha256'])
            or not 0 < manifest['bytes'] <= MAX_ARCHIVE_BYTES):
        raise ValueError('Invalid pinned GitHub toolchain manifest')
    environment = os.environ.copy()
    environment.pop('GH_DEBUG', None)
    result = subprocess.run(['gh', 'release', 'download', tag, '--repo', repository,
                             '--pattern', asset, '--dir', str(destination.parent)],
                            env=environment, capture_output=True, timeout=1200)
    if result.returncode:
        raise RuntimeError('Private SDK download failed; check HARMONY_SDK_TOKEN and release access')
    (destination.parent / asset).rename(destination)
    if destination.stat().st_size != manifest['bytes']:
        raise ValueError('Downloaded SDK size differs from the pinned manifest')
    return manifest['sha256']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    pack = commands.add_parser('pack')
    pack.add_argument('--tools', type=Path, required=True)
    pack.add_argument('--output', type=Path, required=True)
    install = commands.add_parser('install')
    source = install.add_mutually_exclusive_group(required=True)
    source.add_argument('--archive', type=Path)
    source.add_argument('--release-manifest', type=Path)
    install.add_argument('--sha256', default=os.environ.get('VESPER_HARMONY_SDK_SHA256'))
    install.add_argument('--directory', type=Path, required=True)
    options = parser.parse_args()
    if options.command == 'pack':
        report = pack_tools(options.tools, options.output)
    else:
        with tempfile.TemporaryDirectory(prefix='harmony-sdk-download-') as temporary:
            archive = options.archive or Path(temporary) / 'sdk.tar.gz'
            if options.release_manifest:
                options.sha256 = download_release(options.release_manifest, archive)
            report = {'sha256': options.sha256, **extract_verified(archive, options.sha256, options.directory)}
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
