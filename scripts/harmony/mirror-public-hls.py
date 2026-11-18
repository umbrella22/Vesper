#!/usr/bin/env python3
"""Mirror one complete unencrypted public HLS rendition for bounded EOF comparisons."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import urllib.request
from urllib.parse import urljoin, urlsplit

DEFAULT = 'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--url', default=DEFAULT)
    options = parser.parse_args()
    root = options.root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    records = []
    retained = {}
    total = 0

    def fetch(uri):
        nonlocal total
        if urlsplit(uri).scheme != 'https':
            raise ValueError('The public mirror requires HTTPS')
        with urllib.request.urlopen(uri, timeout=30) as response:
            data = response.read(4 * 1024 * 1024 + 1)
            if len(data) > 4 * 1024 * 1024 or total + len(data) > 128 * 1024 * 1024:
                raise ValueError('Public mirror exceeds its 4 MiB resource or 128 MiB total budget')
            if response.status != 200 or response.headers.get('Content-Length') is not None and int(response.headers['Content-Length']) != len(data):
                raise ValueError('Public fixture response was incomplete')
            total += len(data)
            records.append({'publicUrl': uri, 'status': response.status, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()})
            return data

    master = fetch(options.url).decode()
    variants = []
    lines = master.splitlines()
    for index, line in enumerate(lines):
        if line.startswith('#EXT-X-STREAM-INF:') and ('RESOLUTION=' in line or 'avc1' in line):
            variants.append((int(re.search(r'(?:^|,)BANDWIDTH=(\d+)', line.split(':', 1)[1])[1]), line, lines[index + 1]))
    if not variants:
        raise ValueError('Public master has no AVC video rendition')
    _, variant, uri = min(variants)
    audio = re.search(r'(?:^|,)AUDIO="([^"]+)"', variant.split(':', 1)[1])
    media = [line for line in lines if line.startswith('#EXT-X-MEDIA:') and audio is not None
             and f'GROUP-ID="{audio[1]}"' in line and 'TYPE=AUDIO' in line]
    if media:
        media = [next((line for line in media if 'DEFAULT=YES' in line), media[0])]

    def mirror(uri, depth=0):
        if uri in retained:
            return retained[uri]
        if depth > 4 or len(retained) >= 1536:
            raise ValueError('Public HLS mirror recursion or resource limit')
        extension = Path(urlsplit(uri).path).suffix
        if extension not in ['.m3u8', '.ts', '.aac', '.mp4', '.m4s']:
            raise ValueError('Unsupported public fixture resource extension')
        name = 'resource-' + hashlib.sha256(uri.encode()).hexdigest()[:24] + extension
        retained[uri] = name
        data = fetch(uri)
        if extension == '.m3u8':
            text = data.decode()
            if '#EXT-X-ENDLIST' not in text or '#EXT-X-STREAM-INF' in text:
                raise ValueError('Expected a finite complete media playlist')
            rewritten = []
            for line in text.splitlines():
                if line.startswith('#EXT-X-KEY:') and 'METHOD=NONE' not in line:
                    raise ValueError('Encrypted public media is outside this fixture')
                if line and not line.startswith('#'):
                    line = mirror(urljoin(uri, line), depth + 1)
                elif 'URI="' in line:
                    line = re.sub(r'URI="([^"]+)"', lambda match: 'URI="' + mirror(urljoin(uri, match[1]), depth + 1) + '"', line)
                rewritten.append(line)
            data = ('\n'.join(rewritten) + '\n').encode()
        (root / name).write_bytes(data)
        return name

    output = ['#EXTM3U', '#EXT-X-VERSION:7']
    for line in media:
        output.append(re.sub(r'URI="([^"]+)"', lambda match: 'URI="' + mirror(urljoin(options.url, match[1])) + '"', line))
    output.extend([variant, mirror(urljoin(options.url, uri))])
    (root / 'master.m3u8').write_text('\n'.join(output) + '\n')
    identity = {'scope': 'complete-lowest-avc-rendition-and-default-associated-audio', 'sourceMaster': options.url,
                'selectedVideoPlaylist': urljoin(options.url, uri), 'bytes': total, 'resources': records,
                'localSha256': {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in root.iterdir() if path.is_file() and path.name != 'identity.json'}}
    (root / 'identity.json').write_text(json.dumps(identity, indent=2) + '\n')
    print(json.dumps({'status': 'PASS', 'files': len(retained), 'bytes': total}))


if __name__ == '__main__':
    main()
