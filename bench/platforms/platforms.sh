#!/usr/bin/env bash
# Same load on every candidate at once, one size per window, so each
# platform's own billing metrics can be read back per size
# (bench/platforms/platforms_metrics.py). Windows start on a minute boundary and
# are followed by an idle gap: Cloudflare's per-minute usage buckets
# arrive lumpy, so a size's usage is summed over its window plus the gap.
#
#   DURATION=150 CONC=16 GAP=60 bench/platforms/platforms.sh <out-dir> name=url ...
#
# Each request is GET /resize/{w}/800/{size}.jpg@webp with w drawn from
# 700-799. A fixed URL would let the native server coalesce identical
# in-flight requests (measured: 3965 responses from 1113 pipeline runs),
# which the Workers build cannot do. /metrics is scraped around each
# window where a target serves it (the native image, OXIMG_METRICS=1).
set -euo pipefail
out=${1:?out dir}; shift
mkdir -p "$out"
DURATION=${DURATION:-150} CONC=${CONC:-16} GAP=${GAP:-60}
sizes=(${SIZES:-1mp 3mp 12mp})

wait_minute() { # sleep to 3 s past the next minute boundary
  local s; s=$(date -u +%S); sleep $(( (60 - 10#$s) % 60 + 3 ))
}

printf 'size\tstart\tend\n' >"$out/windows.tsv"
for size in "${sizes[@]}"; do
  wait_minute
  start=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  for t in "$@"; do
    name=${t%%=*}; url=${t#*=}
    curl -s -m 20 "$url/metrics" | grep '^oximg_' >"$out/$name-$size.before" || true
    # The path is a regex. `[.]`, not `\.`: oha's URL parsing turns a
    # backslash into a slash before the regex sees it.
    oha -z "${DURATION}s" -c "$CONC" --no-tui --output-format json --rand-regex-url \
      "$url/resize/7[0-9]{2}/800/$size[.]jpg@webp" >"$out/$name-$size.json" &
  done
  wait
  for t in "$@"; do
    name=${t%%=*}; url=${t#*=}
    curl -s -m 20 "$url/metrics" | grep '^oximg_' >"$out/$name-$size.after" || true
  done
  end=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  printf '%s\t%s\t%s\n' "$size" "$start" "$end" >>"$out/windows.tsv"
  echo "$start..$end $size done; idle ${GAP}s"
  sleep "$GAP"
done
