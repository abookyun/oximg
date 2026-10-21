#!/usr/bin/env bash
# Rebuild bench/quality/corpus/, the corpus run.py and QUALITY.md use,
# and verify it against corpus.sha256.
#
#   png/     the 24 Kodak images (768x512 PNG), from r0k.us
#   src/     the same, as q97 4:4:4 JPEG: the "Kodak 768px" group
#   large/   12 pinned picsum photographs at 4000x2667, as served (q75 4:2:0)
#   medium/  the large ones resized to 2000x1334 at q95
#
#   bench/quality/fetch_corpus.sh [--verify-only]
#
# Needs curl, magick and shasum. The derived JPEGs match by decoded
# pixels, not by bytes: ImageMagick versions write different EXIF.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
C=$HERE/corpus
PICSUM=(1015 1018 1024 1039 1043 1050 1062 110 111 116 129 164)

if [ "${1:-}" != --verify-only ]; then
  mkdir -p "$C/png" "$C/src" "$C/large" "$C/medium"
  for i in $(seq -w 1 24); do
    f=$C/png/kodim$i.png
    [ -s "$f" ] || curl -fsSL -o "$f" "http://r0k.us/graphics/kodak/kodak/kodim$i.png"
    [ -s "$C/src/kodim$i.jpg" ] || magick "$f" -quality 97 -sampling-factor 1x1 "$C/src/kodim$i.jpg"
  done
  for id in "${PICSUM[@]}"; do
    f=$C/large/picsum-$id.jpg
    [ -s "$f" ] || curl -fsSL -o "$f" "https://picsum.photos/id/$id/4000/2667"
    [ -s "$C/medium/picsum-$id.jpg" ] || magick "$f" -resize 2000x1334 -quality 95 "$C/medium/picsum-$id.jpg"
  done
fi

bad=0
while read -r kind path want; do
  case $kind in
    file) got=$(shasum -a 256 "$C/$path" | cut -c1-64) ;;
    pixels) got=$(magick "$C/$path" ppm:- | shasum -a 256 | cut -c1-64) ;;
    *) continue ;;
  esac
  if [ "$got" != "$want" ]; then
    echo "MISMATCH $kind $path" >&2
    bad=$((bad + 1))
  fi
done < <(grep -v '^#' "$HERE/corpus.sha256")
[ $bad -eq 0 ] && echo "corpus OK ($(grep -c '^[fp]' "$HERE/corpus.sha256") entries)" || { echo "$bad mismatches" >&2; exit 1; }
