#!/usr/bin/env python3
"""Publish a timestamped test pattern to a controlled MediaMTX RTSP origin."""

import argparse
from datetime import datetime, timezone
from ipaddress import IPv4Address
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time


WIDTH, HEIGHT, CLOCK_HEIGHT = 640, 360, 40
AUTH_MODES = ("none", "basic", "digest")
# Public test fixture credentials. Keep them out of product configuration.
READER = "fixture-reader"
READER_PASSWORD = "vesper-fixture-test"


def reader_uri(host, port, auth):
    user_info = f"{READER}:{READER_PASSWORD}@" if auth != "none" else ""
    return f"rtsp://{user_info}{host}:{port}/clock"


DIGITS = {
    "0": (7, 5, 5, 5, 7), "1": (2, 6, 2, 2, 7), "2": (7, 1, 7, 4, 7),
    "3": (7, 1, 7, 1, 7), "4": (5, 5, 7, 1, 1), "5": (7, 4, 7, 1, 7),
    "6": (7, 4, 7, 5, 7), "7": (7, 1, 1, 1, 1), "8": (7, 5, 7, 5, 7),
    "9": (7, 5, 7, 1, 7), ":": (0, 2, 0, 2, 0), ".": (0, 0, 0, 0, 2),
    " ": (0, 0, 0, 0, 0),
}


def clock_frame(frame):
    now = datetime.now(timezone.utc)
    text = f"{now:%H:%M:%S}.{now.microsecond // 1000:03}  {frame % 1000000:06}"
    pixels = bytearray(WIDTH * CLOCK_HEIGHT)
    scale = 5
    for index, character in enumerate(text):
        for row, bits in enumerate(DIGITS[character]):
            for column in range(3):
                if bits & (4 >> column):
                    x = 12 + (index * 4 + column) * scale
                    for dy in range(scale):
                        offset = (7 + row * scale + dy) * WIDTH + x
                        pixels[offset:offset + scale] = b"\xff" * scale
    return pixels


def wait_for_listener(server, host, port):
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if server.poll() is not None:
            break
        try:
            with socket.create_connection((host, port), timeout=0.2):
                return
        except OSError:
            time.sleep(0.05)
    raise RuntimeError("MediaMTX did not start its RTSP listener")


def stop(child, timeout=3):
    if child.poll() is None:
        child.terminate()
        try:
            child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()


