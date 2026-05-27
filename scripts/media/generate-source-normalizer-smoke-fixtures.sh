#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FFMPEG_BIN="${FFMPEG_BIN:-ffmpeg}"
SOURCE="$REPO_ROOT/fixtures/media/tiny-h264-aac.m4v"
OUTPUT_ROOT="$REPO_ROOT/fixtures/media/generated"

if ! command -v "$FFMPEG_BIN" >/dev/null 2>&1; then
  echo "ffmpeg was not found. Set FFMPEG_BIN=/path/to/ffmpeg or install ffmpeg on PATH." >&2
  exit 127
fi

if [[ ! -f "$SOURCE" ]]; then
  echo "Missing source fixture: $SOURCE" >&2
  exit 1
fi

rm -rf "$OUTPUT_ROOT"
mkdir -p "$OUTPUT_ROOT/nonstandard" "$OUTPUT_ROOT/weird-dash"

"$FFMPEG_BIN" -hide_banner -loglevel error -y \
  -i "$SOURCE" \
  -map 0 -c copy -f flv \
  "$OUTPUT_ROOT/tiny-h264-aac.flv"

if "$FFMPEG_BIN" -hide_banner -loglevel error -y \
  -i "$SOURCE" \
  -map 0:v:0 -map 0:a:0? \
  -c:v libx265 -tag:v hvc1 -preset ultrafast -x265-params log-level=error \
  -c:a copy -f flv \
  "$OUTPUT_ROOT/tiny-hevc-aac.flv"; then
  :
else
  rm -f "$OUTPUT_ROOT/tiny-hevc-aac.flv"
  echo "warning: HEVC FLV fixture generation failed; supply fixtures/media/generated/tiny-hevc-aac.flv locally for that smoke case" >&2
fi

"$FFMPEG_BIN" -hide_banner -loglevel error -y \
  -i "$SOURCE" \
  -map 0 -c copy -movflags frag_keyframe+empty_moov+default_base_moof \
  "$OUTPUT_ROOT/tiny-broken-progressive.mp4"

"$FFMPEG_BIN" -hide_banner -loglevel error -y \
  -i "$SOURCE" \
  -map 0 -c copy \
  -f hls -hls_time 1 -hls_list_size 3 -hls_segment_type fmp4 \
  -hls_fmp4_init_filename init.mp4 \
  -hls_segment_filename "$OUTPUT_ROOT/nonstandard/segment_%05d.m4s" \
  "$OUTPUT_ROOT/nonstandard/index.m3u8"

"$FFMPEG_BIN" -hide_banner -loglevel error -y \
  -i "$SOURCE" \
  -map 0 -c copy \
  -f dash -seg_duration 1 -use_template 1 -use_timeline 0 \
  -init_seg_name 'init-$RepresentationID$.mp4' \
  -media_seg_name 'chunk-$RepresentationID$-$Number%05d$.m4s' \
  "$OUTPUT_ROOT/weird-dash/manifest.mpd"

cat <<EOF
Generated SourceNormalizer smoke fixtures under:
  $OUTPUT_ROOT

These files are intentionally small and can be regenerated at any time.
EOF
