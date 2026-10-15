# Platform comparison bench

Measures what each deployment target bills per transform and how long
it takes to start cold. Results and conclusions are in
[docs/deploy-targets.md](../../docs/deploy-targets.md); this is how to
reproduce them.

| File | |
|---|---|
| `origin/` | Static Worker serving the source JPEGs (`gen.sh` makes them: 1, 3 and 12 MP plasma fractals) |
| `platforms.sh` | Same load on every target at once, one source size per window, `/metrics` scraped around each window |
| `platforms_metrics.py` | Reads each platform's own billing metrics back for those windows |
| `cost.py` | Monthly USD from the measured per-transform usage, egress included |
| `coldstart.sh` | Cold vs warm latency of a Containers deployment after it sleeps |
| `wasm_simd_ab.mjs` | Interleaved A/B of two wasm builds on node's V8 |

## Run

```sh
bench/platforms/origin/gen.sh
(cd bench/platforms/origin && npm install --ignore-scripts && cf deploy)
# deploy the three targets (docs/deploy-cloud-run.md, docs/deploy-cloudflare.md)
# with OXIMG_METRICS=1, and upload origin/public/*.jpg to the Cloud Run bucket

DURATION=150 CONC=16 GAP=60 bench/platforms/platforms.sh out/ \
  workers=https://oximg-workers.<sub>.workers.dev \
  containers=https://oximg-containers.<sub>.workers.dev \
  cloudrun=https://oximg-<hash>.<region>.run.app

# a few minutes later, once usage has landed
CF_ACCOUNT_ID=… WORKERS_VERSION=… FRONT_VERSION=… CONTAINER_APP_ID=… \
DO_NAMESPACE_ID=… GCP_PROJECT=… \
python3 bench/platforms/platforms_metrics.py out/ 60
```

Copy the per-transform numbers into `cost.py`.

## Pitfalls this bench encodes

- **Distinct URLs.** Each request draws its width from 700–799
  (`oha --rand-regex-url`). Note `[.]` rather than `\.` in the regex:
  oha's URL parsing turns the backslash into a slash first. A fixed URL
  let the server coalesce 3965 responses into 1113 pipeline runs.
- **Usage per pipeline run, not per response.** Even 100 URLs at 16
  connections coalesced ~12% on one container. `platforms_metrics.py`
  reports pipeline runs from `/metrics` so usage can be divided by
  them.
- **Lumpy buckets.** Cloudflare's per-minute Containers usage arrives
  in uneven buckets. Windows are summed through the following idle gap,
  never read minute by minute.
- **Version ids, not names.** Workers analytics reports Workers
  deployed by `cf` as `scriptName` `__unknown__`, so they are told
  apart by version id.
- **Aborted requests can pin a container awake** (see
  docs/deploy-cloudflare.md, "Sleep"). oha aborts in-flight requests
  when `-z` expires. Check that the container sleeps before measuring
  cold starts.
