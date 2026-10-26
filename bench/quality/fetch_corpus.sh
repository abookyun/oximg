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

# Each file appears under its final name only once complete, so an
# interrupted run leaves nothing a rerun would mistake for done.
fetch() {  # url path
  [ -s "$2" ] && return
  curl -fsSL -o "$2.part" "$1"
  mv "$2.part" "$2"
}
derive() {  # in out magick-args...
  local in=$1 out=$2
  shift 2
  [ -s "$out" ] && return
  magick "$in" "$@" "${out%.jpg}.part.jpg"
  mv "${out%.jpg}.part.jpg" "$out"
}

if [ "${1:-}" != --verify-only ]; then
  mkdir -p "$C/png" "$C/src" "$C/large" "$C/medium"
  for i in $(seq -w 1 24); do
    fetch "http://r0k.us/graphics/kodak/kodak/kodim$i.png" "$C/png/kodim$i.png"
    derive "$C/png/kodim$i.png" "$C/src/kodim$i.jpg" -quality 97 -sampling-factor 1x1
  done
  for id in "${PICSUM[@]}"; do
    fetch "https://picsum.photos/id/$id/4000/2667" "$C/large/picsum-$id.jpg"
    derive "$C/large/picsum-$id.jpg" "$C/medium/picsum-$id.jpg" -resize 2000x1334 -quality 95
  done
fi

# Read the manifest directly: a missing or unreadable one must fail the
# run, not verify zero files. Comment lines fall to the default case.
bad=0 n=0 listed=()
while read -r kind path want; do
  case $kind in
    file) got=$(shasum -a 256 "$C/$path" | cut -c1-64) ;;
    pixels) got=$(magick "$C/$path" ppm:- | shasum -a 256 | cut -c1-64) ;;
    *) continue ;;
  esac
  n=$((n + 1))
  listed+=("$path")
  if [ "$got" != "$want" ]; then
    echo "MISMATCH $kind $path" >&2
    bad=$((bad + 1))
  fi
done < "$HERE/corpus.sha256"
[ $n -gt 0 ] || { echo "no entries in corpus.sha256" >&2; exit 1; }

# run.py reads every *.jpg under src/, medium/ and large/, hidden ones
# included, so an extra file would change the benchmark's inputs.
while IFS= read -r f; do
  rel=${f#"$C/"}
  case " ${listed[*]} " in
    *" $rel "*) ;;
    *) echo "UNLISTED $rel" >&2; bad=$((bad + 1)) ;;
  esac
done < <(find "$C/src" "$C/medium" "$C/large" -name '*.jpg' 2>/dev/null)

[ $bad -eq 0 ] || { echo "$bad problems" >&2; exit 1; }
echo "corpus OK ($n entries)"