def publish(args, url):
    inputs = [
        args.ffmpeg, "-hide_banner", "-loglevel", "warning", "-nostdin",
        "-f", "rawvideo", "-pixel_format", "gray", "-video_size", f"{WIDTH}x{CLOCK_HEIGHT}",
        "-framerate", str(args.fps), "-i", "pipe:0", "-f", "lavfi", "-i",
        f"testsrc2=size={WIDTH}x{HEIGHT - CLOCK_HEIGHT}:rate={args.fps}",
    ]
    if args.aac:
        inputs.extend(["-f", "lavfi", "-i", "sine=frequency=660:sample_rate=48000"])
    filters = ["-filter_complex", "[0:v]format=yuv420p[clock];[clock][1:v]vstack=shortest=1[video]", "-map", "[video]"]
    audio = ["-map", "2:a", "-c:a", "aac", "-profile:a", "aac_low", "-b:a", "64k"] if args.aac else ["-an"]
    if args.video == "hevc":
        codec = ["-c:v", "libx265", "-preset", "ultrafast", "-tune", "zerolatency", "-profile:v", "main",
                 "-x265-params", f"repeat-headers=1:keyint={args.gop}:min-keyint={args.gop}:scenecut=0:bframes=0:log-level=error:frame-threads=1:numa-pools=none"]
    else:
        codec = ["-c:v", "libx264", "-preset", "ultrafast", "-tune", "zerolatency",
                 "-profile:v", "baseline", "-bf", "0", "-g", str(args.gop), "-sc_threshold", "0",
                 "-x264-params", "repeat-headers=1"]
    ffmpeg = subprocess.Popen(inputs + filters + codec + audio + [
        "-f", "rtsp", "-rtsp_transport", args.transport, url,
    ], stdin=subprocess.PIPE, bufsize=0)
    frame = 0
    next_frame = time.monotonic()
    interval = 1 / args.fps
    print(f"RTSP fixture: {url} (UTC clock, frame counter, {args.gop}-frame GOP)", flush=True)
    try:
        while ffmpeg.poll() is None:
            now = time.monotonic()
            if now - next_frame > interval:
                next_frame = now
            remaining = memoryview(clock_frame(frame))
            while remaining:
                written = ffmpeg.stdin.write(remaining)
                if not written:
                    raise RuntimeError("FFmpeg closed its frame input")
                remaining = remaining[written:]
            frame += 1
            next_frame += interval
            time.sleep(max(0, next_frame - time.monotonic()))
        raise RuntimeError(f"FFmpeg exited with status {ffmpeg.returncode}")
    finally:
        ffmpeg.stdin.close()
        stop(ffmpeg)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mediamtx", default="mediamtx")
    parser.add_argument("--ffmpeg", default="ffmpeg")
    parser.add_argument("--listen", type=IPv4Address, default=IPv4Address("127.0.0.1"))
    parser.add_argument("--port", type=int, default=8554)
    parser.add_argument("--fps", type=int, default=30)
    parser.add_argument("--gop", type=int, help="Frames between IDRs. Defaults to one second.")
    parser.add_argument("--video", choices=("h264", "hevc"), default="h264")
    parser.add_argument("--aac", action="store_true", help="Include a 48 kHz AAC-LC tone.")
    parser.add_argument("--transport", choices=("tcp", "udp"), default="tcp")
    parser.add_argument("--auth", choices=AUTH_MODES, default="none", help="Require fixture credentials for playback.")
    parser.add_argument("--udp-port", type=int, default=8000, help="MediaMTX RTP port for the UDP fixture.")
    args = parser.parse_args()
    if args.gop is None:
        args.gop = args.fps
    if not (1 <= args.port <= 65535 and 1 <= args.fps <= 60 and args.gop > 0):
        parser.error("Expected a valid port, 1 to 60 fps, and a positive GOP length")
    if args.transport == "udp" and (not 1024 <= args.udp_port < 65535 or args.udp_port % 2):
        parser.error("Expected an even UDP port between 1024 and 65534.")
    host = "127.0.0.1" if args.listen.is_unspecified else str(args.listen)
    configuration = f"""rtsp: true
rtspAddress: {args.listen}:{args.port}
rtspTransports: [{args.transport}]
rtpAddress: {args.listen}:{args.udp_port}
rtcpAddress: {args.listen}:{args.udp_port + 1}
rtmp: false
hls: false
webrtc: false
srt: false
moq: false
paths:
  clock:
    source: publisher
"""
    if args.auth != "none":
        configuration += f"""rtspAuthMethods: [{args.auth}]
authInternalUsers:
  - user: any
    ips: [{host}]
    permissions:
      - action: publish
        path: clock
  - user: {READER}
    pass: {READER_PASSWORD}
    permissions:
      - action: read
        path: clock
"""
    with tempfile.TemporaryDirectory(prefix="vesper-rtsp-") as directory:
        config = Path(directory) / "mediamtx.yml"
        config.write_text(configuration, encoding="utf-8")
        server = subprocess.Popen([args.mediamtx, str(config)])
        try:
            wait_for_listener(server, host, args.port)
            publish(args, f"rtsp://{host}:{args.port}/clock")
        finally:
            stop(server)


def interrupt(_signum, _frame):
    raise KeyboardInterrupt


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, interrupt)
    try:
        main()
    except KeyboardInterrupt:
        pass
    except (OSError, RuntimeError) as error:
        raise SystemExit(str(error)) from None
