#!/usr/bin/env python3
"""Verify RTSP ingest, fMP4 playback, and live recovery on the development host."""

import argparse
from contextlib import ExitStack
import json
from pathlib import Path
import re
import socket
import subprocess
import sys
import time
from urllib.error import HTTPError, URLError
from urllib.parse import urljoin
from urllib.request import Request, urlopen

from fixture import AUTH_MODES, READER_PASSWORD, reader_uri, stop


def eventually(predicate, description, seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        result = predicate()
        if result:
            return result
        time.sleep(0.1)
    raise RuntimeError(f"Timed out waiting for {description}")


def request(url, method="GET", **headers):
    try:
        response = urlopen(Request(url, method=method, headers=headers), timeout=1)
    except HTTPError as error:
        response = error
    with response:
        return response.status, response.headers, response.read()


def playlist(url):
    status, _, content = request(url)
    if status == 200:
        return content.decode("ascii")
    raise RuntimeError(f"Unexpected playlist HTTP status {status}")


def sequence(manifest):
    return int(re.search(r"^#EXT-X-MEDIA-SEQUENCE:(\d+)$", manifest, re.MULTILINE)[1])


def probe(ffprobe, url, format_options, stream="v:0"):
    fields = "codec_name,profile,r_frame_rate,width,height" if stream == "v:0" else "codec_name,sample_rate,channels"
    output = subprocess.check_output([
        ffprobe, "-v", "error", *format_options, "-select_streams", stream,
        "-show_entries", f"stream={fields}", "-of", "json", url,
    ], timeout=12)
    return json.loads(output)["streams"][0]


def listener_released(port):
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=0.2):
            return False
    except ConnectionRefusedError:
        return True


