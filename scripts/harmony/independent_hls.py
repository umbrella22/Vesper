#!/usr/bin/env python3
"""Generate bounded FFmpeg HLS controls independently of Vesper's DASH bridge."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess


def run(arguments):
    subprocess.run(arguments, check=True, capture_output=True, timeout=90)


def prepare(root):
    directory = root / 'independent'
    directory.mkdir(parents=True, exist_ok=True)
    source = directory / 'source.mp4'
    run(['ffmpeg', '-y', '-v', 'error', '-f', 'lavfi', '-i', 'testsrc2=size=320x180:rate=24',
         '-f', 'lavfi', '-i', 'sine=frequency=660:sample_rate=48000', '-t', '300',
         '-c:v', 'libx264', '-preset', 'ultrafast', '-profile:v', 'baseline', '-level:v', '3.0',
         '-pix_fmt', 'yuv420p', '-g', '48', '-keyint_min', '48', '-sc_threshold', '0', '-bf', '0',
         '-c:a', 'aac', '-movflags', '+faststart', str(source)])
    common = ['ffmpeg', '-y', '-v', 'error', '-i', str(source), '-map', '0:v:0', '-map', '0:a:0',
              '-c', 'copy', '-f', 'hls', '-hls_time', '2', '-hls_list_size', '0', '-hls_flags', 'independent_segments']
    for name, kind in [('ts', 'mpegts'), ('fmp4-muxed', 'fmp4'), ('fmp4-separate', 'fmp4')]:
        destination = directory / name
        destination.mkdir(exist_ok=True)
        arguments = common + ['-hls_segment_type', kind]
        if name == 'fmp4-separate':
            arguments += ['-var_stream_map', 'v:0,agroup:audio a:0,agroup:audio,default:yes,language:eng',
                          '-master_pl_name', 'master.m3u8', '-hls_fmp4_init_filename', 'init-%v.mp4',
                          '-hls_segment_filename', str(destination / 'segment-%v-%03d.m4s'), str(destination / 'variant-%v.m3u8')]
            run(arguments)
            for index in [0, 1]:
                (destination / f'live-{index}.m3u8').write_text('#EXTM3U\n')
        else:
            suffix = 'ts' if kind == 'mpegts' else 'm4s'
            if kind == 'fmp4':
                arguments += ['-hls_fmp4_init_filename', 'init.mp4']
            run(arguments + ['-hls_segment_filename', str(destination / f'segment-%03d.{suffix}'), str(destination / 'vod.m3u8')])
            (destination / 'master.m3u8').write_text('#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-STREAM-INF:BANDWIDTH=1200000,CODECS="avc1.42c01e,mp4a.40.2"\nvod.m3u8\n')
        (destination / 'live.m3u8').write_text('#EXTM3U\n')
    catalog = {str(path.relative_to(directory)): hashlib.sha256(path.read_bytes()).hexdigest()
               for path in directory.rglob('*') if path.is_file() and path.name != 'identity.json'}
    (directory / 'identity.json').write_text(json.dumps({'generator': 'ffmpeg-hls-muxer', 'seconds': 300,
        'sharedMediaSourceSha256': hashlib.sha256(source.read_bytes()).hexdigest(), 'files': catalog}, indent=2) + '\n')
    return len(catalog)


def sliding_playlist(text, elapsed, start=False, resource_prefix=''):
    """Expose 48 seconds of continuous media; never wrap or recycle timestamped bytes."""
    lines = text.splitlines()
    header = [line for line in lines if line.startswith(('#EXT-X-VERSION:', '#EXT-X-TARGETDURATION:', '#EXT-X-MAP:', '#EXT-X-INDEPENDENT-SEGMENTS'))]
    segments = []
    cumulative = 0.0
    for index, line in enumerate(lines):
        if line.startswith('#EXTINF:'):
            duration = float(line.split(':', 1)[1].rstrip(','))
            uri = lines[index + 1]
            if not duration > 0 or uri.startswith('#'):
                raise ValueError('Invalid independent fixture segment')
            cumulative += duration
            segments.append((cumulative, line, resource_prefix + uri))
    if not segments or len(segments) > 256:
        raise ValueError('Independent fixture requires 1–256 segments')
    horizon = min(segments[-1][0], 96 + max(0, elapsed))
    end = sum(1 for item in segments if item[0] <= horizon + 0.001)
    begin = max(0, end - 24)
    result = ['#EXTM3U'] + header + [f'#EXT-X-MEDIA-SEQUENCE:{begin}']
    if start:
        result.append('#EXT-X-START:TIME-OFFSET=-6.0,PRECISE=YES')
    for _, duration, uri in segments[begin:end]:
        result.extend([duration, uri])
    if end == len(segments):
        result.append('#EXT-X-ENDLIST')
    return '\n'.join(result) + '\n'


def live_response(path, elapsed, session, start=False):
    if path.parent.name == 'fmp4-separate' and path.name == 'live.m3u8':
        return re.sub(r'variant-(\d)\.m3u8', lambda match: f'live-{match[1]}.m3u8?session={session}',
                      (path.parent / 'master.m3u8').read_text())
    original = 'variant-' + path.stem.split('-')[-1] + '.m3u8' if path.name.startswith('live-') else 'vod.m3u8'
    return sliding_playlist((path.parent / original).read_text(), elapsed, start)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    options = parser.parse_args()
    print(json.dumps({'status': 'PASS', 'files': prepare(options.root.resolve())}))
