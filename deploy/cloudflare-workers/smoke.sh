#!/usr/bin/env bash
# POST each fixture to a running worker and compare the bytes with the
# native `transcode` example (cargo build --release --example transcode
# --no-default-features). Usage: smoke.sh http://127.0.0.1:8791
set -uo pipefail
base=${1:?worker base URL}
root=$(cd "$(dirname "$0")/../.." && pwd)
native=$root/target/release/examples/transcode
out=$(mktemp -d)
for c in "photo.jpg jpg" "photo.jpg webp" "photo.jpg png" "rgba.png webp" \
         "photo.webp jpg" "still.gif png" "cmyk_icc.jpg jpg" "gray16.png jpg" \
         "bogus_huffman.jpg jpg"; do
  set -- $c
  f=$root/tests/fixtures/$1
  code=$(curl -s -o "$out/w" -D "$out/h" -w '%{http_code}' --data-binary @"$f" \
    "$base/?w=400&h=400&fmt=$2")
  ms=$(grep -i x-oximg-pipeline-ms "$out/h" | tr -d '\r' | awk '{print $2}')
  same=-
  if [ "$code" = 200 ] && [ -x "$native" ]; then
    "$native" "$f" 400 400 "$2" "$out/n" >/dev/null 2>&1
    cmp -s "$out/w" "$out/n" && same=identical || same=DIFFERS
  fi
  printf '%-18s -> %-4s  http %s  %7s bytes  pipeline %4s ms  vs native: %s\n' \
    "$1" "$2" "$code" "$(wc -c <"$out/w" | tr -d ' ')" "${ms:--}" "$same"
  [ "$code" = 200 ] || head -c 200 "$out/w" && echo
done | grep -v '^$'
rm -rf "$out"
