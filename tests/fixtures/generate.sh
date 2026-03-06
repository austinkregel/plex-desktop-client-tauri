#!/bin/bash
# Generate the multi-track test fixture for integration tests.
# Requires: ffmpeg with libvpx and libvorbis support.
#
# Output: multi_track.mkv
#   - 1 video track  (VP8, 160x120, 2 seconds of blue)
#   - 3 audio tracks (Vorbis, 8kHz)
#       Track 0: 440 Hz tone, language=jpn, title="Japanese"
#       Track 1: 880 Hz tone, language=eng, title="English"
#       Track 2: 1320 Hz tone, language=spa, title="Spanish"
#   - 2 subtitle tracks (SRT)
#       Track 0: language=eng, title="English"
#       Track 1: language=spa, title="Spanish"
#
# Safety: uses -hwaccel none and -threads 1 to avoid GPU/CPU overload.

set -euo pipefail
cd "$(dirname "$0")"

ffmpeg -hwaccel none -threads 1 -nostdin -y \
  -f lavfi -i "color=c=blue:s=160x120:d=2,format=yuv420p" \
  -f lavfi -i "sine=frequency=440:duration=2:sample_rate=8000" \
  -f lavfi -i "sine=frequency=880:duration=2:sample_rate=8000" \
  -f lavfi -i "sine=frequency=1320:duration=2:sample_rate=8000" \
  -f srt -i eng.srt \
  -f srt -i spa.srt \
  -map 0 -map 1 -map 2 -map 3 -map 4 -map 5 \
  -metadata:s:a:0 language=jpn -metadata:s:a:0 title="Japanese" \
  -metadata:s:a:1 language=eng -metadata:s:a:1 title="English" \
  -metadata:s:a:2 language=spa -metadata:s:a:2 title="Spanish" \
  -metadata:s:s:0 language=eng -metadata:s:s:0 title="English" \
  -metadata:s:s:1 language=spa -metadata:s:s:1 title="Spanish" \
  -c:v libvpx -b:v 50k -threads 1 \
  -c:a libvorbis -b:a 32k \
  -c:s srt \
  multi_track.mkv

echo "Generated multi_track.mkv ($(du -h multi_track.mkv | cut -f1))"