def verify(args):
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    origin = reader_uri("127.0.0.1", port, args.auth)
    fixture_command = [
        sys.executable, "-u", str(Path(__file__).with_name("fixture.py")),
        "--mediamtx", args.mediamtx, "--ffmpeg", args.ffmpeg, "--port", str(port),
        "--video", args.video, "--transport", args.transport, "--udp-port", str(args.udp_port),
        "--auth", args.auth, "--gop", str(args.gop),
    ]
    if args.aac:
        fixture_command.append("--aac")
    phase_log = args.output / "bridge.log"
    playlist_file = args.output / "playlist.url"
    with ExitStack() as resources:
        def start_origin(log_name):
            path = args.output / log_name
            with path.open("w", encoding="utf-8") as log:
                process = subprocess.Popen(fixture_command, stdout=log, stderr=subprocess.STDOUT)
            # The fixture gives its encoder and origin time to shut down before it exits.
            resources.callback(stop, process, timeout=8)

            def published():
                if process.poll() is not None:
                    raise RuntimeError(f"RTSP fixture exited during startup. See {path}")
                return "stream is available and online" in path.read_text(encoding="utf-8")

            eventually(published, f"RTSP publication in {path}", 8)
            return process

        source = start_origin("origin.log")
        if args.auth != "none":
            for denied in (reader_uri("127.0.0.1", port, "none"), origin.replace(READER_PASSWORD, "wrong-test-password")):
                rejected = subprocess.run([args.bridge, denied], capture_output=True, text=True, timeout=9)
                assert rejected.returncode != 0 and not rejected.stdout
                assert "AuthenticationRequired" in rejected.stderr
                assert "wrong-test-password" not in rejected.stderr
        with phase_log.open("w", encoding="utf-8") as phase, playlist_file.open("w", encoding="utf-8") as route:
            start = time.monotonic()
            bridge = subprocess.Popen([args.bridge, origin], stdout=route, stderr=phase)
            resources.callback(stop, bridge)
        url = eventually(lambda: playlist_file.read_text(encoding="ascii").strip(), "the HLS route", 9)
        ready_seconds = time.monotonic() - start
        input_options = ["-rtsp_transport", args.transport]
        direct_video = probe(args.ffprobe, origin, input_options)
        bridged_video = probe(args.ffprobe, url, [])
        assert direct_video == bridged_video == {
            "codec_name": args.video, "profile": "Main" if args.video == "hevc" else "Constrained Baseline",
            "width": 640, "height": 360, "r_frame_rate": "30/1",
        }, (direct_video, bridged_video)
        audio = None
        maps = ["-map", "0:v:0"]
        if args.aac:
            audio = probe(args.ffprobe, url, [], stream="a:0")
            assert audio == probe(args.ffprobe, origin, input_options, stream="a:0") == {
                "codec_name": "aac", "sample_rate": "48000", "channels": 1,
            }
            maps.extend(["-map", "0:a:0"])
        subprocess.run([args.ffmpeg, "-v", "error", "-xerror", "-i", url,
                        "-t", "8", *maps, "-f", "null", "-"],
                       check=True, timeout=18)

        before = playlist(url)
        assert "#EXT-X-ENDLIST" not in before and "#EXT-X-TARGETDURATION:2\n" in before
        durations = [float(value) for value in re.findall(r"^#EXTINF:([\d.]+),", before, re.MULTILINE)]
        assert durations and all(0 < duration < 2.5 for duration in durations)
        init = urljoin(url, re.search(r'#EXT-X-MAP:URI="([^"]+)"', before)[1])
        media = urljoin(url, re.search(r"^(seg-\d+\.m4s)$", before, re.MULTILINE)[1])
        status, response, body = request(init, Range="bytes=0-1")
        assert status == 206 and body == b"\0\0" and response["Content-Range"].startswith("bytes 0-1/")
        status, response, body = request(media, method="HEAD")
        assert status == 200 and int(response["Content-Length"]) > 0 and not body
        assert request(media, Range="bytes=999999999-")[0] == 416
        assert request(url, Host="other.invalid")[0] == 400
        assert request(urljoin(url, "/wrong/index.m3u8"))[0] == 404

        stop(source, timeout=8)
        assert source.returncode == 0, "The fixture did not shut down cleanly"
        eventually(lambda: listener_released(port), "the RTSP listener to close", 5)
        listener_closed_at = time.monotonic()
        eventually(lambda: "Reconnecting" in phase_log.read_text(encoding="utf-8"), "reconnect state", 5)
        during = playlist(url)
        assert "#EXT-X-ENDLIST" not in during and sequence(during) >= sequence(before)
        while time.monotonic() - listener_closed_at < 10:
            assert playlist(url) == during, "Playlist changed while the origin was offline"
            time.sleep(0.25)
        restarted = start_origin("origin-restarted.log")
        published_at = time.monotonic()

        def resumed():
            after = playlist(url)
            phases = phase_log.read_text(encoding="utf-8").splitlines()
            if (phases[-1] == "Ready" and "#EXT-X-DISCONTINUITY\n" in after
                    and sequence(after) > sequence(during)):
                return after
            return None
        after = eventually(resumed, "a discontinuity and a growing live window", 10)
        recovered_at = time.monotonic()

        permanent_cut_at = time.monotonic()
        stop(restarted, timeout=8)
        assert restarted.returncode == 0, "The restarted fixture did not shut down cleanly"
        assert bridge.wait(timeout=25) != 0, "The bridge should fail when the recovery window expires"
        failed_at = time.monotonic()
        assert failed_at - permanent_cut_at >= 18, "The recovery window expired too early"
        assert "Failed(TimedOut)" in phase_log.read_text(encoding="utf-8")

        try:
            request(url)
            raise AssertionError("the stopped bridge still accepts HTTP requests")
        except URLError:
            pass
        return {
            "video": bridged_video,
            "audio": audio,
            "transport": args.transport,
            "authentication": args.auth,
            "gop_frames": args.gop,
            "max_extinf_seconds": max(durations),
            "bridge_ready_seconds": round(ready_seconds, 3),
            "decoded_seconds": 8,
            "media_sequence_before_restart": sequence(before),
            "media_sequence_after_restart": sequence(after),
            "origin_outage_seconds": round(published_at - listener_closed_at, 3),
            "recovery_after_republication_seconds": round(recovered_at - published_at, 3),
            "permanent_outage_failure_seconds": round(failed_at - permanent_cut_at, 3),
            "checks": ["get", "head", "range", "invalid_range", "host", "token",
                       "origin_listener_released", "origin_republished", "origin_reconnect",
                       "discontinuity", "no_live_endlist", "bounded_recovery", "listener_released"]
                      + (["missing_credentials", "incorrect_credentials"] if args.auth != "none" else []),
        }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mediamtx", default="mediamtx")
    parser.add_argument("--ffmpeg", default="ffmpeg")
    parser.add_argument("--ffprobe", default="ffprobe")
    parser.add_argument("--bridge", default="target/debug/examples/bridge")
    parser.add_argument("--video", choices=("h264", "hevc"), default="h264")
    parser.add_argument("--gop", type=int, default=30, help="Frames between IDRs in the 30 fps fixture.")
    parser.add_argument("--aac", action="store_true")
    parser.add_argument("--transport", choices=("tcp", "udp"), default="tcp")
    parser.add_argument("--auth", choices=AUTH_MODES, default="none")
    parser.add_argument("--udp-port", type=int, default=8000)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    result = verify(args)
    receipt = json.dumps(result, indent=2)
    (args.output / "receipt.json").write_text(receipt + "\n", encoding="utf-8")
    print(receipt)


if __name__ == "__main__":
    main()
