#!/usr/bin/env bash
# Copy the crates that need a wasm32-unknown-unknown fix out of the cargo
# registry into vendor/ and apply patches/. vendor/ is gitignored: the
# jpegli-sys crate carries ~30 MB of libjxl source.
set -euo pipefail
cd "$(dirname "$0")"
reg=$(echo "${CARGO_HOME:-$HOME/.cargo}"/registry/src/index.crates.io-*)
rm -rf vendor && mkdir vendor
for p in patches/*.patch; do
  crate=$(basename "$p" .patch)              # e.g. mozjpeg-0.10.13
  name=${crate%-[0-9]*}                      # e.g. mozjpeg
  src=$reg/$crate
  [ -d "$src" ] || (cd ../.. && cargo fetch -q)
  cp -R "$src" "vendor/$name"
  patch -s -d "vendor/$name" -p1 <"$p"
  echo "vendored $crate"
done
