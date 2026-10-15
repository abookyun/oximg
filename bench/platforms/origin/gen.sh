#!/usr/bin/env bash
# Generate the benchmark sources (not committed: 4.3 MB of noise).
# Deterministic plasma fractals, JPEG q90/q85, via ImageMagick 7.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p public
magick -size 1200x900 plasma:fractal -seed 3 -quality 85 public/1mp.jpg
magick -size 2000x1500 plasma:fractal -seed 7 -quality 90 public/3mp.jpg
magick -size 4000x3000 plasma:fractal -seed 7 -quality 90 public/12mp.jpg
ls -l public
