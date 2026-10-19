#!/usr/bin/env bash
# Cold-start latency of the Containers candidate after it sleeps
# (sleepAfter in deploy/cloudflare-containers/src/index.js): idle 4 min, then one
# request, then a warm one for comparison. Repeats ROUNDS times.
#
#   ROUNDS=3 bench/platforms/coldstart.sh https://oximg-containers.<subdomain>.workers.dev
set -uo pipefail
url=${1:?url}
for i in $(seq 1 "${ROUNDS:-3}"); do
  sleep "${IDLE:-240}"
  for kind in cold warm; do
    curl -s -o /dev/null -m 120 -w "round $i $kind: %{http_code} total=%{time_total}s ttfb=%{time_starttransfer}s\n" \
      "$url/resize/800/800/1mp.jpg@webp"
  done
done
