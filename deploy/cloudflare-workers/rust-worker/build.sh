#!/usr/bin/env bash
# wrangler's [build] command: set the wasm C toolchain, then worker-build.
set -euo pipefail
cd "$(dirname "$0")"
. ../env.sh
exec worker-build --release "$@"
