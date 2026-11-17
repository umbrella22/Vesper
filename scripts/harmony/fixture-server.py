#!/usr/bin/env python3
"""Generate small deterministic media and serve bounded, audited Range requests."""

import argparse
import hashlib
import http.server
import json
import mimetypes
from pathlib import Path
import re
import struct
import subprocess
import threading
import time
import xml.etree.ElementTree as ET
from urllib.parse import parse_qs, unquote, urlsplit
from independent_hls import prepare as prepare_independent_hls, sliding_playlist, live_response


def command(args):
    subprocess.run(args, check=True, timeout=60, stdout=subprocess.DEVNULL,
                   stderr=subprocess.PIPE)


def segment_base(path):
    data = path.read_bytes()
    offset = 0
    while offset + 8 <= len(data):
        size, kind = struct.unpack_from('>I4s', data, offset)
        if size < 8 or offset + size > len(data):
            raise ValueError('Invalid generated MP4 box')
        if kind == b'sidx':
            return offset, offset + size - 1
        offset += size
    raise ValueError('Generated DASH media has no sidx')


def timeline_entries(template):
    """Expand this fixture's finite timeline, retaining exact ticks and numbering."""
    entries = []
    tick = 0
    number = int(template.get('startNumber', '1'))
    for entry in template.find('{*}SegmentTimeline'):
        tick = int(entry.get('t', tick))
        duration = int(entry.get('d'))
        repeat = int(entry.get('r', '0'))
        if duration <= 0 or repeat < 0 or len(entries) + repeat + 1 > 256:
            raise ValueError('Fixture timeline is not finite and bounded')
        for _ in range(repeat + 1):
            entries.append((number, tick, duration))
            tick += duration
            number += 1
    return entries


def time_addressed_manifest(directory):
    """Give the offline fixture real Time-addressed files and a non-one sequence."""
    path = directory / 'manifest.mpd'
    document = ET.fromstring(path.read_text())
    for adaptation in document.findall('.//{*}AdaptationSet'):
        for representation in adaptation.findall('{*}Representation'):
            template = representation.find('{*}SegmentTemplate')
            media = template.get('media')
            for number, tick, _ in timeline_entries(template):
                old_name = media.replace('$RepresentationID$', representation.get('id')).replace('$Number%05d$', f'{number:05d}').replace('$Number$', str(number))
                new_name = f'time-{representation.get("id")}-{tick}.m4s'
                (directory / old_name).rename(directory / new_name)
            template.set('media', 'time-$RepresentationID$-$Time$.m4s')
            template.set('startNumber', '41')
    ET.register_namespace('', 'urn:mpeg:dash:schema:mpd:2011')
    path.write_text(ET.tostring(document, encoding='unicode'))


def live_dash_manifest(path, elapsed):
    """Expose a 24-second sliding window over continuous, finite fMP4 timestamps."""
    document = ET.fromstring(path.read_text())
    document.set('type', 'dynamic')
    document.set('minimumUpdatePeriod', 'PT2S')
    document.set('timeShiftBufferDepth', 'PT24S')
    document.attrib.pop('mediaPresentationDuration', None)
    begin = min(80, int(elapsed // 2) * 2)
    for period in document.findall('{*}Period'):
        period.attrib.pop('start', None)
        period.attrib.pop('duration', None)
        for template in period.findall('.//{*}SegmentTemplate'):
            scale = int(template.get('timescale', '1'))
            entries = timeline_entries(template)
            selected = [(number, tick, duration) for number, tick, duration in entries
                        if tick >= begin * scale - scale // 20 and tick < (begin + 24) * scale - scale // 20]
            template.set('startNumber', str(selected[0][0]))
            timeline = template.find('{*}SegmentTimeline')
            timeline.clear()
            for number, tick, duration in selected:
                ET.SubElement(timeline, '{urn:mpeg:dash:schema:mpd:2011}S', {'t': str(tick), 'd': str(duration)})
    return ET.tostring(document, encoding='utf-8')


def webvtt_mp4(path):
    """Write an ISO/IEC 14496-30 wvtt track with three timed samples."""
    def box(kind, payload):
        return struct.pack('>I4s', len(payload) + 8, kind.encode()) + payload
    def full(kind, flags, payload):
        return box(kind, struct.pack('>I', flags) + payload)
    def words(*values):
        return struct.pack('>' + 'I' * len(values), *values)
    matrix = words(0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000)
    mvhd = full('mvhd', 0, words(0, 0, 1000, 12000, 0x10000) + b'\x01\x00' + bytes(10) + matrix + bytes(24) + words(2))
    tkhd = full('tkhd', 3, words(0, 0, 1, 0, 12000) + bytes(16) + matrix + words(0, 0))
    mdhd = full('mdhd', 0, words(0, 0, 1000, 12000) + struct.pack('>HH', 0x15c7, 0))
    hdlr = full('hdlr', 0, words(0) + b'text' + bytes(12) + b'WebVTT\x00')
    entry = box('wvtt', bytes(6) + b'\x00\x01' + box('vttC', b'WEBVTT'))
    stbl = box('stbl', full('stsd', 0, words(1) + entry) + full('stts', 0, words(0))
               + full('stsc', 0, words(0)) + full('stsz', 0, words(0, 0)) + full('stco', 0, words(0)))
    dinf = box('dinf', full('dref', 0, words(1) + full('url ', 1, b'')))
    minf = box('minf', full('nmhd', 0, b'') + dinf + stbl)
    trak = box('trak', tkhd + box('mdia', mdhd + hdlr + minf))
    moov = box('moov', mvhd + trak + box('mvex', full('trex', 0, words(1, 1, 0, 0, 0))))
    samples = [box('vtte', b''), box('vttc', box('payl', b'Vesper DASH WebVTT')), box('vtte', b'')]
    def fragment(offset):
        records = b''.join(words(duration, len(sample), 0x02000000)
                           for duration, sample in zip([100, 7900, 4000], samples))
        traf = box('traf', full('tfhd', 0x020000, words(1)) + full('tfdt', 0x01000000, struct.pack('>Q', 0))
                   + full('trun', 0x701, words(3, offset) + records))
        return box('moof', full('mfhd', 0, words(1)) + traf)
    moof = fragment(0)
    media = fragment(len(moof) + 8) + box('mdat', b''.join(samples))
    sidx = full('sidx', 0, words(1, 1000, 0, 0) + struct.pack('>HH', 0, 1) + words(len(media), 12000, 0x90000000))
    path.write_bytes(box('ftyp', b'iso6' + words(1) + b'iso6dashmp41') + moov + sidx + media)


def prepare(root):
    root.mkdir(parents=True, exist_ok=True)
    command(['ffmpeg', '-y', '-v', 'error', '-f', 'lavfi', '-i',
             'testsrc2=size=320x180:rate=24', '-f', 'lavfi', '-i',
             'sine=frequency=660:sample_rate=48000', '-t', '12', '-c:v', 'libx264',
             '-preset', 'ultrafast', '-profile:v', 'baseline', '-level:v', '3.0',
             '-pix_fmt', 'yuv420p', '-g', '48', '-bf', '0', '-c:a', 'aac',
             '-movflags', '+faststart', str(root / 'probe.mp4')])
    command(['ffmpeg', '-y', '-v', 'error', '-i', str(root / 'probe.mp4'), '-vf', 'transpose=1',
             '-c:v', 'libx264', '-preset', 'ultrafast', '-profile:v', 'baseline', '-g', '48', '-bf', '0',
             '-c:a', 'copy', '-movflags', '+faststart', str(root / 'portrait.mp4')])
    command(['ffmpeg', '-y', '-v', 'error', '-i', str(root / 'probe.mp4'), '-f', 'lavfi', '-i',
             'sine=frequency=880:sample_rate=48000', '-t', '12', '-map', '0:v:0', '-map', '0:a:0', '-map', '1:a:0',
             '-c:v', 'copy', '-c:a', 'aac', '-metadata:s:a:0', 'language=eng', '-metadata:s:a:1', 'language=jpn',
             '-movflags', '+faststart', str(root / 'tracks.mp4')])
    hls = root / 'hls'
    hls.mkdir(exist_ok=True)
    command(['ffmpeg', '-y', '-v', 'error', '-i', str(root / 'probe.mp4'),
             '-c', 'copy', '-f', 'hls', '-hls_time', '2', '-hls_list_size', '0',
             '-hls_segment_filename', str(hls / 'segment-%02d.ts'), str(hls / 'media.m3u8')])
    low = hls / 'low'; low.mkdir(exist_ok=True)
    command(['ffmpeg', '-y', '-v', 'error', '-i', str(root / 'probe.mp4'), '-vf', 'scale=160:90',
             '-c:v', 'libx264', '-preset', 'ultrafast', '-profile:v', 'baseline', '-g', '48', '-bf', '0', '-c:a', 'copy',
             '-f', 'hls', '-hls_time', '2', '-hls_list_size', '0', '-hls_segment_filename', str(low / 'segment-%02d.ts'), str(low / 'media.m3u8')])
    (hls / 'master.m3u8').write_text('#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=350000,RESOLUTION=160x90\nlow/media.m3u8\n'
                                   '#EXT-X-STREAM-INF:BANDWIDTH=1200000,RESOLUTION=320x180\nmedia.m3u8\n')
    abr = root / 'abr'; abr.mkdir(exist_ok=True)
    abr_low = abr / 'low'; abr_low.mkdir(exist_ok=True)
    command(['ffmpeg', '-y', '-v', 'error', '-stream_loop', '3', '-i', str(root / 'probe.mp4'),
             '-t', '48', '-c', 'copy', '-f', 'hls', '-hls_time', '2', '-hls_list_size', '0',
             '-hls_segment_filename', str(abr / 'segment-%02d.ts'), str(abr / 'media.m3u8')])
    command(['ffmpeg', '-y', '-v', 'error', '-stream_loop', '3', '-i', str(root / 'probe.mp4'),
             '-t', '48', '-vf', 'scale=160:90', '-c:v', 'libx264', '-preset', 'ultrafast',
             '-profile:v', 'baseline', '-g', '48', '-bf', '0', '-c:a', 'copy', '-f', 'hls',
             '-hls_time', '2', '-hls_list_size', '0', '-hls_segment_filename', str(abr_low / 'segment-%02d.ts'),
             str(abr_low / 'media.m3u8')])
    (abr / 'master.m3u8').write_text((hls / 'master.m3u8').read_text())
    live = root / 'live'
    live.mkdir(exist_ok=True)
    (live / 'media.m3u8').write_text('#EXTM3U\n')
    dvr = root / 'dvr'
    dvr.mkdir(exist_ok=True)
    prepare_independent_hls(root)
    (dvr / 'media.m3u8').write_text('#EXTM3U\n')
    dash = root / 'dash'
    dash.mkdir(exist_ok=True)
    for kind, selector in [('video', '0:v:0'), ('audio', '0:a:0')]:
        command(['ffmpeg', '-y', '-v', 'error', '-i', str(root / 'probe.mp4'),
                 '-map', selector, '-c', 'copy', '-f', 'mp4', '-movflags',
                 '+dash+global_sidx', '-frag_duration', '2000000', str(dash / f'{kind}.mp4')])
    representations = []
    for kind, codec, bandwidth in [('video', 'avc1.42c01e', 1000000), ('audio', 'mp4a.40.2', 128000)]:
        begin, end = segment_base(dash / f'{kind}.mp4')
        dimensions = ' width="320" height="180" frameRate="24"' if kind == 'video' else ' audioSamplingRate="48000"'
        representations.append(f'<AdaptationSet contentType="{kind}" mimeType="{kind}/mp4">'
                               f'<Representation id="{kind}" codecs="{codec}" bandwidth="{bandwidth}"{dimensions}>'
                               f'<BaseURL>{kind}.mp4</BaseURL><SegmentBase indexRange="{begin}-{end}">'
                               f'<Initialization range="0-{begin - 1}"/></SegmentBase>'
                               '</Representation></AdaptationSet>')
    (dash / 'manifest.mpd').write_text('<?xml version="1.0"?>\n'
        '<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" '
        'profiles="urn:mpeg:dash:profile:isoff-on-demand:2011" '
        'mediaPresentationDuration="PT12S" minBufferTime="PT1S">'
        '<Period duration="PT12S">' + ''.join(representations) + '</Period></MPD>')
    (dash / 'captions.vtt').write_text('WEBVTT\n\n00:00.100 --> 00:08.000\nVesper DASH WebVTT\n\n')
    original = (dash / 'manifest.mpd').read_text()
    (dash / 'subtitles.mpd').write_text(original.replace('</Period>', '<AdaptationSet contentType="text" mimeType="text/vtt" lang="en">'
        '<Representation id="captions" bandwidth="1024"><BaseURL>captions.vtt</BaseURL></Representation></AdaptationSet></Period>'))
    webvtt_mp4(dash / 'captions.mp4')
    begin, end = segment_base(dash / 'captions.mp4')
    (dash / 'wvtt.mpd').write_text(original.replace('</Period>', '<AdaptationSet contentType="text" mimeType="application/mp4" lang="en">'
        f'<Representation id="captions" codecs="wvtt" bandwidth="1024"><BaseURL>captions.mp4</BaseURL><SegmentBase indexRange="{begin}-{end}">'
        f'<Initialization range="0-{begin - 1}"/></SegmentBase></Representation></AdaptationSet></Period>'))
    for name, timeline in [('template', '0'), ('timeline', '1')]:
        directory = root / name
        directory.mkdir(exist_ok=True)
        command(['ffmpeg', '-y', '-v', 'error', '-i', str(root / 'tracks.mp4'), '-map', '0:v:0', '-map', '0:a',
                 '-c', 'copy', '-f', 'dash', '-adaptation_sets', 'id=0,streams=v id=1,streams=1 id=2,streams=2',
                 '-seg_duration', '2', '-use_template', '1', '-use_timeline', timeline,
                 str(directory / 'manifest.mpd')])
        manifest = directory / 'manifest.mpd'
        # Explicit Period offsets remain outside the separately validated playback profile.
        manifest.write_text(re.sub(r' start="[^"]*"', '', manifest.read_text()))
        if name == 'timeline':
            time_addressed_manifest(directory)
    dynamic = root / 'dash-live'
    dynamic.mkdir(exist_ok=True)
    command(['ffmpeg', '-y', '-v', 'error', '-stream_loop', '9', '-i', str(root / 'probe.mp4'),
             '-t', '120', '-map', '0:v:0', '-map', '0:a:0', '-c', 'copy', '-f', 'dash',
             '-seg_duration', '2', '-use_template', '1', '-use_timeline', '1', str(dynamic / 'manifest.mpd')])
    long_template = root / 'long-template'
    long_template.mkdir(exist_ok=True)
    command(['ffmpeg', '-y', '-v', 'error', '-f', 'lavfi', '-i', 'color=c=navy:size=160x90:rate=1',
             '-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=48000', '-t', '7200',
             '-c:v', 'libx264', '-preset', 'ultrafast', '-profile:v', 'baseline', '-g', '6', '-bf', '0',
             '-c:a', 'aac', '-b:a', '16000', '-f', 'dash', '-seg_duration', '6',
             '-use_template', '1', '-use_timeline', '0', str(long_template / 'manifest.mpd')])
    manifest = long_template / 'manifest.mpd'
    manifest.write_text(re.sub(r' start="[^"]*"', '', manifest.read_text()))
    (root / 'caption-long.vtt').write_text('WEBVTT\n\n00:00.100 --> 00:08.000\n'
        'A long caption wraps within the video and remains clear above playback controls.\n'
        '\u4e2d\u6587\u5b57\u5e55\u7528\u4e8e\u68c0\u67e5\u6362\u884c\u3001\u5927\u5b57\u548c\u63a7\u5236\u680f\u907f\u8ba9\u3002\n\n')
    catalog = {str(p.relative_to(root)): {'bytes': p.stat().st_size,
               'sha256': hashlib.sha256(p.read_bytes()).hexdigest()}
               for p in root.rglob('*') if p.is_file() and p.name != 'catalog.json'}
    (root / 'catalog.json').write_text(json.dumps(catalog, indent=2) + '\n')


def serve(root, port, audit, allow_anonymous=False):
    lock = threading.Lock()
    records = []
    total_requests = 0
    started = time.monotonic()
    recover_once = {}
    armed_faults = {}
    live_sessions = {}

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_GET(self):
            nonlocal total_requests
            body_write_completed = None
            route = unquote(urlsplit(self.path).path)
            slow = route.startswith('/slow/')
            if slow:
                route = route.replace('/slow', '', 1)
            ignored_range = route.startswith('/no-range/')
            if ignored_range:
                route = route.replace('/no-range', '', 1)
            transient = False
            if route.startswith('/recover-once/'):
                with lock:
                    if self.path not in recover_once:
                        if len(recover_once) >= 32:
                            recover_once.pop(next(iter(recover_once)))
                        recover_once[self.path] = True
                        transient = True
                route = route.replace('/recover-once', '', 1)
            fault_case = None
            fault_route = re.fullmatch(r'/fault-once/(\d{10,16})(/.*)', route)
            if fault_route:
                fault_case, route = fault_route.groups()
                with lock:
                    suffix = armed_faults.get(fault_case)
                    if suffix and route.endswith('.' + suffix):
                        transient = True
                        armed_faults.pop(fault_case)
            arm = re.fullmatch(r'/fault-control/(\d{10,16})/arm', route)
            if route == '/audit':
                with lock:
                    data = json.dumps({'totalRequests': total_requests, 'records': records}).encode()
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(data)))
                self.end_headers()
                self.wfile.write(data)
                return
            with lock:
                total_requests += 1
            authorized = self.headers.get('X-Vesper-Probe') == 'h00'
            response = 200
            if route.startswith('/redirect/'):
                self.send_response(302)
                self.send_header('Location', f'http://127.0.0.1:{port}' + route.replace('/redirect', '', 1))
                self.send_header('Content-Length', '0')
                self.end_headers()
                response = 302
                count = 0
            elif not authorized and not allow_anonymous:
                self.send_error(403)
                response = 403
                count = 0
            elif arm:
                with lock:
                    if len(armed_faults) >= 32:
                        armed_faults.pop(next(iter(armed_faults)))
                    armed_faults[arm[1]] = parse_qs(urlsplit(self.path).query).get('resource', ['ts'])[0]
                self.send_response(200)
                self.send_header('Content-Length', '2')
                self.end_headers()
                self.wfile.write(b'OK')
                count = 2
            elif transient:
                self.send_response(503)
                self.send_header('Content-Length', '0')
                self.end_headers()
                response, count = 503, 0
            else:
                path = (root / route.lstrip('/')).resolve()
                if root not in path.parents or not path.is_file():
                    self.send_error(404)
                    response, count = 404, 0
                else:
                    data = path.read_bytes()
                    if route == '/live/media.m3u8':
                        sequence = 100 + int((time.monotonic() - started) // 2)
                        lines = ['#EXTM3U', '#EXT-X-VERSION:3', '#EXT-X-TARGETDURATION:2', f'#EXT-X-MEDIA-SEQUENCE:{sequence}']
                        for number in range(sequence, sequence + 3):
                            lines.extend(['#EXT-X-DISCONTINUITY', '#EXTINF:2.0,', f'/hls/segment-{number % 6:02d}.ts?sequence={number}'])
                        data = ('\n'.join(lines) + '\n').encode()
                    elif route in ('/dvr/media.m3u8', '/dash-live/manifest.mpd') or (route.startswith('/independent/') and path.name.startswith('live') and path.suffix == '.m3u8'):
                        session = parse_qs(urlsplit(self.path).query).get('session', ['default'])[0][:64]
                        with lock:
                            if session not in live_sessions:
                                if len(live_sessions) >= 32:
                                    live_sessions.pop(next(iter(live_sessions)))
                                live_sessions[session] = time.monotonic()
                            elapsed = time.monotonic() - live_sessions[session]
                        if route == '/dash-live/manifest.mpd':
                            data = live_dash_manifest(path, elapsed)
                        elif route.startswith('/independent/'):
                            data = live_response(path, elapsed, session, parse_qs(urlsplit(self.path).query).get('start') == ['1']).encode()
                        else:
                            data = sliding_playlist((root / 'independent/ts/vod.m3u8').read_text(), elapsed,
                                parse_qs(urlsplit(self.path).query).get('start') == ['1'], '/independent/ts/').encode()
                    begin, end = 0, len(data) - 1
                    match = re.fullmatch(r'bytes=(\d+)-(\d*)', self.headers.get('Range', ''))
                    if match and not ignored_range:
                        begin = int(match[1])
                        end = min(int(match[2]) if match[2] else end, end)
                        response = 206
                    if begin > end or begin >= len(data):
                        self.send_response(416)
                        self.send_header('Content-Range', f'bytes */{len(data)}')
                        self.send_header('Content-Length', '0')
                        self.end_headers()
                        response, count = 416, 0
                    else:
                        self.send_response(response)
                        content_type = {'.mpd': 'application/dash+xml', '.m3u8': 'application/vnd.apple.mpegurl', '.ts': 'video/mp2t'}.get(path.suffix, mimetypes.guess_type(path)[0] or 'application/octet-stream')
                        self.send_header('Content-Type', content_type)
                        self.send_header('Accept-Ranges', 'bytes')
                        self.send_header('ETag', '"' + hashlib.sha256(data).hexdigest() + '"')
                        self.send_header('Content-Length', str(end - begin + 1))
                        if response == 206:
                            self.send_header('Content-Range', f'bytes {begin}-{end}/{len(data)}')
                        self.end_headers()
                        try:
                            if slow:
                                time.sleep(2)
                            self.wfile.write(data[begin:end + 1])
                            self.wfile.flush()
                            body_write_completed = True
                        except (BrokenPipeError, ConnectionResetError):
                            body_write_completed = False
                        count = end - begin + 1
            record = {'time': time.time(), 'path': route, 'authorized': authorized, 'anonymousAllowed': allow_anonymous,
                      'range': self.headers.get('Range'), 'status': response, 'bytes': count,
                      'bodyWriteCompleted': body_write_completed, 'faultCase': fault_case}
            with lock:
                if len(records) >= 4096:
                    records.pop(0)
                records.append(record)
                if total_requests <= 100000:
                    with audit.open('a') as output:
                        output.write(json.dumps(record) + '\n')

    server = http.server.ThreadingHTTPServer(('127.0.0.1', port), Handler)
    server.daemon_threads = True
    print(json.dumps({'status': 'listening', 'port': port, 'root': str(root), 'audit': str(audit)}), flush=True)
    server.serve_forever()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--port', type=int, default=18765)
    parser.add_argument('--allow-anonymous', action='store_true', help='Allow URL-only example apps on this loopback test origin')
    parser.add_argument('--prepare', action='store_true')
    parser.add_argument('--prepare-only', action='store_true')
    options = parser.parse_args()
    directory = options.root.resolve()
    if options.prepare or options.prepare_only:
        prepare(directory)
    if not options.prepare_only:
        serve(directory, options.port, directory.parent / 'origin-audit.jsonl', allow_anonymous=options.allow_anonymous)
